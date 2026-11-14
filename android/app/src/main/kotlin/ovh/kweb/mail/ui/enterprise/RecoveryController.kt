package ovh.kweb.mail.ui.enterprise

import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import ovh.kweb.mail.R
import ovh.kweb.mail.ui.UiMessage
import uniffi.kweb_core.DeletedItem
import uniffi.kweb_core.RecoveryState

/** A question on top of the screen. What was typed for another account is part of it, so it survives a rotation. */
sealed interface RecoveryDialog {
    /** [query] is what is typed to search the server's accounts, [matches] what the server found for it. */
    data class Account(
        val choice: AccountChoice,
        val error: UiMessage? = null,
        val query: String = "",
        val matches: List<AccountMatch> = emptyList(),
        val searching: Boolean = false,
    ) : RecoveryDialog

    data class Restore(val item: DeletedItem) : RecoveryDialog
}

data class RecoveryScreenState(
    val phase: Phase = Phase.LOADING,
    val recovery: RecoveryState? = null,
    /** The account the list is of. */
    val account: AccountChoice = AccountChoice(),
    val items: List<DeletedItem> = emptyList(),
    val total: UInt = 0u,
    /** The first page of the account's items is on its way. */
    val listing: Boolean = false,
    val loadingMore: Boolean = false,
    /** The key may read the accounts, so another account can be picked; otherwise only the own one is offered. */
    val canPick: Boolean = false,
    /** A restore is on its way; another is refused meanwhile. */
    val busy: Boolean = false,
    val dialog: RecoveryDialog? = null,
    val notice: UiMessage? = null,
    val error: UiMessage? = null,
) {
    val hasMore: Boolean get() = items.size.toLong() < total.toLong()
}

/**
 * Recovery of deleted items: first whether the server keeps them, then the list of the chosen account, by kind, with
 * Restore behind a confirmation. Answers that a newer request overtook are dropped; one restore at a time.
 */
class RecoveryController(
    private val scope: CoroutineScope,
    private val io: CoroutineDispatcher,
    private val backend: EnterpriseBackend,
    private val pageSize: UInt = 50u,
    private val searchDebounceMs: Long = 250,
) {
    private val stateFlow = MutableStateFlow(RecoveryScreenState())
    private var started = false
    private var ticket = 0
    private var searchJob: Job? = null
    private var searchTicket = 0

    val state: StateFlow<RecoveryScreenState> get() = stateFlow

    fun start() {
        if (started) return
        started = true
        load()
    }

    fun reset() {
        ticket++
        started = false
        stateFlow.value = RecoveryScreenState()
    }

    /** Reads whether recovery is on and, if so, the list of the chosen account (Retry, pull down). */
    fun load() {
        val mine = ++ticket
        // A new first read ends any page read in flight: its answer is dropped, so it must not stay "loading".
        stateFlow.update { it.copy(phase = if (it.recovery == null) Phase.LOADING else it.phase, loadingMore = false, error = null) }
        scope.launch {
            try {
                val recovery = withContext(io) { backend.recoveryState() }
                if (mine != ticket) return@launch
                if (!recovery.isOn) {
                    stateFlow.update { it.copy(phase = Phase.READY, recovery = recovery, items = emptyList(), total = 0u, listing = false) }
                    return@launch
                }
                val canPick = try {
                    withContext(io) { backend.mayReadAccounts() }
                } catch (e: CancellationException) {
                    throw e
                } catch (e: Exception) {
                    false
                }
                if (mine != ticket) return@launch
                stateFlow.update { it.copy(phase = Phase.READY, recovery = recovery, canPick = canPick, listing = true) }
                readList(mine)
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                if (mine != ticket) return@launch
                val message = failure(e, R.string.ent_recovery_failed)
                stateFlow.update { it.copy(phase = if (it.recovery == null) Phase.FAILED else Phase.READY, listing = false, error = message) }
            }
        }
    }

    private suspend fun readList(mine: Int) {
        val account = stateFlow.value.account.accountId
        try {
            val page = withContext(io) { backend.deletedItems(account, 0u, pageSize) }
            if (mine == ticket) stateFlow.update { it.copy(items = page.items, total = page.total, listing = false) }
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            if (mine == ticket) stateFlow.update { it.copy(items = emptyList(), total = 0u, listing = false, error = failure(e, R.string.ent_recovery_failed)) }
        }
    }

    /** Reads the list of the account again, without asking for the state of recovery. */
    private fun relist() {
        val mine = ++ticket
        stateFlow.update { it.copy(listing = true, loadingMore = false, error = null) }
        scope.launch { readList(mine) }
    }

    fun loadMore() {
        val s = stateFlow.value
        if (s.recovery?.isOn != true || s.listing || s.loadingMore || !s.hasMore) return
        val mine = ++ticket
        val offset = s.items.size.toUInt()
        val account = s.account.accountId
        stateFlow.update { it.copy(loadingMore = true) }
        scope.launch {
            try {
                val page = withContext(io) { backend.deletedItems(account, offset, pageSize) }
                if (mine == ticket) {
                    stateFlow.update { cur ->
                        val known = cur.items.map { it.id }.toSet()
                        cur.copy(items = cur.items + page.items.filter { it.id !in known }, total = page.total, loadingMore = false)
                    }
                }
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                if (mine == ticket) stateFlow.update { it.copy(loadingMore = false, error = failure(e, R.string.ent_recovery_failed)) }
            }
        }
    }

    // ---- the account chooser ----

    fun askAccount() = stateFlow.update {
        if (it.recovery?.isOn == true && it.canPick && !it.busy) it.copy(dialog = RecoveryDialog.Account(it.account)) else it
    }

    fun setAccountChoice(choice: AccountChoice) = stateFlow.update {
        val d = it.dialog
        if (d is RecoveryDialog.Account) it.copy(dialog = d.copy(choice = choice, error = null)) else it
    }

    /** Searches the server's accounts for what was typed (after a short pause); the matches are listed in the dialog. */
    fun searchAccounts(text: String) {
        stateFlow.update { s -> (s.dialog as? RecoveryDialog.Account)?.let { s.copy(dialog = it.copy(query = text, searching = text.isNotBlank() || it.matches.isEmpty())) } ?: s }
        searchJob?.cancel()
        val mine = ++searchTicket
        searchJob = scope.launch {
            delay(searchDebounceMs)
            try {
                val found = withContext(io) { backend.findAccounts(text.trim()) }
                if (mine == searchTicket) stateFlow.update { s -> (s.dialog as? RecoveryDialog.Account)?.let { s.copy(dialog = it.copy(matches = found, searching = false)) } ?: s }
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                if (mine == searchTicket) stateFlow.update { s -> (s.dialog as? RecoveryDialog.Account)?.let { s.copy(dialog = it.copy(matches = emptyList(), searching = false, error = failure(e, R.string.ent_account_search_failed))) } ?: s }
            }
        }
    }

    /** Takes one of the matches as the account to look in. */
    fun pickAccount(match: AccountMatch) = setAccountChoice(AccountChoice(true, match.id, match.label))

    /** Takes the choice of the dialog; another account needs its id. */
    fun applyAccount() {
        val d = stateFlow.value.dialog as? RecoveryDialog.Account ?: return
        if (!d.choice.isValid) {
            stateFlow.update { it.copy(dialog = d.copy(error = UiMessage.Res(R.string.ent_account_id_needed))) }
            return
        }
        val choice = d.choice.copy(id = d.choice.id.trim())
        stateFlow.update { it.copy(account = choice, dialog = null, items = emptyList(), total = 0u) }
        relist()
    }

    // ---- restoring ----

    fun askRestore(id: String) {
        val s = stateFlow.value
        if (s.busy) return
        val item = s.items.firstOrNull { it.id == id } ?: return
        stateFlow.update { it.copy(dialog = RecoveryDialog.Restore(item)) }
    }

    fun dismissDialog() {
        searchJob?.cancel()
        searchTicket++
        stateFlow.update { it.copy(dialog = null) }
    }

    /** Puts the item of the open question back. The item leaves the list once the server did it. */
    fun confirmRestore() {
        val s = stateFlow.value
        val d = s.dialog as? RecoveryDialog.Restore ?: return
        if (s.busy) return
        val account = s.account.accountId
        stateFlow.update { it.copy(busy = true, dialog = null, error = null, notice = null) }
        scope.launch {
            try {
                withContext(io) { backend.restore(account, listOf(d.item.id)) }
                // Only when the same list is still on screen (the account was not changed meanwhile).
                stateFlow.update {
                    if (it.account.accountId == account) {
                        it.copy(busy = false, items = it.items.filter { i -> i.id != d.item.id }, total = if (it.total > 0u) it.total - 1u else 0u, notice = UiMessage.Res(R.string.ent_restored))
                    } else {
                        it.copy(busy = false, notice = UiMessage.Res(R.string.ent_restored))
                    }
                }
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                stateFlow.update { it.copy(busy = false, error = failure(e, R.string.ent_restore_failed)) }
            }
        }
    }

    fun dismissNotice() = stateFlow.update { it.copy(notice = null) }

    fun dismissError() = stateFlow.update { it.copy(error = null) }
}
