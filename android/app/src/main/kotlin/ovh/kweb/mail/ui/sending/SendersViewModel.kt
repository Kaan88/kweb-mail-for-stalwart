package ovh.kweb.mail.ui.sending

import android.app.Application
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import ovh.kweb.mail.background.Background
import ovh.kweb.mail.ui.SessionState

/** The senders screens' state. It lives as long as the route (see ScreenStores), so a rotation keeps the open form. */
class SendersViewModel(app: Application) : AndroidViewModel(app) {
    val controller = SendersController(
        scope = viewModelScope,
        io = Dispatchers.IO,
        backend = CoreSendersBackend { Background.core(app) },
    )

    init {
        viewModelScope.launch {
            SessionState.signedIn.collect { signedIn -> if (signedIn == false) controller.reset() }
        }
    }
}
