package ovh.kweb.mail.ui.sharing

import ovh.kweb.mail.accounts.AccountSession
import android.app.Application
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import ovh.kweb.mail.background.Background
import ovh.kweb.mail.ui.SessionState
import uniffi.kweb_core.ShareKind

/** The Share screen's state. It lives as long as its route (see ScreenStores), so a rotation keeps the typed text. */
class ShareViewModel(app: Application, private val session: AccountSession, kind: ShareKind, id: String) : AndroidViewModel(app) {
    val controller = ShareController(viewModelScope, Dispatchers.IO, CoreSharingBackend { session.core }, kind, id)

    init {
        viewModelScope.launch { SessionState.signedIn.collect { if (it == false) controller.reset() } }
    }
}
