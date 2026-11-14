package ovh.kweb.mail.ui.account

import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import ovh.kweb.mail.R
import ovh.kweb.mail.ui.UiMessage
import uniffi.kweb_core.AccountSecurity
import uniffi.kweb_core.ConnectedApp
import uniffi.kweb_core.Credential
import uniffi.kweb_core.NewSecret
import uniffi.kweb_core.Passkey
import uniffi.kweb_core.TwoStepSetup

enum class SecurityPhase { LOADING, READY, FAILED, NONE }

/**
 * A question or form on top of the screen. The secrets it holds ([TwoStep], [Secret]) are shown to the user and
 * go the moment the dialog closes; a typed password or code is never here (the form keeps it, see SecurityDialogs).
 */
sealed interface SecurityDialog {
    data object ChangePassword : SecurityDialog

    data class TwoStep(val setup: TwoStepSetup) : SecurityDialog

    data object TwoStepOff : SecurityDialog

    data class NewCredential(val kind: CredentialKind) : SecurityDialog

    /** A new app password or key, shown once. */
    data class Secret(val kind: CredentialKind, val secret: NewSecret) : SecurityDialog

    data class DeleteCredential(val kind: CredentialKind, val credential: Credential) : SecurityDialog

    data class RenamePasskey(val passkey: Passkey) : SecurityDialog

    data class DeletePasskey(val passkey: Passkey, val last: Boolean) : SecurityDialog

    data class Revoke(val app: ConnectedApp) : SecurityDialog

    /** A password, a code or a secret is on screen: the window is kept out of screenshots. */
    val showsSecret: Boolean
        get() = this is ChangePassword || this is TwoStep || this is TwoStepOff || this is Secret
}

data class SecurityState(
    val phase: SecurityPhase = SecurityPhase.LOADING,
    val security: AccountSecurity? = null,
    val unsupported: UiMessage? = null,
    /** A change is on its way: the buttons wait and the dialog cannot be closed. */
    val busy: Boolean = false,
    val error: UiMessage? = null,
    /** Something that is not a failure: what happened. */
    val info: UiMessage? = null,
    val dialog: SecurityDialog? = null,
    /** The core's refusal of the open dialog's form (a wrong password, a wrong code). */
    val dialogError: UiMessage? = null,
)

/**
 * The state and actions of the account security screen. The core is asked off the main thread ([io]); a read that
 * a newer one has overtaken is dropped; one change at a time. A password, code or new secret that is passed in
 * goes to the core and is not stored anywhere here; a secret that comes back is held only while its dialog is open.
 */
class SecurityController(
    private val scope: CoroutineScope,
    private val io: CoroutineDispatcher,
    private val backend: AccountBackend,
    private val clock: () -> Long = { System.currentTimeMillis() / 1000 },
) {
    private val stateFlow = MutableStateFlow(SecurityState())
    private var started = false
    private var ticket = 0
    private var epoch = 0

    val state: StateFlow<SecurityState> get() = stateFlow

    fun start() {
        if (started) return
        started = true
        load()
    }

    fun reset() {
        ticket++
        epoch++
        started = false
        stateFlow.value = SecurityState()
    }

    fun load() {
        scope.launch { readNow() }
    }

    private suspend fun readNow() {
        val mine = ++ticket
        stateFlow.update { if (it.security == null) it.copy(phase = SecurityPhase.LOADING) else it }
        try {
            val security = withContext(io) { backend.accountSecurity() }
            if (mine != ticket) return
            stateFlow.update { it.copy(phase = SecurityPhase.READY, security = security, unsupported = null, error = null) }
        } catch (e: CancellationException) {
            throw e
        } catch (e: uniffi.kweb_core.Exception.Invalid) {
            if (mine != ticket) return
            stateFlow.update {
                if (it.security != null) it.copy(error = UiMessage.of(e, R.string.security_load_failed))
                else it.copy(phase = SecurityPhase.NONE, unsupported = UiMessage.of(e, R.string.security_none))
            }
        } catch (e: Exception) {
            if (mine != ticket) return
            stateFlow.update {
                if (it.security != null) it.copy(error = UiMessage.Res(R.string.security_load_failed)) else it.copy(phase = SecurityPhase.FAILED)
            }
        }
    }

    /** Drops everything that is on screen, a shown secret included, whatever is going on. */
    fun dismissAll() {
        // An answer that is still on its way belongs to a dialog that is gone: it must not bring a secret back.
        epoch++
        stateFlow.update { it.copy(dialog = null, dialogError = null, busy = false) }
    }

    fun dismissError() = stateFlow.update { it.copy(error = null) }

    fun dismissInfo() = stateFlow.update { it.copy(info = null) }

    // ---- dialogs ----

    private fun open(dialog: SecurityDialog) = stateFlow.update { if (it.busy || it.security == null) it else it.copy(dialog = dialog, dialogError = null) }

    /** Closes the dialog (and with it any secret it held); not while a change is on its way. */
    fun dismissDialog() = stateFlow.update { if (it.busy) it else it.copy(dialog = null, dialogError = null) }

    fun askChangePassword() = open(SecurityDialog.ChangePassword)

    fun askTwoStepOff() = open(SecurityDialog.TwoStepOff)

    fun askNewCredential(kind: CredentialKind) = open(SecurityDialog.NewCredential(kind))

    fun askDeleteCredential(kind: CredentialKind, credential: Credential) = open(SecurityDialog.DeleteCredential(kind, credential))

    fun askRenamePasskey(passkey: Passkey) = open(SecurityDialog.RenamePasskey(passkey))

    fun askDeletePasskey(passkey: Passkey) {
        val count = stateFlow.value.security?.passkeys?.size ?: return
        open(SecurityDialog.DeletePasskey(passkey, SecurityRules.isLastPasskey(count)))
    }

    fun askRevoke(app: ConnectedApp) {
        if (SecurityRules.mayRevoke(app)) open(SecurityDialog.Revoke(app))
    }

    // ---- changes ----

    /**
     * With two-step codes on the server wants a current code for each of these changes: [currentCode] is then
     * required (six digits) and goes to the core; with them off it is ignored and none is sent.
     */
    private fun codeToSend(currentCode: String): Pair<Boolean, String?> {
        if (stateFlow.value.security?.twoStep != true) return true to null
        return if (SecurityRules.codeIsComplete(currentCode)) true to SecurityRules.cleanCode(currentCode) else false to null
    }

    fun changePassword(current: String, new: String, currentCode: String = "") {
        val (ok, code) = codeToSend(currentCode)
        if (!ok) return
        change(R.string.security_password_failed, { backend.changePassword(current, new, code) }) { done(info = R.string.security_password_changed) }
    }

    /** Makes the secret on this device (nothing is sent) and shows it. */
    fun startTwoStep() {
        change(R.string.security_two_step_failed, { backend.newTwoStep() }, refresh = false) { setup ->
            stateFlow.update { it.copy(dialog = SecurityDialog.TwoStep(setup)) }
        }
    }

    /** [code] proves the new secret; [oldCode] is the current code of the secret being replaced (needed when two-step codes are on). */
    fun enableTwoStep(currentPassword: String, code: String, oldCode: String = "") {
        val setup = (stateFlow.value.dialog as? SecurityDialog.TwoStep)?.setup ?: return
        val (ok, old) = codeToSend(oldCode)
        if (!ok) return
        val clean = SecurityRules.cleanCode(code)
        change(R.string.security_two_step_failed, { backend.enableTwoStep(currentPassword, setup.otpauthUrl, clean, old) }) {
            done(info = R.string.security_two_step_on)
        }
    }

    fun disableTwoStep(currentPassword: String, currentCode: String = "") {
        val (ok, code) = codeToSend(currentCode)
        if (!ok) return
        change(R.string.security_two_step_failed, { backend.disableTwoStep(currentPassword, code) }) { done(info = R.string.security_two_step_off) }
    }

    fun createCredential(kind: CredentialKind, note: String, expiry: Expiry) {
        val expires = SecurityRules.expiresAt(expiry, clock())
        val text = SecurityRules.cleanNote(note)
        val failed = if (kind == CredentialKind.APP_PASSWORD) R.string.security_app_password_failed else R.string.security_api_key_failed
        change(
            failed,
            { if (kind == CredentialKind.APP_PASSWORD) backend.createAppPassword(text, expires) else backend.createApiKey(text, expires) },
        ) { made ->
            stateFlow.update { it.copy(dialog = SecurityDialog.Secret(kind, made)) }
        }
    }

    fun deleteCredential() {
        val dialog = stateFlow.value.dialog as? SecurityDialog.DeleteCredential ?: return
        val id = dialog.credential.id
        change(R.string.security_delete_failed, {
            if (dialog.kind == CredentialKind.APP_PASSWORD) backend.deleteAppPassword(id) else backend.deleteApiKey(id)
        }) { done() }
    }

    fun renamePasskey(name: String) {
        val dialog = stateFlow.value.dialog as? SecurityDialog.RenamePasskey ?: return
        val text = name.trim()
        if (text.isEmpty()) return
        if (text == dialog.passkey.name) return dismissDialog()
        change(R.string.security_rename_failed, { backend.renamePasskey(dialog.passkey.id, text) }) { done() }
    }

    fun deletePasskey() {
        val dialog = stateFlow.value.dialog as? SecurityDialog.DeletePasskey ?: return
        change(R.string.security_delete_failed, { backend.deletePasskey(dialog.passkey.id) }) { done() }
    }

    fun revoke() {
        val dialog = stateFlow.value.dialog as? SecurityDialog.Revoke ?: return
        if (!SecurityRules.mayRevoke(dialog.app)) return
        change(R.string.security_revoke_failed, { backend.revokeConnectedApp(dialog.app.id) }) { done() }
    }

    /** What a finished change does to the state: closes the dialog, says [info] when given. */
    private fun done(info: Int? = null) {
        stateFlow.update { it.copy(dialog = null, dialogError = null, info = info?.let { id -> UiMessage.Res(id) } ?: it.info) }
    }

    /**
     * One change at a time. On success [onDone] runs with the answer and the list is read again; a refusal goes
     * into the open dialog (which stays, so the user can fix the form) or, without one, onto the screen.
     */
    private fun <T> change(fallback: Int, call: () -> T, refresh: Boolean = true, onDone: (T) -> Unit) {
        if (stateFlow.value.busy) return
        val mine = epoch
        stateFlow.update { it.copy(busy = true, dialogError = null) }
        scope.launch {
            try {
                val answer = withContext(io) { call() }
                if (mine != epoch) return@launch
                stateFlow.update { it.copy(busy = false) }
                onDone(answer)
                if (refresh) readNow()
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                if (mine != epoch) return@launch
                val message = UiMessage.of(e, fallback)
                stateFlow.update { if (it.dialog != null) it.copy(busy = false, dialogError = message) else it.copy(busy = false, error = message) }
            }
        }
    }
}
