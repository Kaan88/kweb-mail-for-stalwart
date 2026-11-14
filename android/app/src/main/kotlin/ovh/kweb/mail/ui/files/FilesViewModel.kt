package ovh.kweb.mail.ui.files

import ovh.kweb.mail.accounts.AccountSession
import android.app.Application
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import ovh.kweb.mail.background.Background
import ovh.kweb.mail.ui.SessionState

/**
 * The Files area's state. It lives as long as the activity, so that a visit to another root, a rotation or
 * a running upload keep the folder, the dialogs and the queue. The logic is in [FilesController].
 */
class FilesViewModel(app: Application, private val session: AccountSession) : AndroidViewModel(app) {
    val controller = FilesController(
        scope = viewModelScope,
        io = Dispatchers.IO,
        backend = CoreFilesBackend { session.core },
        platform = AndroidFilesPlatform(app, session.record.id),
    )

    init {
        viewModelScope.launch {
            SessionState.signedIn.collect { signedIn -> if (signedIn == false) controller.reset() }
        }
    }
}
