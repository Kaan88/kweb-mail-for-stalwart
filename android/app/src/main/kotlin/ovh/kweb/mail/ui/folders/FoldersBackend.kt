package ovh.kweb.mail.ui.folders

import uniffi.kweb_core.ConnectionStatus
import uniffi.kweb_core.Core
import uniffi.kweb_core.Mailbox

/** What the folder screens ask of the core. Every call blocks: run it off the main thread. The tests use a fake. */
interface FoldersBackend {
    fun mailboxes(): List<Mailbox>
    fun setNotify(mailboxId: String, notify: Boolean)
    fun create(name: String, parentId: String?): Mailbox
    fun rename(mailboxId: String, name: String)
    fun move(mailboxId: String, parentId: String?)
    fun delete(mailboxId: String, withMail: Boolean)
}

class CoreFoldersBackend(private val core: () -> Core, private val checkSoon: () -> Unit = {}) : FoldersBackend {
    override fun mailboxes() = core().mailboxes()
    override fun setNotify(mailboxId: String, notify: Boolean) {
        val c = core()
        c.setMailboxNotify(mailboxId, notify)
        // The core pokes the watcher; without a live connection a check is what applies the change on screen.
        if (c.status() !is ConnectionStatus.Live) checkSoon()
    }
    override fun create(name: String, parentId: String?) = core().createMailbox(name, parentId)
    override fun rename(mailboxId: String, name: String) = core().renameMailbox(mailboxId, name)
    override fun move(mailboxId: String, parentId: String?) = core().moveMailbox(mailboxId, parentId)
    override fun delete(mailboxId: String, withMail: Boolean) = core().deleteMailbox(mailboxId, withMail)
}
