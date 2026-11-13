package ovh.kweb.mail.ui.signin

import java.net.URI
import java.net.URISyntaxException

/** What the user types in the server field, turned into the URL the core is given. */
object ServerAddress {
    /**
     * Returns the address with a scheme, or null when it cannot be an address at all. A bare
     * host gets `https://`. The core does the strict checks (https only, except for a server on
     * this machine); this only keeps obvious typos from costing a network round trip.
     */
    fun normalise(input: String): String? {
        val text = input.trim()
        if (text.isEmpty() || text.any { it.isWhitespace() }) return null
        val withScheme = if ("://" in text) text else "https://$text"
        val uri = try {
            URI(withScheme)
        } catch (_: URISyntaxException) {
            return null
        }
        if (uri.scheme?.lowercase() !in setOf("http", "https")) return null
        if (uri.host.isNullOrEmpty()) return null
        return withScheme.trimEnd('/')
    }
}
