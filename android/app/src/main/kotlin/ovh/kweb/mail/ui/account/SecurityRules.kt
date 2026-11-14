package ovh.kweb.mail.ui.account

import uniffi.kweb_core.ConnectedApp

enum class CredentialKind { APP_PASSWORD, API_KEY }

enum class Expiry { NEVER, DAYS_30, YEAR_1 }

/** What a connected app may reach, in the words of the screen. */
enum class ScopeWord { MAIL, CALENDARS, CONTACTS, FILES, SETTINGS, SECURITY, FULL, STAY_SIGNED_IN }

/** One scope of an app: a known word, or the server's own name when the app does not know it. */
sealed interface ScopePart {
    data class Known(val word: ScopeWord) : ScopePart

    data class Raw(val name: String) : ScopePart
}

/** Why a password form cannot be sent yet. */
enum class PasswordProblem { CURRENT_EMPTY, NEW_EMPTY, MISMATCH }

/** Pure decisions of the account security screen. Nothing here keeps or logs a secret. */
object SecurityRules {
    private const val SCOPE_PREFIX = "urn:stalwart:oauth:scope:"

    /** When an app password or key made now ends; null for none. */
    fun expiresAt(choice: Expiry, nowSeconds: Long): Long? = when (choice) {
        Expiry.NEVER -> null
        Expiry.DAYS_30 -> nowSeconds + 30 * AccountRules.SECONDS_PER_DAY
        Expiry.YEAR_1 -> nowSeconds + 365 * AccountRules.SECONDS_PER_DAY
    }

    /** The server's scope names in words; every name once, in the order given. */
    fun scopes(names: List<String>): List<ScopePart> = names.map { it.trim() }.filter { it.isNotEmpty() }.distinct().map { raw ->
        val word = when (raw.removePrefix(SCOPE_PREFIX).lowercase()) {
            "mail", "email" -> ScopeWord.MAIL
            "calendars", "calendar" -> ScopeWord.CALENDARS
            "contacts" -> ScopeWord.CONTACTS
            "files" -> ScopeWord.FILES
            "settings" -> ScopeWord.SETTINGS
            "security" -> ScopeWord.SECURITY
            "full" -> ScopeWord.FULL
            "offline_access" -> ScopeWord.STAY_SIGNED_IN
            else -> null
        }
        if (word != null) ScopePart.Known(word) else ScopePart.Raw(raw)
    }

    /** The first problem of the change-password form, or null when it can be sent. The new password is checked only for being typed twice the same. */
    fun passwordProblem(current: String, new: String, again: String): PasswordProblem? = when {
        current.isEmpty() -> PasswordProblem.CURRENT_EMPTY
        new.isEmpty() -> PasswordProblem.NEW_EMPTY
        new != again -> PasswordProblem.MISMATCH
        else -> null
    }

    /** The second field is filled and differs from the first: the form says so. */
    fun showsMismatch(new: String, again: String): Boolean = again.isNotEmpty() && new != again

    /** A code from an authenticator app: exactly six digits (spaces typed in the middle are ignored). */
    fun codeIsComplete(code: String): Boolean = code.filterNot { it.isWhitespace() }.let { it.length == 6 && it.all { c -> c in '0'..'9' } }

    /** The code as the core wants it. */
    fun cleanCode(code: String): String = code.filterNot { it.isWhitespace() }

    /** A two-step secret for the clipboard: without the spaces of its groups. */
    fun plainSecret(secret: String): String = secret.filterNot { it.isWhitespace() }

    /** Whether [app] may be revoked from here: this app's own sign-in is left alone (the user signs out instead). */
    fun mayRevoke(app: ConnectedApp): Boolean = !app.isThisApp

    /** Deleting the last passkey is said more strongly. */
    fun isLastPasskey(count: Int): Boolean = count <= 1

    /** The note of a new app password or key: trimmed; blank is allowed. */
    fun cleanNote(note: String): String = note.trim()
}
