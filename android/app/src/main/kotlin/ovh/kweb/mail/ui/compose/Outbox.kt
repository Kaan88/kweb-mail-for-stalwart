package ovh.kweb.mail.ui.compose

import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.filter
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.flow.receiveAsFlow
import kotlinx.coroutines.withContext
import uniffi.kweb_core.Core
import uniffi.kweb_core.Address
import uniffi.kweb_core.DraftKind
import uniffi.kweb_core.PendingSend

/**
 * Hands a message that was just sent from the compose screen to the mail list, which shows
 * "Sending…" with Undo until the core lets it go. A process-wide channel and not a field of a view
 * model because the two screens do not know each other and the list is not on screen while the
 * message is written. One reader: the list's view model.
 */
object Outbox {
    /** A message sent from account [accountId]. */
    class Sent(val accountId: String, val pending: PendingSend)

    private val channel = Channel<Sent>(Channel.BUFFERED)

    /** What was sent from [accountId]; the list of another account does not show it (its Undo would be for the wrong account). */
    fun sent(accountId: String): Flow<PendingSend> = channel.receiveAsFlow().filter { it.accountId == accountId }.map { it.pending }

    fun announce(accountId: String, pending: PendingSend) {
        channel.trySend(Sent(accountId, pending))
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
    suspend fun create(core: Core, kind: DraftKind): String =
        withContext(Dispatchers.IO) { core.newDraft(kind).id }

    /** Throws the draft [id] away again; a failure is not worth more than a log line, so none is raised. */
    suspend fun discard(core: Core, id: String) {
        withContext(Dispatchers.IO) { runCatching { core.discardDraft(id) } }
    }
}
