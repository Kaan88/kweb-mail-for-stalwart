package ovh.kweb.mail.ui.sending

import uniffi.kweb_core.IdentityDetail

/** The form of one sender, as typed. [id] null: a sender being added. */
data class SenderForm(
    val id: String?,
    val name: String,
    val email: String,
    val replyTo: String,
    val signature: String,
    /** "Use for new messages". */
    val useForNew: Boolean,
    /** The sender is the one new messages start with already: the switch cannot be turned off. */
    val isDefault: Boolean,
    val mayDelete: Boolean,
) {
    val isNew: Boolean get() = id == null
}

/** The rules of the senders screens that need no Android. */
object SendersRules {
    /** What the server took when probed (name 254, signature 2047). The unit (characters or bytes) is not known: the core's refusal is the last word. */
    const val NAME_LIMIT = 254
    const val SIGNATURE_LIMIT = 2047

    enum class Problem { NameTooLong, SignatureTooLong, NoEmail, BadEmail, BadReplyTo }

    fun blank() = SenderForm(null, "", "", "", "", useForNew = false, isDefault = false, mayDelete = false)

    fun form(detail: IdentityDetail) = SenderForm(
        detail.id, detail.name, detail.email, detail.replyTo, detail.signature,
        useForNew = detail.isDefault, isDefault = detail.isDefault, mayDelete = detail.mayDelete,
    )

    /** Characters as the user counts them: a letter outside the basic plane is one. */
    fun length(text: String): Int = text.codePointCount(0, text.length)

    /** A plain check of "name@host": one at sign, text on both sides, no spaces. The server has the last word. */
    fun looksLikeAddress(text: String): Boolean {
        val t = text.trim()
        val at = t.indexOf('@')
        return at > 0 && at == t.lastIndexOf('@') && at < t.length - 1 && t.none { it.isWhitespace() }
    }

    /** Line breaks as the server keeps them. */
    fun cleanSignature(text: String): String = text.replace("\r\n", "\n").replace('\r', '\n').trimEnd()

    /** What is wrong with [form], or null when it can be saved. */
    fun problem(form: SenderForm): Problem? = when {
        length(form.name) > NAME_LIMIT -> Problem.NameTooLong
        length(form.signature) > SIGNATURE_LIMIT -> Problem.SignatureTooLong
        form.isNew && form.email.isBlank() -> Problem.NoEmail
        form.isNew && !looksLikeAddress(form.email) -> Problem.BadEmail
        form.replyTo.isNotBlank() && !looksLikeAddress(form.replyTo) -> Problem.BadReplyTo
        else -> null
    }

    /** Whether the user changed anything since the form was opened. */
    fun dirty(original: SenderForm, now: SenderForm): Boolean = original != now

    /** The heading of a row: the name, or the address when there is no name. */
    fun title(detail: IdentityDetail): String = detail.name.ifBlank { detail.email }

    /** The line under it: the address, unless it is the heading already. */
    fun subtitle(detail: IdentityDetail): String? = detail.email.takeIf { detail.name.isNotBlank() }

    /** Whether saving asks the core to make the sender the default: only when switched on and it was not. */
    fun makesDefault(form: SenderForm): Boolean = form.useForNew && !form.isDefault
}
