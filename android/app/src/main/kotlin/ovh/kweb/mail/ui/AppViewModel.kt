package ovh.kweb.mail.ui

import android.app.Application
import android.util.Log
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.receiveAsFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import ovh.kweb.mail.R
import ovh.kweb.mail.background.Background
import ovh.kweb.mail.background.Notifier
import ovh.kweb.mail.ui.compose.DraftLauncher
import uniffi.kweb_core.DraftKind

/** Navigation. The back stack never gets empty. */
class AppViewModel(app: Application) : AndroidViewModel(app) {
    private val stackFlow = MutableStateFlow(BackStack.initial)

    private val serverUrlFlow = MutableStateFlow<String?>(null)

    private val messageChannel = Channel<Int>(Channel.BUFFERED)

    val stack: StateFlow<List<Route>> get() = stackFlow

    /** Short texts for a snackbar, as string resource ids. */
    val messages: Flow<Int> get() = messageChannel.receiveAsFlow()

    /** Address of the signed-in server, to prefill the field when signing in again. */
    val serverUrl: StateFlow<String?> get() = serverUrlFlow

    init {
        viewModelScope.launch { SessionState.load(app) }
        viewModelScope.launch {
            SessionState.signedIn.collect { signedIn ->
                if (signedIn == false) {
                    stackFlow.value = BackStack.initial
                    serverUrlFlow.value = null
                } else if (signedIn == true) {
                    serverUrlFlow.value = withContext(Dispatchers.IO) { Background.core(app).account()?.serverUrl }
                }
            }
        }
    }

    fun open(route: Route) = stackFlow.update { BackStack.push(it, route) }

    fun back() = stackFlow.update { BackStack.pop(it) }

    /** A message was handed over for sending: back to the list. */
    fun sent() {
        stackFlow.value = BackStack.afterSend()
    }

    /**
     * The Reply button of a mail notification (notification [androidId], 0 when unknown): start the
     * answer and show it over the list. When the draft cannot be made (not signed in, the mail is
     * gone) the screen stays, a message says so and the notification stays too. Only on success is
     * the notification taken away and the core told that was on purpose.
     */
    fun replyTo(messageId: String, androidId: Int) {
        viewModelScope.launch {
            val draftId = try {
                DraftLauncher.create(getApplication(), DraftKind.Reply(messageId))
            } catch (e: CancellationException) {
                throw e
            } catch (e: kotlin.Exception) {
                Log.w(Background.TAG, "starting the answer from a notification failed", e)
                messageChannel.send(R.string.compose_start_failed)
                return@launch
            }
            stackFlow.value = BackStack.forCompose(draftId)
            val app = getApplication<Application>()
            withContext(Dispatchers.IO) {
                runCatching { Background.core(app).notificationSwiped(messageId) }
                if (androidId != 0) runCatching { Notifier(app).cancelMail(androidId) }
            }
        }
    }

    /**
     * The activity was opened from the notification of [messageId]: show that message over the
     * list. A message the cache does not have (it should) leaves just the list.
     */
    fun focusMessage(messageId: String) {
        viewModelScope.launch {
            val stack = withContext(Dispatchers.IO) {
                try {
                    val core = Background.core(getApplication())
                    val header = core.message(messageId)
                    val inboxId = core.mailboxes().firstOrNull { it.role == "inbox" }?.id
                    if (header == null) BackStack.initial else BackStack.forNotification(header.threadId, inboxId, messageId)
                } catch (e: kotlin.Exception) {
                    Log.w(Background.TAG, "looking up the tapped message failed", e)
                    BackStack.initial
                }
            }
            stackFlow.value = stack
        }
    }
}
