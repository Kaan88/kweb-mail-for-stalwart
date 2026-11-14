package ovh.kweb.mail.ui.mailextras

import java.util.concurrent.CopyOnWriteArrayList
import uniffi.kweb_core.Label
import uniffi.kweb_core.MessageLabels
import uniffi.kweb_core.SearchResult
import uniffi.kweb_core.Snippet
import uniffi.kweb_core.Unsubscribe

/** A core for the tests: answers what it is told to, and can be held back or made to fail. */
class FakeExtras : MailExtrasBackend {
    val calls = CopyOnWriteArrayList<String>()
    var infos: MutableMap<String, Unsubscribe?> = HashMap()
    var infoFails: Exception? = null
    var unsubscribeFails: Exception? = null
    var unsubscribeGate: (() -> Unit)? = null
    var labelList: List<Label> = emptyList()
    var labelsFail: Exception? = null
    val keywords: MutableMap<String, List<String>> = HashMap()
    /** Runs after the answer is decided and before it is returned, to hold an answer back. */
    var messageLabelsGate: ((List<String>) -> Unit)? = null
    var messageLabelsFails: Exception? = null
    var setLabelFails: Exception? = null
    /** Runs when a change reaches the core (with its label), before it is applied, to hold it back. */
    var setLabelGate: ((String) -> Unit)? = null
    var createFails: Exception? = null
    var deleteFails: Exception? = null
    var snippets: List<Snippet> = emptyList()
    var snippetsGate: ((String) -> Unit)? = null
    var snippetsFail: Exception? = null
    /** When true, [messageLabels] leaves out messages without labels, as the core may. */
    var sparse = true

    override fun unsubscribeInfo(messageId: String): Unsubscribe? {
        calls += "info:$messageId"
        infoFails?.let { throw it }
        return infos[messageId]
    }

    override fun unsubscribe(messageId: String) {
        calls += "unsubscribe:$messageId"
        unsubscribeGate?.invoke()
        unsubscribeFails?.let { throw it }
    }

    override fun labels(): List<Label> {
        calls += "labels"
        labelsFail?.let { throw it }
        return labelList
    }

    override fun createLabel(name: String): Label {
        calls += "create:$name"
        createFails?.let { throw it }
        val label = Label(name.lowercase(), name)
        labelList = labelList + label
        return label
    }

    override fun deleteLabel(labelId: String) {
        calls += "delete:$labelId"
        deleteFails?.let { throw it }
        labelList = labelList.filter { it.id != labelId }
    }

    override fun messageLabels(messageIds: List<String>): List<MessageLabels> {
        calls += "messageLabels:${messageIds.joinToString(",")}"
        messageLabelsFails?.let { throw it }
        val answer = messageIds.filter { !sparse || keywords[it].orEmpty().isNotEmpty() }.map { MessageLabels(it, keywords[it].orEmpty()) }
        messageLabelsGate?.invoke(messageIds)
        return answer
    }

    override fun setLabel(messageIds: List<String>, labelId: String, on: Boolean) {
        calls += "set:${messageIds.joinToString(",")}:$labelId:$on"
        setLabelGate?.invoke(labelId)
        setLabelFails?.let { throw it }
        for (id in messageIds) {
            val now = keywords[id].orEmpty()
            keywords[id] = if (on) (now + labelId).distinct() else now - labelId
        }
    }

    override fun labelMessages(labelId: String, limit: UInt): SearchResult = throw UnsupportedOperationException()

    override fun searchSnippets(query: String, messageIds: List<String>): List<Snippet> {
        calls += "snippets:$query:${messageIds.joinToString(",")}"
        snippetsFail?.let { throw it }
        snippetsGate?.invoke(query)
        return snippets
    }
}

/** Waits (polling) until [check] holds. */
fun waitFor(what: String, check: () -> Boolean) {
    val end = System.currentTimeMillis() + 5000
    while (!check()) {
        if (System.currentTimeMillis() > end) throw AssertionError("timed out waiting for $what")
        Thread.sleep(5)
    }
}
