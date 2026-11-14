package ovh.kweb.mail.ui.folders

import android.app.Application
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import ovh.kweb.mail.background.Background
import ovh.kweb.mail.background.CheckScheduler
import ovh.kweb.mail.ui.SessionState

/** The folder screen's state. It lives as long as the route (see ScreenStores), so a rotation keeps the open dialog and what was typed. */
class FoldersViewModel(app: Application) : AndroidViewModel(app) {
    val controller = FoldersController(
        scope = viewModelScope,
        io = Dispatchers.IO,
        backend = CoreFoldersBackend({ Background.core(app) }, { CheckScheduler.checkSoon(app) }),
    )

    init {
        viewModelScope.launch {
            SessionState.signedIn.collect { signedIn -> if (signedIn == false) controller.reset() }
        }
        // A sync can change the folders (and their counts) under an open screen.
        viewModelScope.launch { Background.cacheChanges.collect { controller.refresh() } }
    }
}
