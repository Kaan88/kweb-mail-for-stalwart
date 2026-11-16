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

    /**
     * The channel of mail that came to one of the user's own addresses ([key]: the address in lower case, as
     * the core names it). The id carries the account and the address, so nothing of one account is ever posted
     * in another's channel and two addresses never share one. The legacy account has no group, but its address
     * channels are told apart by its id like every other.
     */
    fun address(record: AccountRecord, key: String): String = "${Channels.MAIL}.a.${record.id}.$key"

    /** The quiet channel of [key] (no sound or vibration): what an address set to "Quiet" has instead of [address]. */
    fun addressQuiet(record: AccountRecord, key: String): String = "${Channels.QUIET}.a.${record.id}.$key"

    /**
     * The channel a mail notification of [record] is posted in: that of the address it came to ([address], null
     * when it matched none of the user's addresses) or, with none, the account's default channel.
     */
    fun forMail(record: AccountRecord, quiet: Boolean, address: String? = null): String = when {
        address == null -> if (quiet) quiet(record) else mail(record)
        quiet -> addressQuiet(record, address)
        else -> address(record, address)
    }

    /** The address a channel of [record] is made for; null for any other channel (also another account's). */
    fun addressOf(record: AccountRecord, channelId: String): String? {
        val loud = "${Channels.MAIL}.a.${record.id}."
        val quiet = "${Channels.QUIET}.a.${record.id}."
        return when {
            channelId.startsWith(loud) -> channelId.removePrefix(loud)
            channelId.startsWith(quiet) -> channelId.removePrefix(quiet)
            else -> null
        }?.takeIf { it.isNotEmpty() }
    }

    /**
     * The address channels [record] should have: the loud one for each address that notifies, the quiet one for
     * each address set to quiet, none for one that is off. [notify] and [quiet] hold addresses in lower case.
     */
    fun wantedAddressChannels(record: AccountRecord, notify: Collection<String>, quiet: Collection<String>): Set<String> =
        notify.map { address(record, it) }.toSet() + quiet.map { addressQuiet(record, it) }

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
        channelId == mail(record) || channelId == quiet(record) || addressOf(record, channelId) != null
}
