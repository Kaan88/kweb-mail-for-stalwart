package ovh.kweb.mail.ui.sending

import uniffi.kweb_core.Core
import uniffi.kweb_core.Draft
import uniffi.kweb_core.ScheduledSend

/** What the list of held messages asks of the core. Every call blocks: run it off the main thread. The tests use a fake. */
interface ScheduledBackend {
    fun scheduledSends(): List<ScheduledSend>
    fun cancelScheduled(id: String): Draft
}

class CoreScheduledBackend(private val core: () -> Core) : ScheduledBackend {
    override fun scheduledSends() = core().scheduledSends()
    override fun cancelScheduled(id: String) = core().cancelScheduled(id)
}
