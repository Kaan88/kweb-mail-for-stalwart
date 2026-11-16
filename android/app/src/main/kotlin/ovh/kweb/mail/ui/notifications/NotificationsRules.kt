package ovh.kweb.mail.ui.notifications

import ovh.kweb.mail.accounts.AccountRecord
import ovh.kweb.mail.background.ChannelRules
import uniffi.kweb_core.AddressChoice
import uniffi.kweb_core.AddressMode

/** The decisions of the Notifications screen that need no Android. */
object NotificationsRules {
    /** The choices in the order the dialog offers them: the default first. */
    val modes: List<AddressMode> = listOf(AddressMode.NOTIFY, AddressMode.SILENT, AddressMode.OFF)

    /** The name an address goes by in a channel id: lower case, as the core names it. */
    fun key(address: String): String = address.trim().lowercase()

    /**
     * The address channels an account keeps for [choices]: the loud one for an address that notifies, the quiet one
     * for an address set to quiet, none for one that is off.
     */
    fun wantedChannels(record: AccountRecord, choices: List<AddressChoice>): Set<String> = ChannelRules.wantedAddressChannels(
        record,
        notify = choices.filter { it.mode == AddressMode.NOTIFY }.map { key(it.address) },
        quiet = choices.filter { it.mode == AddressMode.SILENT }.map { key(it.address) },
    )

    /** [choices] with [address] set to [mode] (compared without regard to case), for showing the choice before the core has answered. */
    fun with(choices: List<AddressChoice>, address: String, mode: AddressMode): List<AddressChoice> =
        choices.map { if (it.address.equals(address, ignoreCase = true)) it.copy(mode = mode) else it }
}
