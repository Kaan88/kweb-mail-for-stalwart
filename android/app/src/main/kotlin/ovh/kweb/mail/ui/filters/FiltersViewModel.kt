package ovh.kweb.mail.ui.filters

import android.app.Application
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import ovh.kweb.mail.background.Background
import ovh.kweb.mail.ui.SessionState

/** The filter screens' state. It lives as long as the route (see ScreenStores), so a rotation keeps the open rule. */
class FiltersViewModel(app: Application) : AndroidViewModel(app) {
    val controller = FiltersController(
        scope = viewModelScope,
        io = Dispatchers.IO,
        backend = CoreFiltersBackend { Background.core(app) },
    )

    init {
        viewModelScope.launch {
            SessionState.signedIn.collect { signedIn -> if (signedIn == false) controller.reset() }
        }
    }
}
