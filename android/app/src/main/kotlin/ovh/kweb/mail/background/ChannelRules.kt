package ovh.kweb.mail.background

import ovh.kweb.mail.accounts.AccountRecord

/**
 * Which notification channel a mail notification of an account goes to. Pure, and pinned by tests: the
 * migrated (legacy) account keeps the channel `mail` the old version created, because a channel cannot be
 * changed after it is made and the user's choice of sound and importance lives in it. Every other account has
 * channels of its own, in a channel group named after it, so its sound can be set apart from the rest.
 *
 * Each account has two mail channels: the loud one (the default importance) and a quiet one (low importance,
 * no sound or vibration) for mail that came to an address the user chose "silent" for.
 */
internal object ChannelRules {
    /** The id of the loud mail channel of [record]. */
    fun mail(record: AccountRecord): String = if (record.legacy) Channels.MAIL else "${Channels.MAIL}.${record.id}"

    /** The id of the quiet mail channel of [record]. */
    fun quiet(record: AccountRecord): String = if (record.legacy) Channels.QUIET else "${Channels.QUIET}.${record.id}"

    /** The channel a mail notification of [record] is posted in. */
    fun forMail(record: AccountRecord, quiet: Boolean): String = if (quiet) quiet(record) else mail(record)

    /**
     * The channel group of [record]; null for the legacy account, whose `mail` channel exists without one (a
     * channel cannot be put in a group afterwards).
     */
    fun group(record: AccountRecord): String? = if (record.legacy) null else "account.${record.id}"

    /**
     * The name the system settings show for a channel called [base]. A group names the account for the others;
     * the legacy account has none, so with several accounts its name says whose it is.
     */
    fun name(base: String, record: AccountRecord, label: String?): String =
        if (record.legacy && label != null) "$base · $label" else base

    /** Whether [channelId] is one of the mail channels of [record]. */
    fun isMailChannelOf(record: AccountRecord, channelId: String): Boolean =
        channelId == mail(record) || channelId == quiet(record)
}
