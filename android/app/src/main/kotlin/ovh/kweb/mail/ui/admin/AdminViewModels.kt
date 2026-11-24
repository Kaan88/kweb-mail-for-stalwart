package ovh.kweb.mail.ui.admin

import android.app.Application
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import ovh.kweb.mail.accounts.AccountSession
import ovh.kweb.mail.ui.SessionState
import uniffi.kweb_core.AdminAccess

/**
 * The view models of the admin screens. Each lives as long as its route (see ScreenStores) and is given the
 * session of its account when it is made, so a later switch of the account cannot send what it does to another
 * account's core. A signed-out app clears what a screen holds.
 */
private fun backendOf(session: AccountSession): AdminBackend = CoreAdminBackend { session.core }

/** Keeps the last known access of the account (two flags, never the key) for the drawer's entry. */
private fun rememberOf(session: AccountSession): (AdminAccess) -> Unit =
    { session.settings.rememberAdminAccess(AdminEntryRules.keyAccepted(it), it.mayDashboard) }

class AdminRowViewModel(app: Application, session: AccountSession) : AndroidViewModel(app) {
    val access = AccessController(viewModelScope, Dispatchers.IO, backendOf(session), onKnown = rememberOf(session))

    override fun onCleared() = access.clear()
}

class AdminAccessViewModel(app: Application, session: AccountSession) : AndroidViewModel(app) {
    val access = AccessController(viewModelScope, Dispatchers.IO, backendOf(session), onKnown = rememberOf(session))

    init {
        viewModelScope.launch { SessionState.signedIn.collect { if (it == false) access.clear() } }
    }

    override fun onCleared() = access.clear()
}

class AdminHomeViewModel(app: Application, session: AccountSession) : AndroidViewModel(app) {
    val home = HomeController(viewModelScope, Dispatchers.IO, backendOf(session), rememberOf(session))

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

class AdminDomainsViewModel(app: Application, session: AccountSession) : AndroidViewModel(app) {
    val domains = DomainsController(viewModelScope, Dispatchers.IO, backendOf(session))
}

class AdminDomainViewModel(app: Application, session: AccountSession) : AndroidViewModel(app) {
    private val backend = backendOf(session)
    var controller: DomainController? = null
        private set

    fun forDomain(id: String): DomainController =
        controller ?: DomainController(viewModelScope, Dispatchers.IO, backend, id).also { controller = it }
}

class AdminReportsViewModel(app: Application, session: AccountSession) : AndroidViewModel(app) {
    val reports = ReportsController(viewModelScope, Dispatchers.IO, backendOf(session))
}

class AdminReportViewModel(app: Application, session: AccountSession) : AndroidViewModel(app) {
    private val backend = backendOf(session)
    var controller: ReportController? = null
        private set

    fun forReport(kind: uniffi.kweb_core.ReportKind, id: String): ReportController =
        controller ?: ReportController(viewModelScope, Dispatchers.IO, backend, kind, id).also { controller = it }
}

class AdminTasksViewModel(app: Application, session: AccountSession) : AndroidViewModel(app) {
    val tasks = TasksController(viewModelScope, Dispatchers.IO, backendOf(session))
}

class AdminClusterViewModel(app: Application, session: AccountSession) : AndroidViewModel(app) {
    val cluster = ClusterController(viewModelScope, Dispatchers.IO, backendOf(session))
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
