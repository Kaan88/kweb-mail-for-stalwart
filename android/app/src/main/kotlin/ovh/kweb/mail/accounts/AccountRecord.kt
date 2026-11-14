package ovh.kweb.mail.accounts

/**
 * One account of the app. [id] is made by the app when the sign-in is started (16 hex characters) and is
 * never the JMAP id, which repeats between servers. [legacy] marks the account that was already in the app
 * before several accounts existed: its files, notification ids and intents keep today's names, so an update
 * changes nothing for it (see [AccountPaths] and [NotificationKeys]).
 */
data class AccountRecord(
    val id: String,
    val legacy: Boolean,
    val username: String,
    val serverUrl: String,
    val jmapAccountId: String,
    val addedAt: Long,
)

/** An account whose storage is to be deleted at the next process start, if it is not gone by then. */
data class DiscardEntry(val id: String, val legacy: Boolean) {
    fun encode(): String = if (legacy) "$id$LEGACY_SUFFIX" else id

    companion object {
        private const val LEGACY_SUFFIX = ":legacy"

        fun decode(text: String): DiscardEntry =
            if (text.endsWith(LEGACY_SUFFIX)) DiscardEntry(text.removeSuffix(LEGACY_SUFFIX), true) else DiscardEntry(text, false)
    }
}

/**
 * What the registry holds. [active] is the id of the account the screens show. [discard] holds the encoded
 * [DiscardEntry]s of accounts being added or removed: written before any file exists and cleared after
 * the files are gone, so a process death in between is finished by the sweep at the next start.
 */
data class RegistryState(
    val version: Int = CURRENT_VERSION,
    val accounts: List<AccountRecord> = emptyList(),
    val active: String? = null,
    val discard: Set<String> = emptySet(),
) {
    val activeRecord: AccountRecord? get() = accounts.firstOrNull { it.id == active }

    companion object {
        const val CURRENT_VERSION = 1
    }
}
