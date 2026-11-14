package ovh.kweb.mail.ui.sending

import uniffi.kweb_core.Core
import uniffi.kweb_core.IdentityDetail

/** What the senders screen asks of the core. Every call blocks: run it off the main thread. The tests use a fake. */
interface SendersBackend {
    fun identityDetails(): List<IdentityDetail>
    fun saveIdentity(id: String?, name: String, email: String, replyTo: String, signature: String): IdentityDetail
    fun deleteIdentity(id: String)
    fun setDefaultIdentity(id: String)
}

class CoreSendersBackend(private val core: () -> Core) : SendersBackend {
    override fun identityDetails() = core().identityDetails()
    override fun saveIdentity(id: String?, name: String, email: String, replyTo: String, signature: String) =
        core().saveIdentity(id, name, email, replyTo, signature)

    override fun deleteIdentity(id: String) = core().deleteIdentity(id)
    override fun setDefaultIdentity(id: String) = core().setDefaultIdentity(id)
}
