package ovh.kweb.mail.ui.enterprise

import android.app.Application
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import ovh.kweb.mail.accounts.AccountSession
import ovh.kweb.mail.ui.SessionState
import uniffi.kweb_core.Tenant

/*
 * One view model per screen. Each is given the session of its account when it is built and keeps it, and lives as
 * long as its route is on the back stack (see ScreenStores), so a rotation keeps what was typed and the open question.
 * Signing out clears what the core handed out.
 */

class DashboardViewModel(app: Application, session: AccountSession) : AndroidViewModel(app) {
    private val backend = CoreEnterpriseBackend { session.core }
    val controller = DashboardController(viewModelScope, Dispatchers.IO, backend)

    init {
        viewModelScope.launch { SessionState.signedIn.collect { if (it == false) controller.reset() } }
    }

    override fun onCleared() {
        controller.shutdown()
    }
}

class DeliveriesViewModel(app: Application, session: AccountSession) : AndroidViewModel(app) {
    val controller = DeliveriesController(viewModelScope, Dispatchers.IO, CoreEnterpriseBackend { session.core })

    init {
        viewModelScope.launch { SessionState.signedIn.collect { if (it == false) controller.reset() } }
    }
}

class DeliveryDetailViewModel(app: Application, session: AccountSession) : AndroidViewModel(app) {
    val controller = DeliveryDetailController(viewModelScope, Dispatchers.IO, CoreEnterpriseBackend { session.core })

    init {
        viewModelScope.launch { SessionState.signedIn.collect { if (it == false) controller.reset() } }
    }
}

class RecoveryViewModel(app: Application, session: AccountSession) : AndroidViewModel(app) {
    val controller = RecoveryController(viewModelScope, Dispatchers.IO, CoreEnterpriseBackend { session.core })

    init {
        viewModelScope.launch { SessionState.signedIn.collect { if (it == false) controller.reset() } }
    }
}

class AlertsViewModel(app: Application, session: AccountSession) : AndroidViewModel(app) {
    val controller = AlertsController(viewModelScope, Dispatchers.IO, CoreEnterpriseBackend { session.core })

    init {
        viewModelScope.launch { SessionState.signedIn.collect { if (it == false) controller.reset() } }
    }
}

class TenantsViewModel(app: Application, session: AccountSession) : AndroidViewModel(app) {
    private val backend = CoreEnterpriseBackend { session.core }
    val controller = LoadController<List<Tenant>>(viewModelScope, Dispatchers.IO, ovh.kweb.mail.R.string.ent_tenants_failed) { backend.tenants() }

    init {
        viewModelScope.launch { SessionState.signedIn.collect { if (it == false) controller.reset() } }
    }
}
