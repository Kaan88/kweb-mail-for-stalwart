package ovh.kweb.mail.ui.admin

import android.app.Application
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import ovh.kweb.mail.accounts.AccountSession
import ovh.kweb.mail.ui.SessionState

/**
 * The view models of the admin screens. Each lives as long as its route (see ScreenStores) and is given the
 * session of its account when it is made, so a later switch of the account cannot send what it does to another
 * account's core. A signed-out app clears what a screen holds.
 */
private fun backendOf(session: AccountSession): AdminBackend = CoreAdminBackend { session.core }

class AdminRowViewModel(app: Application, session: AccountSession) : AndroidViewModel(app) {
    val access = AccessController(viewModelScope, Dispatchers.IO, backendOf(session))

    override fun onCleared() = access.clear()
}

class AdminAccessViewModel(app: Application, session: AccountSession) : AndroidViewModel(app) {
    val access = AccessController(viewModelScope, Dispatchers.IO, backendOf(session))

    init {
        viewModelScope.launch { SessionState.signedIn.collect { if (it == false) access.clear() } }
    }

    override fun onCleared() = access.clear()
}

class AdminHomeViewModel(app: Application, session: AccountSession) : AndroidViewModel(app) {
    val home = HomeController(viewModelScope, Dispatchers.IO, backendOf(session))

    override fun onCleared() = home.access.clear()
}

class AdminQueueViewModel(app: Application, session: AccountSession) : AndroidViewModel(app) {
    val queue = QueueController(viewModelScope, Dispatchers.IO, backendOf(session))
}

class AdminQueuedMessageViewModel(app: Application, session: AccountSession) : AndroidViewModel(app) {
    // The id comes with the screen, which sets it once.
    private val backend = backendOf(session)
    var controller: QueuedMessageController? = null
        private set

    fun forMessage(id: String): QueuedMessageController =
        controller ?: QueuedMessageController(viewModelScope, Dispatchers.IO, backend, id).also { controller = it }
}

class AdminAccountsViewModel(app: Application, session: AccountSession) : AndroidViewModel(app) {
    val accounts = AccountsController(viewModelScope, Dispatchers.IO, backendOf(session))
}

class AdminAccountViewModel(app: Application, session: AccountSession) : AndroidViewModel(app) {
    private val backend = backendOf(session)
    var controller: AccountController? = null
        private set

    fun forAccount(id: String): AccountController =
        controller ?: AccountController(viewModelScope, Dispatchers.IO, backend, id).also { controller = it }

    override fun onCleared() {
        controller?.clear()
    }
}

class AdminNewAccountViewModel(app: Application, session: AccountSession) : AndroidViewModel(app) {
    val form = NewAccountController(viewModelScope, Dispatchers.IO, backendOf(session))

    init {
        viewModelScope.launch { SessionState.signedIn.collect { if (it == false) form.clear() } }
    }

    override fun onCleared() = form.clear()
}

class AdminBlockedViewModel(app: Application, session: AccountSession) : AndroidViewModel(app) {
    val blocked = BlockedController(viewModelScope, Dispatchers.IO, backendOf(session))
}

class AdminLogsViewModel(app: Application, session: AccountSession) : AndroidViewModel(app) {
    val logs = LogsController(viewModelScope, Dispatchers.IO, backendOf(session))
}

class AdminMaintenanceViewModel(app: Application, session: AccountSession) : AndroidViewModel(app) {
    val maintenance = MaintenanceController(viewModelScope, Dispatchers.IO, backendOf(session))
}
