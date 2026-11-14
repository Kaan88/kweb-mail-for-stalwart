package ovh.kweb.mail.ui.compose

import android.content.Context
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.receiveAsFlow
import kotlinx.coroutines.withContext
import ovh.kweb.mail.background.Background
import uniffi.kweb_core.DraftKind
import uniffi.kweb_core.PendingSend

/**
 * Hands a message that was just sent from the compose screen to the mail list, which shows
 * "Sending…" with Undo until the core lets it go. A process-wide channel and not a field of a view
 * model because the two screens do not know each other and the list is not on screen while the
 * message is written. One reader: the list's view model.
 */
object Outbox {
    private val channel = Channel<PendingSend>(Channel.BUFFERED)

    val sent: Flow<PendingSend> get() = channel.receiveAsFlow()

    fun announce(pending: PendingSend) {
        channel.trySend(pending)
    }
}

/** How long Undo is offered, as the list works it out from the core's answer. */
object SendRules {
    private const val HOLD_MILLIS = 10_000L

    /**
     * How much of the hold-back is left for a message that goes out at [sendAtSeconds]: at most the
     * whole hold-back (a clock that is off must not keep the snackbar for minutes), zero when it has gone.
     */
    fun remainingMillis(sendAtSeconds: Long, nowMillis: Long): Long =
        (sendAtSeconds * 1000 - nowMillis).coerceIn(0, HOLD_MILLIS)
}

/** Starts drafts for every way into the compose screen. */
object DraftLauncher {
    /** Asks the core for a draft of [kind] (it is stored at once) and returns its id. Throws what the core throws. */
    suspend fun create(context: Context, kind: DraftKind): String =
        withContext(Dispatchers.IO) { Background.core(context).newDraft(kind).id }
}
