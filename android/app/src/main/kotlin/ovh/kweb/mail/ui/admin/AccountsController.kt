package ovh.kweb.mail.ui.admin

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
import uniffi.kweb_core.ServerAccount
import uniffi.kweb_core.ServerDomain

data class AccountsState(
    /** What the search field holds. */
    val text: String = "",
    /** How many accounts the server has, as last seen without a search: for "Search 42 accounts". */
    val knownTotal: Int? = null,
)

/** Server accounts: the list in pages, with the server searching as the user types (after a short pause). */
class AccountsController(
    private val scope: CoroutineScope,
    io: CoroutineDispatcher,
    private val backend: AdminBackend,
    private val debounceMs: Long = 300,
) {
    private val stateFlow = MutableStateFlow(AccountsState())
    private var searchJob: Job? = null

    /**
     * A page of a list is read for the text of the first page on screen (the loader's tag), whatever is typed now; a
     * first page that failed to arrive changes nothing about the list that is still shown.
     */
    val list = PagedLoader<ServerAccount>(scope, io, AdminRules.PAGE, R.string.admin_accounts_load_failed, { it.id }) { offset, limit, tag ->
        val text = if (offset == 0u) stateFlow.value.text.trim() else tag as? String ?: ""
        backend.accounts(text, offset, limit).let { Page(it.accounts, it.total.toInt(), text) }
    }
    val state: StateFlow<AccountsState> get() = stateFlow

    init {
        // Without a search the total of the list is the number of accounts.
        scope.launch {
            list.state.collect { s ->
                val total = s.total
                if (s.phase == ListPhase.READY && s.tag == "" && total != null) stateFlow.update { it.copy(knownTotal = total) }
            }
        }
    }

    fun refresh() = list.refresh()

    fun setText(text: String) {
        if (text == stateFlow.value.text) return
        stateFlow.update { it.copy(text = text) }
        searchJob?.cancel()
        searchJob = scope.launch {
            delay(debounceMs)
            list.reload(quiet = true)
        }
    }
}

enum class AccountPhase { LOADING, READY, FAILED }

sealed interface AccountDialog {
    /** [openedWith] is the limit text the dialog opened with: a limit the user did not touch is sent back as it was. */
    data class Edit(val description: String, val limit: String, val openedWith: String = limit) : AccountDialog {
        val limitInvalid: Boolean get() = limit != openedWith && AdminRules.parseQuota(limit) == QuotaInput.Invalid
    }

    /** The two password fields live here while the dialog is open, in memory only, and are gone when it closes. */
    data class Reset(val password: String = "", val repeat: String = "") : AccountDialog {
        val problem: PasswordProblem? get() = AdminRules.passwordProblem(password, repeat)
    }

    data object Suspend : AccountDialog

    data class Delete(val typed: String = "") : AccountDialog
}

data class AccountState(
    val phase: AccountPhase = AccountPhase.LOADING,
    val account: ServerAccount? = null,
    val dialog: AccountDialog? = null,
    val busy: Boolean = false,
    /** The account is deleted: the screen closes. */
    val gone: Boolean = false,
    val error: UiMessage? = null,
    val info: UiMessage? = null,
)

/** One server account: what it holds, and what an administrator does to it (each risky change asks first). */
class AccountController(
    private val scope: CoroutineScope,
    private val io: CoroutineDispatcher,
    private val backend: AdminBackend,
    private val id: String,
) {
    private val gate = ActionGate(scope, io)
    private val stateFlow = MutableStateFlow(AccountState())
    private var ticket = 0

    val state: StateFlow<AccountState> get() = stateFlow

    fun refresh() {
        val mine = ++ticket
        scope.launch {
            try {
                val account = withContext(io) { backend.account(id) }
                if (mine != ticket) return@launch
                stateFlow.update { it.copy(phase = AccountPhase.READY, account = account, error = null) }
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                if (mine != ticket) return@launch
                stateFlow.update {
                    if (it.account != null) it.copy(error = UiMessage.of(e, R.string.admin_load_failed)) else it.copy(phase = AccountPhase.FAILED, error = UiMessage.of(e, R.string.admin_load_failed))
                }
            }
        }
    }

    // ---- edit ----

    fun openEdit() {
        val account = stateFlow.value.account ?: return
        stateFlow.update { it.copy(dialog = AccountDialog.Edit(account.description, AdminRules.quotaText(account.quota)), error = null, info = null) }
    }

    fun typeDescription(text: String) = stateFlow.update { s -> (s.dialog as? AccountDialog.Edit)?.let { s.copy(dialog = it.copy(description = text)) } ?: s }

    fun typeLimit(text: String) = stateFlow.update { s -> (s.dialog as? AccountDialog.Edit)?.let { s.copy(dialog = it.copy(limit = text)) } ?: s }

    fun saveEdit() {
        val edit = stateFlow.value.dialog as? AccountDialog.Edit ?: return
        val quota: ULong? = if (edit.limit == edit.openedWith) {
            // Not edited: the text is a rounded way of showing the limit, so the limit itself goes back unchanged.
            stateFlow.value.account?.quota
        } else {
            when (val q = AdminRules.parseQuota(edit.limit)) {
                QuotaInput.Invalid -> return
                QuotaInput.NoLimit -> null
                is QuotaInput.Limit -> q.bytes.toULong()
            }
        }
        gate.run(
            { backend.updateAccount(id, edit.description.trim(), quota) },
            ::started,
            { fail(it, R.string.admin_account_save_failed) },
        ) { account -> stateFlow.update { it.copy(busy = false, dialog = null, account = account, info = UiMessage.Res(R.string.admin_account_saved)) } }
    }

    // ---- password ----

    fun openReset() = stateFlow.update { it.copy(dialog = AccountDialog.Reset(), error = null, info = null) }

    fun typePassword(text: String) = stateFlow.update { s -> (s.dialog as? AccountDialog.Reset)?.let { s.copy(dialog = it.copy(password = text)) } ?: s }

    fun typeRepeat(text: String) = stateFlow.update { s -> (s.dialog as? AccountDialog.Reset)?.let { s.copy(dialog = it.copy(repeat = text)) } ?: s }

    fun confirmReset() {
        val reset = stateFlow.value.dialog as? AccountDialog.Reset ?: return
        if (reset.problem != null) return
        val password = reset.password
        // The dialog stays open (with its fields) while the server answers, so a failure leaves the input; it closes on success.
        gate.run({ backend.resetPassword(id, password) }, ::started, { fail(it, R.string.admin_password_failed) }) {
            stateFlow.update { it.copy(busy = false, dialog = null, info = UiMessage.Res(R.string.admin_password_done)) }
        }
    }

    // ---- suspend ----

    /** Suspending asks; lifting a suspension does not (it only gives back what was there). */
    fun toggleSuspend() {
        val account = stateFlow.value.account ?: return
        if (account.isSuspended) setSuspended(false) else stateFlow.update { it.copy(dialog = AccountDialog.Suspend, error = null, info = null) }
    }

    fun confirmSuspend() {
        stateFlow.update { it.copy(dialog = null) }
        setSuspended(true)
    }

    private fun setSuspended(suspended: Boolean) {
        gate.run({ backend.suspendAccount(id, suspended) }, ::started, { fail(it, R.string.admin_suspend_failed) }) {
            stateFlow.update { s ->
                s.copy(busy = false, account = s.account?.copy(isSuspended = suspended), info = UiMessage.Res(if (suspended) R.string.admin_suspended_done else R.string.admin_lifted_done))
            }
            refresh()
        }
    }

    // ---- delete ----

    fun askDelete() = stateFlow.update { it.copy(dialog = AccountDialog.Delete(), error = null, info = null) }

    fun typeDelete(text: String) = stateFlow.update { s -> (s.dialog as? AccountDialog.Delete)?.let { s.copy(dialog = it.copy(typed = text)) } ?: s }

    fun confirmDelete() {
        val delete = stateFlow.value.dialog as? AccountDialog.Delete ?: return
        val account = stateFlow.value.account ?: return
        if (!AdminRules.deleteConfirmed(delete.typed, account.name)) return
        stateFlow.update { it.copy(dialog = null) }
        gate.run({ backend.deleteAccount(id) }, ::started, { fail(it, R.string.admin_delete_failed) }) {
            stateFlow.update { it.copy(busy = false, gone = true) }
        }
    }

    private fun started() = stateFlow.update { it.copy(busy = true, error = null, info = null) }

    private fun fail(e: Exception, fallback: Int) = stateFlow.update { it.copy(busy = false, error = UiMessage.of(e, fallback)) }

    /** Closes a dialog; a password typed in it is let go. */
    fun dismissDialog() = stateFlow.update { if (it.busy) it else it.copy(dialog = null) }

    fun dismissError() = stateFlow.update { it.copy(error = null) }

    fun dismissInfo() = stateFlow.update { it.copy(info = null) }

    /** The screen is gone. */
    fun clear() = stateFlow.update { it.copy(dialog = null) }
}

enum class NewAccountPhase { LOADING, READY, FAILED }

data class NewAccountState(
    val phase: NewAccountPhase = NewAccountPhase.LOADING,
    val domains: List<ServerDomain> = emptyList(),
    val domainId: String = "",
    val name: String = "",
    val description: String = "",
    val password: String = "",
    val repeat: String = "",
    val busy: Boolean = false,
    /** Set once it was made; the screen closes. */
    val created: ServerAccount? = null,
    val error: UiMessage? = null,
) {
    val nameProblem: NameProblem? get() = AdminRules.nameProblem(name)
    val passwordProblem: PasswordProblem? get() = AdminRules.passwordProblem(password, repeat)
    val ready: Boolean get() = phase == NewAccountPhase.READY && domainId.isNotEmpty() && nameProblem == null && passwordProblem == null && !busy
}

/** The form for a new account. The password is in the state only until the account is made or the screen is left. */
class NewAccountController(
    private val scope: CoroutineScope,
    private val io: CoroutineDispatcher,
    private val backend: AdminBackend,
) {
    private val stateFlow = MutableStateFlow(NewAccountState())
    private val gate = ActionGate(scope, io)
    private var ticket = 0

    val state: StateFlow<NewAccountState> get() = stateFlow

    /** Reads the domains once (and again after a failure). */
    fun load() {
        if (stateFlow.value.phase == NewAccountPhase.READY) return
        val mine = ++ticket
        stateFlow.update { it.copy(phase = NewAccountPhase.LOADING) }
        scope.launch {
            try {
                val domains = withContext(io) { backend.domains() }
                if (mine != ticket) return@launch
                stateFlow.update { it.copy(phase = NewAccountPhase.READY, domains = domains, domainId = it.domainId.ifEmpty { domains.firstOrNull()?.id.orEmpty() }, error = null) }
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                if (mine != ticket) return@launch
                stateFlow.update { it.copy(phase = NewAccountPhase.FAILED, error = UiMessage.of(e, R.string.admin_domains_failed)) }
            }
        }
    }

    fun typeName(text: String) = stateFlow.update { it.copy(name = text.trim(), error = null) }

    fun pickDomain(id: String) = stateFlow.update { it.copy(domainId = id) }

    fun typeDescription(text: String) = stateFlow.update { it.copy(description = text) }

    fun typePassword(text: String) = stateFlow.update { it.copy(password = text, error = null) }

    fun typeRepeat(text: String) = stateFlow.update { it.copy(repeat = text, error = null) }

    fun create() {
        val s = stateFlow.value
        if (!s.ready) return
        gate.run(
            { backend.createAccount(s.name, s.domainId, s.description.trim(), s.password) },
            { stateFlow.update { it.copy(busy = true, error = null) } },
            { e -> stateFlow.update { it.copy(busy = false, error = UiMessage.of(e, R.string.admin_create_failed)) } },
        ) { account -> stateFlow.update { it.copy(busy = false, created = account, password = "", repeat = "") } }
    }

    fun dismissError() = stateFlow.update { it.copy(error = null) }

    /** The screen is gone: the password goes with it. */
    fun clear() = stateFlow.update { it.copy(password = "", repeat = "") }
}
