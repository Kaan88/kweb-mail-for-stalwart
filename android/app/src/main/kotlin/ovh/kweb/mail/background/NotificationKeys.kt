package ovh.kweb.mail.background

import ovh.kweb.mail.accounts.AccountRecord

/**
 * How a notification and its intents are told apart between accounts. Pure, and pinned by tests, because the
 * migrated (legacy) account must keep exactly what the old version posted: no tag, the group named after its
 * JMAP account id, and the old intent data URIs. A new tag or group for it would duplicate everything on
 * screen after an update, and the PendingIntents already posted carry no account.
 *
 * Android identifies a notification by (tag, id); the core's ids are unique within one account only, so every
 * other account has its own tag (its id) and the same ids cannot collide.
 */
internal object NotificationKeys {
    private const val GROUP_PREFIX = "ovh.kweb.mail.mail."

    /** The ids the app itself uses (below the core's mail ids). */
    private const val SUMMARY_ID = 1

    /** The notification tag of [record]: none for the legacy account, its id for every other. */
    fun tag(record: AccountRecord): String? = if (record.legacy) null else record.id

    /** The group key of the mail notifications and their summary. */
    fun group(record: AccountRecord): String =
        GROUP_PREFIX + if (record.legacy) record.jmapAccountId.ifEmpty { "none" } else "local-${record.id}"

    /** The path segments of an intent's `kwebmail://notification/...` data URI. */
    fun uriSegments(record: AccountRecord, kind: String, id: String): List<String> =
        if (record.legacy) listOf(kind, id) else listOf("a", record.id, kind, id)

    /** [uriSegments] as the text of the URI (without the escaping `Uri.Builder` applies to a segment). */
    fun uriText(record: AccountRecord, kind: String, id: String): String =
        "kwebmail://notification/" + uriSegments(record, kind, id).joinToString("/")

    /**
     * Whether the notification ([tag], [id]) on screen belongs to [record]. For the legacy account: no tag, and
     * the id is the summary, a reminder's or a mail's (1000 or more); never the service entry (2), the test (3)
     * or the check (4). For the others: the tag is the account's id.
     */
    fun owns(record: AccountRecord, tag: String?, id: Int): Boolean =
        if (record.legacy) tag == null && (id == SUMMARY_ID || ReminderRules.isReminderId(id) || id >= FIRST_MAIL_ID)
        else tag == record.id

    /** Ids below this are the app's own; the core keeps mail ids at or above it. */
    const val FIRST_MAIL_ID = 1000
}

/** Which account an intent that came from a notification is for. */
internal object IntentAccount {
    /**
     * - An id that is registered gives that account.
     * - An unknown id gives nothing: the intent is dropped (and its notification cancelled by the caller).
     * - No extra gives the legacy account if there is one: that is every notification and snooze alarm the
     *   old version posted.
     */
    fun resolve(extra: String?, records: List<AccountRecord>): AccountRecord? =
        if (extra == null) records.firstOrNull { it.legacy } else records.firstOrNull { it.id == extra }
}
