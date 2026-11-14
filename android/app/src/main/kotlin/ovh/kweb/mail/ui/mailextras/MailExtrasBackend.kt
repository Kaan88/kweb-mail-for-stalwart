package ovh.kweb.mail.ui.mailextras

import uniffi.kweb_core.Core
import uniffi.kweb_core.Label
import uniffi.kweb_core.MessageLabels
import uniffi.kweb_core.SearchResult
import uniffi.kweb_core.Snippet
import uniffi.kweb_core.Unsubscribe

/** What unsubscribing, labels and search highlights ask of the core. Every call blocks: run it off the main thread. The tests use a fake. */
interface MailExtrasBackend {
    fun unsubscribeInfo(messageId: String): Unsubscribe?
    fun unsubscribe(messageId: String)
    fun labels(): List<Label>
    fun createLabel(name: String): Label
    fun deleteLabel(labelId: String)
    fun messageLabels(messageIds: List<String>): List<MessageLabels>
    fun setLabel(messageIds: List<String>, labelId: String, on: Boolean)
    fun labelMessages(labelId: String, limit: UInt): SearchResult
    fun searchSnippets(query: String, messageIds: List<String>): List<Snippet>
}

class CoreMailExtrasBackend(private val core: () -> Core) : MailExtrasBackend {
    override fun unsubscribeInfo(messageId: String) = core().unsubscribeInfo(messageId)
    override fun unsubscribe(messageId: String) = core().unsubscribe(messageId)
    override fun labels() = core().labels()
    override fun createLabel(name: String) = core().createLabel(name)
    override fun deleteLabel(labelId: String) = core().deleteLabel(labelId)
    override fun messageLabels(messageIds: List<String>) = core().messageLabels(messageIds)
    override fun setLabel(messageIds: List<String>, labelId: String, on: Boolean) = core().setLabel(messageIds, labelId, on)
    override fun labelMessages(labelId: String, limit: UInt) = core().labelMessages(labelId, limit)
    override fun searchSnippets(query: String, messageIds: List<String>) = core().searchSnippets(query, messageIds)
}
