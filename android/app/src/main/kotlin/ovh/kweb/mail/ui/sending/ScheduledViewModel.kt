package ovh.kweb.mail.ui.sending

import ovh.kweb.mail.accounts.AccountSession
import android.app.Application
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import ovh.kweb.mail.background.Background
import ovh.kweb.mail.ui.SessionState

/** The state of the held-messages screen. It lives as long as the route (see ScreenStores). */
class ScheduledViewModel(app: Application, private val session: AccountSession) : AndroidViewModel(app) {
    val controller = ScheduledController(
        scope = viewModelScope,
        io = Dispatchers.IO,
        backend = CoreScheduledBackend { session.core },
    )

    init {
        viewModelScope.launch {
            SessionState.signedIn.collect { signedIn -> if (signedIn == false) controller.reset() }
        }
    }
}
