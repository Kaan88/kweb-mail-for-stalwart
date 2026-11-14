package ovh.kweb.mail.accounts

import java.net.URI

/** How an account is called where several are shown: its login, or the login and the host when two logins are equal. */
object AccountLabels {
    fun of(records: List<AccountRecord>): Map<String, String> {
        val counts = records.groupingBy { it.username.lowercase() }.eachCount()
        return records.associate { r ->
            val clash = (counts[r.username.lowercase()] ?: 0) > 1
            r.id to if (clash) "${r.username} · ${host(r.serverUrl)}" else r.username
        }
    }

    fun host(serverUrl: String): String =
        try { URI(serverUrl.trim()).host } catch (_: Exception) { null } ?: serverUrl.trim()
}
