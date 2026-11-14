package ovh.kweb.mail.ui.filters

import ovh.kweb.mail.accounts.AccountSession
import android.app.Application
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import ovh.kweb.mail.background.Background
import ovh.kweb.mail.ui.SessionState

/** The filter screens' state. It lives as long as the route (see ScreenStores), so a rotation keeps the open rule. */
class FiltersViewModel(app: Application, private val session: AccountSession) : AndroidViewModel(app) {
    val controller = FiltersController(
        scope = viewModelScope,
        io = Dispatchers.IO,
        backend = CoreFiltersBackend { session.core },
    )

    init {
        viewModelScope.launch {
            SessionState.signedIn.collect { signedIn -> if (signedIn == false) controller.reset() }
        }
    }
}
