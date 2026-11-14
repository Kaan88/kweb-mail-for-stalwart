package ovh.kweb.mail.ui

import android.content.Context
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import ovh.kweb.mail.background.Background
import ovh.kweb.mail.background.CheckScheduler
import uniffi.kweb_core.Action
import uniffi.kweb_core.ConnectionStatus

/** What a conversation menu or swipe does to a list of messages, shared by the list and the thread. */
object MailActions {
    fun markRead(ids: List<String>) = ids.map { Action.MarkRead(it) }

    fun markUnread(ids: List<String>) = ids.map { Action.MarkUnread(it) }

    fun setFlagged(ids: List<String>, flagged: Boolean) =
        ids.map { if (flagged) Action.Flag(it) else Action.Unflag(it) }

    fun archive(ids: List<String>) = ids.map { Action.Archive(it) }

    /** In Trash the core turns this into a permanent delete. */
    fun delete(ids: List<String>) = ids.map { Action.Delete(it) }

    fun move(ids: List<String>, mailboxId: String) = ids.map { Action.Move(it, mailboxId) }

    /**
     * Hands [actions] to the core with one `submitAll`. The core applies them one by one and stops
     * at the first error, so after a failure the earlier ones stay applied: callers reload. When
     * the live connection is down a check is scheduled, so they reach the server as soon as there
     * is a network. Throws what the core throws.
     */
    suspend fun submit(context: Context, actions: List<Action>) {
        if (actions.isEmpty()) return
        withContext(Dispatchers.IO) {
            val core = Background.core(context)
            core.submitAll(actions)
            if (core.status() !is ConnectionStatus.Live) CheckScheduler.checkSoon(context)
        }
    }
}
