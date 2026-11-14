package ovh.kweb.mail.accounts

import java.net.URI
import java.util.Locale

/** Plain rules about the list of accounts. */
object AccountRules {
    /**
     * The server address in one form: lower-case scheme and host, the default port dropped, no path, query or
     * trailing slash. A different port is a different server. Text that is not an address is only lower-cased
     * and trimmed.
     */
    fun normaliseServer(url: String): String {
        val text = url.trim()
        val uri = try { URI(text) } catch (_: Exception) { null }
        val host = uri?.host
        val scheme = uri?.scheme
        if (uri == null || host == null || scheme == null) return text.lowercase(Locale.ROOT).trimEnd('/')
        val lowerScheme = scheme.lowercase(Locale.ROOT)
        val default = when (lowerScheme) {
            "https" -> 443
            "http" -> 80
            else -> -1
        }
        val port = uri.port.takeIf { it != -1 && it != default }
        return "$lowerScheme://${host.lowercase(Locale.ROOT)}" + (port?.let { ":$it" } ?: "")
    }

    /**
     * Whether [candidate] is an account the app already has: the same server and the same JMAP account id
     * or the same login (compared without regard to case).
     */
    fun isDuplicate(existing: List<AccountRecord>, candidate: AccountRecord): Boolean {
        val server = normaliseServer(candidate.serverUrl)
        return existing.any { other ->
            other.id != candidate.id &&
                normaliseServer(other.serverUrl) == server &&
                ((candidate.jmapAccountId.isNotEmpty() && other.jmapAccountId == candidate.jmapAccountId) ||
                    (candidate.username.isNotEmpty() && other.username.equals(candidate.username, ignoreCase = true)))
        }
    }
}
