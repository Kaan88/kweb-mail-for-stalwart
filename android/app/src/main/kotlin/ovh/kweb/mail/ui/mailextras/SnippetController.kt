package ovh.kweb.mail.ui.mailextras

import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

/** Where the words of one search were found: by message id, for the rows that have a hit in the subject or the text. */
data class SnippetState(val query: String = "", val byMessage: Map<String, Highlighted> = emptyMap())

/** One message's marked subject and text; each is null where the hit is not. */
data class Highlighted(val subject: Highlight?, val preview: Highlight?)

/**
 * Asks the core where a search found its hits, after the list is on screen. It never blocks or changes the
 * list: a slow or failed call just leaves the rows as they are, and the answer to an older query is dropped.
 */
class SnippetController(
    private val scope: CoroutineScope,
    private val io: CoroutineDispatcher,
    private val backend: MailExtrasBackend,
) {
    private val stateFlow = MutableStateFlow(SnippetState())
    private var job: Job? = null

    val state: StateFlow<SnippetState> get() = stateFlow

    /** Starts the question for [query] and [messageIds]; the earlier one, if still running, is given up. */
    fun request(query: String, messageIds: List<String>) {
        job?.cancel()
        stateFlow.value = SnippetState(query)
        val ids = messageIds.distinct()
        if (ids.isEmpty()) return
        job = scope.launch {
            val snippets = try {
                withContext(io) { backend.searchSnippets(query, ids) }
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                return@launch
            }
            val found = HashMap<String, Highlighted>()
            for (snippet in snippets) {
                if (snippet.messageId !in ids) continue
                val subject = Highlight.of(snippet.subject)
                val preview = Highlight.of(snippet.preview)
                if (subject != null || preview != null) found[snippet.messageId] = Highlighted(subject, preview)
            }
            // withContext saw a cancellation if a newer request replaced this one; this checks the query too.
            stateFlow.update { if (it.query == query) it.copy(byMessage = found) else it }
        }
    }

    fun clear() {
        job?.cancel()
        job = null
        stateFlow.value = SnippetState()
    }
}
