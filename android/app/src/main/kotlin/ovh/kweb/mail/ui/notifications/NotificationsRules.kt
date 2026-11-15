package ovh.kweb.mail.ui.notifications

import uniffi.kweb_core.AddressChoice
import uniffi.kweb_core.AddressMode

/** The decisions of the Notifications screen that need no Android. */
object NotificationsRules {
    /** The choices in the order the dialog offers them: the default first. */
    val modes: List<AddressMode> = listOf(AddressMode.NOTIFY, AddressMode.SILENT, AddressMode.OFF)

    /** [choices] with [address] set to [mode] (compared without regard to case), for showing the choice before the core has answered. */
    fun with(choices: List<AddressChoice>, address: String, mode: AddressMode): List<AddressChoice> =
        choices.map { if (it.address.equals(address, ignoreCase = true)) it.copy(mode = mode) else it }
}
