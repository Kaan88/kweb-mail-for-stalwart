package ovh.kweb.mail.ui.accounts

import android.app.Application
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.SharingStarted
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.flow.conflate
import kotlinx.coroutines.flow.flowOn
import kotlinx.coroutines.flow.stateIn
import ovh.kweb.mail.accounts.Accounts
import uniffi.kweb_core.ConnectionStatus

/**
 * The accounts as the switcher shows them: per device, not per account. The unread count of each Inbox is a
 * read of that account's own cache ([uniffi.kweb_core.Core.mailboxes]), repeated whenever its cache changed.
 */
class AccountSwitcherViewModel(app: Application) : AndroidViewModel(app) {
    val rows: StateFlow<List<AccountRow>> = combine(
        Accounts.records,
        Accounts.active,
        Accounts.connection,
        Accounts.cacheTicks,
    ) { records, active, connection, _ ->
        val needs = connection.items.filter { it.status == ConnectionStatus.SignInNeeded }.map { it.record.id }.toSet()
        // Opening a session may open its cache; reading the mailboxes is a cache read. Both off the main thread.
        val unread = HashMap<String, Int>()
        for (r in records) {
            val inbox = runCatching { Accounts.session(r.id)?.core?.mailboxes()?.firstOrNull { it.role == "inbox" } }.getOrNull()
            if (inbox != null) unread[r.id] = inbox.unreadEmails.toInt()
        }
        AccountRows.build(records, active?.id, unread, needs)
    }.conflate().flowOn(Dispatchers.IO).stateIn(viewModelScope, SharingStarted.Eagerly, emptyList())
}
