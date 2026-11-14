package ovh.kweb.mail.ui.shared

import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.NonCancellable
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.ensureActive
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.receiveAsFlow
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import ovh.kweb.mail.R
import ovh.kweb.mail.background.Background
import ovh.kweb.mail.ui.UiMessage
import uniffi.kweb_core.DraftKind

/**
 * Starts the user's own answer to, or forward of, a message of a shared folder, and has the screen open it. One at a
 * time, so a double tap makes one draft. A draft nobody gets to open (the screen was left meanwhile) is thrown away again.
 */
class SharedDraftStarter(
    private val scope: CoroutineScope,
    private val io: CoroutineDispatcher,
    private val backend: SharedWriteBackend,
    private val account: String,
) {
    private val opened = Channel<String>(Channel.BUFFERED)
    private val failed = Channel<UiMessage>(Channel.BUFFERED)
    private var starting = false
    private val working = MutableStateFlow(false)

    /** True while a draft is being made (a forward copies its attachments), for a progress sign. */
    val busy: StateFlow<Boolean> get() = working

    /** The id of a draft that was started: open it in the compose screen. */
    val draftStarted: Flow<String> get() = opened.receiveAsFlow()

    /** Why a draft could not be started. */
    val problems: Flow<UiMessage> get() = failed.receiveAsFlow()

    /** [kind] names the message (an id in the shared account) and is a reply, reply to all or forward. */
    fun start(kind: DraftKind) {
        if (starting) return
        starting = true
        working.value = true
        scope.launch {
            var draftId: String? = null
            var handedOver = false
            try {
                // Not cancellable, and the id is kept from inside: withContext drops its result when the scope was
                // cancelled meanwhile, and a draft the core made must reach the discard below, or the screen.
                withContext(NonCancellable + io) { draftId = backend.newDraftFromShared(account, kind) }
                ensureActive()
                opened.send(draftId!!)
                handedOver = true
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                runCatching { android.util.Log.w(Background.TAG, "starting an answer to a shared message failed: ${e.javaClass.simpleName}") }
                failed.trySend(SharedRules.message(e, R.string.compose_start_failed))
            } finally {
                val orphan = draftId
                if (orphan != null && !handedOver) withContext(NonCancellable + io) { runCatching { backend.discardDraft(orphan) } }
                starting = false
                working.value = false
            }
        }
    }
}
