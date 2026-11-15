package ovh.kweb.mail.ui.admin

import ovh.kweb.mail.R
import uniffi.kweb_core.AdminAccess

/** The entry the folder drawer offers for the server's admin area. */
enum class AdminEntry { Dashboard, Administration }

/** Which drawer entry the two remembered flags of an account give, and what it is called. No network, no key. */
object AdminEntryRules {
    /** Null: no accepted key on this phone, so no entry. A key that opens the dashboard goes straight to it. */
    fun entry(keyAccepted: Boolean, mayDashboard: Boolean): AdminEntry? = when {
        !keyAccepted -> null
        mayDashboard -> AdminEntry.Dashboard
        else -> AdminEntry.Administration
    }

    fun label(entry: AdminEntry): Int = if (entry == AdminEntry.Dashboard) R.string.admin_drawer_dashboard else R.string.admin_title

    /** What to remember of an access that was just read: a key counts only while the server accepts it. */
    fun keyAccepted(access: AdminAccess): Boolean = access.connected
}
