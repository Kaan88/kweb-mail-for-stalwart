package ovh.kweb.mail.accounts

import java.net.URI

/** How an account is called where several are shown: its login, or the login and the host when two logins are equal. */
object AccountLabels {
    fun of(records: List<AccountRecord>, names: Map<String, String> = emptyMap()): Map<String, String> {
        val counts = records.groupingBy { it.username.lowercase() }.eachCount()
        return records.associate { r ->
            val clash = (counts[r.username.lowercase()] ?: 0) > 1
            r.id to (names[r.id]?.takeIf { it.isNotBlank() } ?: if (clash) "${r.username} · ${host(r.serverUrl)}" else r.username)
        }
    }

    /**
     * What a notification, a channel or its group says about [record] besides the mail: the name the user gave the
     * account, else the login where several accounts exist, else nothing (one unnamed account adds no label).
     */
    fun forNotice(record: AccountRecord, records: List<AccountRecord>, names: Map<String, String>): String? =
        names[record.id]?.takeIf { it.isNotBlank() } ?: if (records.size > 1) of(records)[record.id] else null

    fun host(serverUrl: String): String =
        try { URI(serverUrl.trim()).host } catch (_: Exception) { null } ?: serverUrl.trim()
}
