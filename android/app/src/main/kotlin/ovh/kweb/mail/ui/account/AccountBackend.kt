package ovh.kweb.mail.ui.account

import uniffi.kweb_core.AccountFeatures
import uniffi.kweb_core.AccountSecurity
import uniffi.kweb_core.Core
import uniffi.kweb_core.MaskedAddress
import uniffi.kweb_core.MaskedAddresses
import uniffi.kweb_core.NewSecret
import uniffi.kweb_core.TwoStepSetup

/** What the masked-address and security screens ask of the core. Every call blocks: run it off the main thread. The tests use a fake. */
interface AccountBackend {
    fun accountFeatures(): AccountFeatures
    fun maskedAddresses(): MaskedAddresses
    fun createMaskedAddress(description: String, forDomain: String, prefix: String): MaskedAddress
    fun updateMaskedAddress(id: String, enabled: Boolean, description: String?)
    fun deleteMaskedAddress(id: String)
    fun accountSecurity(): AccountSecurity
    fun changePassword(current: String, new: String, code: String?)
    fun newTwoStep(): TwoStepSetup
    fun enableTwoStep(currentPassword: String, otpauthUrl: String, code: String, oldCode: String?)
    fun disableTwoStep(currentPassword: String, code: String?)
    fun createAppPassword(description: String, expiresAt: Long?): NewSecret
    fun deleteAppPassword(id: String)
    fun createApiKey(description: String, expiresAt: Long?): NewSecret
    fun deleteApiKey(id: String)
    fun renamePasskey(id: String, name: String)
    fun deletePasskey(id: String)
    fun revokeConnectedApp(id: String)
}

class CoreAccountBackend(private val core: () -> Core) : AccountBackend {
    override fun accountFeatures() = core().accountFeatures()
    override fun maskedAddresses() = core().maskedAddresses()
    override fun createMaskedAddress(description: String, forDomain: String, prefix: String) =
        core().createMaskedAddress(description, forDomain, prefix)
    override fun updateMaskedAddress(id: String, enabled: Boolean, description: String?) = core().updateMaskedAddress(id, enabled, description)
    override fun deleteMaskedAddress(id: String) = core().deleteMaskedAddress(id)
    override fun accountSecurity() = core().accountSecurity()
    override fun changePassword(current: String, new: String, code: String?) = core().changePasswordWithCode(current, new, code)
    override fun newTwoStep() = core().newTwoStep()
    override fun enableTwoStep(currentPassword: String, otpauthUrl: String, code: String, oldCode: String?) =
        core().enableTwoStepWithCode(currentPassword, otpauthUrl, code, oldCode)
    override fun disableTwoStep(currentPassword: String, code: String?) = core().disableTwoStepWithCode(currentPassword, code)
    override fun createAppPassword(description: String, expiresAt: Long?) = core().createAppPassword(description, expiresAt)
    override fun deleteAppPassword(id: String) = core().deleteAppPassword(id)
    override fun createApiKey(description: String, expiresAt: Long?) = core().createApiKey(description, expiresAt)
    override fun deleteApiKey(id: String) = core().deleteApiKey(id)
    override fun renamePasskey(id: String, name: String) = core().renamePasskey(id, name)
    override fun deletePasskey(id: String) = core().deletePasskey(id)
    override fun revokeConnectedApp(id: String) = core().revokeConnectedApp(id)
}
