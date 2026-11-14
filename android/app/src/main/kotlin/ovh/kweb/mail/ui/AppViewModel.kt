package ovh.kweb.mail.ui

import android.app.Application
import android.util.Log
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.first
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

    /** The state of the screens that live as long as their route is on the stack. */
    val stores = ScreenStores()

    private val serverUrlFlow = MutableStateFlow<String?>(null)

    private val messageChannel = Channel<Int>(Channel.BUFFERED)

    val stack: StateFlow<List<Route>> get() = stackFlow

    /** Short texts for a snackbar, as string resource ids. */
    val messages: Flow<Int> get() = messageChannel.receiveAsFlow()

    /** Address of the signed-in server, to prefill the field when signing in again. */
    val serverUrl: StateFlow<String?> get() = serverUrlFlow

    init {
        viewModelScope.launch { SessionState.load(app) }
        viewModelScope.launch { stackFlow.collect { stores.retain(it) } }
        viewModelScope.launch {
            var wasOut = false
            SessionState.signedIn.collect { signedIn ->
                if (signedIn == false) {
                    wasOut = true
                    stackFlow.value = BackStack.initial
                    serverUrlFlow.value = null
                } else if (signedIn == true) {
                    // Whatever was asked while signed out must not wait under the new session.
                    if (wasOut) stackFlow.value = BackStack.initial
                    wasOut = false
                    serverUrlFlow.value = withContext(Dispatchers.IO) { Background.core(app).account()?.serverUrl }
                }
            }
        }
    }

    override fun onCleared() {
        stores.clear()
    }

    fun open(route: Route) = stackFlow.update { BackStack.push(it, route) }

    /** The root screen a tab of the bottom bar leads to. Each side keeps its own state in its view model. */
    fun switchTo(root: Route) {
        if (SessionState.signedIn.value != true) return
        stackFlow.value = BackStack.switchTo(root)
    }

    /**
     * The activity was opened from the reminder of [eventId] (for [recurrenceId] when it repeats): show that
     * event over the agenda. Waits for the first answer of the session; a signed-out app ignores it.
     */
    fun openReminder(eventId: String, recurrenceId: String?) {
        viewModelScope.launch {
            val signedIn = SessionState.signedIn.first { it != null }
            if (signedIn == true) stackFlow.update { BackStack.showOver(it, BackStack.forReminder(eventId, recurrenceId)) }
        }
    }

    /** An entry was saved: back to the agenda. It reads again when it comes up. */
    fun eventSaved() = stackFlow.update { BackStack.afterEventSaved(it) }

    /** A contact was saved: the form is gone; a changed contact shows again, a new one leaves the list. */
    fun contactSaved() = stackFlow.update { BackStack.afterContactSaved(it) }

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
            stackFlow.update { BackStack.showOver(it, BackStack.forCompose(draftId)) }
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
            stackFlow.update { BackStack.showOver(it, stack) }
        }
    }
}
