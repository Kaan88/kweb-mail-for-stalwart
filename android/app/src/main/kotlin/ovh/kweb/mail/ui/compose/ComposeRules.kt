package ovh.kweb.mail.ui.compose

import uniffi.kweb_core.Address
import uniffi.kweb_core.Draft

/** The rules of the compose screen that need no Android. */
object ComposeRules {
    /** How long after the last change the draft is saved. */
    const val AUTOSAVE_MILLIS = 2_000L

    /** How long a sent message is held back so Undo can take it back. */
    const val UNDO_SECONDS = 10u

    /** The wait before the text in a recipient field is looked up. */
    const val SUGGEST_MILLIS = 300L

    /** What the draft is: sets the title of the screen. */
    enum class Kind { New, Reply, Forward }

    fun kind(draft: Draft): Kind = when {
        draft.sourceMessageId == null -> Kind.New
        draft.isForward -> Kind.Forward
        else -> Kind.Reply
    }

    /** What the user can have changed: the rest of a draft is the core's bookkeeping. */
    fun sameContent(a: Draft, b: Draft): Boolean = a.identityId == b.identityId && a.to == b.to && a.cc == b.cc &&
        a.bcc == b.bcc && a.subject == b.subject && a.body == b.body && a.attachments == b.attachments

    /**
     * Leaving throws the draft away instead of saving it: it was a blank new message when the
     * screen opened (no recipients, subject or attachments, not an answer, never on the server;
     * the text may be a signature) and nothing has been changed since. A [prefill] recipient the screen
     * added itself is not a change: the message is still untouched when it is the only addition.
     */
    fun discardsOnLeave(opened: Draft, now: Draft, prefill: Address? = null): Boolean =
        isBlankNew(opened) && (sameContent(opened, now) || (prefill != null && sameContent(opened.copy(to = listOf(prefill)), now)))

    /** A draft nobody has written in yet. */
    fun isBlankNew(draft: Draft): Boolean = draft.sourceMessageId == null && draft.serverId == null &&
        draft.sendError == null && draft.to.isEmpty() && draft.cc.isEmpty() && draft.bcc.isEmpty() &&
        draft.subject.isBlank() && draft.attachments.isEmpty()
}
