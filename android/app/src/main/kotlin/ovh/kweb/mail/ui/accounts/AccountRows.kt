package ovh.kweb.mail.ui.accounts

import ovh.kweb.mail.accounts.AccountLabels
import ovh.kweb.mail.accounts.AccountRecord

/** One line of the account list: who it is, what is unread in its Inbox and whether it needs attention. */
data class AccountRow(
    val id: String,
    val label: String,
    val host: String,
    val initial: String,
    /** Unread mail in the Inbox; null while it is not known yet. */
    val unread: Int?,
    val active: Boolean,
    /** The account's sign-in is no longer valid: it needs the user. */
    val needsSignIn: Boolean,
)

/** Pure rules of the account list. */
object AccountRows {
    fun build(
        records: List<AccountRecord>,
        activeId: String?,
        unread: Map<String, Int>,
        needsSignIn: Set<String>,
    ): List<AccountRow> {
        val labels = AccountLabels.of(records)
        return records.map { r ->
            AccountRow(
                id = r.id,
                label = labels[r.id] ?: r.username,
                host = AccountLabels.host(r.serverUrl),
                initial = initial(r.username),
                unread = unread[r.id],
                active = r.id == activeId,
                needsSignIn = r.id in needsSignIn,
            )
        }
    }

    /** The letter in the round picture of an account: the first letter or digit of the login, in capitals. */
    fun initial(username: String): String =
        username.firstOrNull { it.isLetterOrDigit() }?.uppercaseChar()?.toString() ?: "?"

    /** Names of accounts are shown (in notifications, headers, settings) only when there is more than one. */
    fun showsAccountNames(count: Int): Boolean = count > 1

    /** The badge text for an unread count: nothing for none or unknown, "99+" for a large number. */
    fun badge(unread: Int?): String? = when {
        unread == null || unread <= 0 -> null
        unread > 99 -> "99+"
        else -> unread.toString()
    }
}
