package ovh.kweb.mail.ui

import android.app.Application
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import ovh.kweb.mail.background.Background

/** Navigation and the pending "show this message" request. The back stack never gets empty. */
class AppViewModel(app: Application) : AndroidViewModel(app) {
    private val stackFlow = MutableStateFlow(listOf(Route.Inbox))
    private val focusFlow = MutableStateFlow<FocusRequest?>(null)
    private var serial = 0L

    private val serverUrlFlow = MutableStateFlow<String?>(null)

    val stack: StateFlow<List<Route>> get() = stackFlow
    val focus: StateFlow<FocusRequest?> get() = focusFlow

    /** Address of the signed-in server, to prefill the field when signing in again. */
    val serverUrl: StateFlow<String?> get() = serverUrlFlow

    init {
        viewModelScope.launch { SessionState.load(app) }
        viewModelScope.launch {
            SessionState.signedIn.collect { signedIn ->
                if (signedIn == false) {
                    stackFlow.value = listOf(Route.Inbox)
                    focusFlow.value = null
                    serverUrlFlow.value = null
                } else if (signedIn == true) {
                    serverUrlFlow.value = withContext(Dispatchers.IO) { Background.core(app).account()?.serverUrl }
                }
            }
        }
    }

    fun open(route: Route) = stackFlow.update { if (it.last() == route) it else it + route }

    fun back() = stackFlow.update { if (it.size > 1) it.dropLast(1) else it }

    /** The activity was opened from the notification of [messageId]. */
    fun focusMessage(messageId: String) {
        stackFlow.value = listOf(Route.Inbox)
        focusFlow.value = FocusRequest(messageId, ++serial)
    }

    /** The Inbox has scrolled to the request, or given up on it. Later requests are kept. */
    fun focusHandled(request: FocusRequest) = focusFlow.update { if (it == request) null else it }
}
