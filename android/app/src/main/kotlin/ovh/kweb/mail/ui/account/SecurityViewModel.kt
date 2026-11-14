package ovh.kweb.mail.ui.account

import android.app.Application
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import ovh.kweb.mail.accounts.AccountSession
import ovh.kweb.mail.ui.SessionState

/** The account security screen's state. It lives as long as the route (see ScreenStores). */
class SecurityViewModel(app: Application, private val session: AccountSession) : AndroidViewModel(app) {
    val controller = SecurityController(
        scope = viewModelScope,
        io = Dispatchers.IO,
        backend = CoreAccountBackend { session.core },
    )

    init {
        viewModelScope.launch {
            SessionState.signedIn.collect { signedIn -> if (signedIn == false) controller.reset() }
        }
    }

    override fun onCleared() {
        // The screen is gone: nothing a dialog still held (a secret shown once) stays behind.
        controller.dismissAll()
    }
}
