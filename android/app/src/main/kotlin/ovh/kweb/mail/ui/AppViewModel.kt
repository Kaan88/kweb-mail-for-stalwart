package ovh.kweb.mail.ui

import android.app.Application
import android.util.Log
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import ovh.kweb.mail.background.Background

/** Navigation. The back stack never gets empty. */
class AppViewModel(app: Application) : AndroidViewModel(app) {
    private val stackFlow = MutableStateFlow(BackStack.initial)

    private val serverUrlFlow = MutableStateFlow<String?>(null)

    val stack: StateFlow<List<Route>> get() = stackFlow

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
