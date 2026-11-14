package ovh.kweb.mail.ui.enterprise

import androidx.annotation.DrawableRes
import androidx.annotation.StringRes
import java.time.Instant
import java.time.ZoneId
import java.time.format.DateTimeFormatter
import java.util.Locale
import ovh.kweb.mail.R
import uniffi.kweb_core.DeletedItem
import uniffi.kweb_core.DeletedKind

/** An account of the server the administrator can pick: its id (for the core) and how it is shown. */
data class AccountMatch(val id: String, val label: String, val detail: String = "")

/**
 * Whose deleted items: the account the admin key belongs to, or another account of the server, picked from the
 * server's accounts: its id (for the core) and [label] (its address, what the screen shows).
 */
data class AccountChoice(val other: Boolean = false, val id: String = "", val label: String = "") {
    /** What the screen shows for the account. */
    val display: String get() = label.ifBlank { id }

    /** What the core is given: null for the own account. */
    val accountId: String? get() = if (other) id.trim() else null
    val isValid: Boolean get() = !other || id.isNotBlank()
}

/** What the recovery screen shows, worked out from the core's values. Pure, so it is tested. */
object RecoveryRules {
    val kindOrder = listOf(DeletedKind.MAIL, DeletedKind.EVENT, DeletedKind.CONTACT, DeletedKind.FILE, DeletedKind.OTHER)

    @StringRes fun kindTitle(kind: DeletedKind): Int = when (kind) {
        DeletedKind.MAIL -> R.string.ent_kind_mail
        DeletedKind.EVENT -> R.string.ent_kind_event
        DeletedKind.CONTACT -> R.string.ent_kind_contact
        DeletedKind.FILE -> R.string.ent_kind_file
        DeletedKind.OTHER -> R.string.ent_kind_other
    }

    @DrawableRes fun kindIcon(kind: DeletedKind): Int = when (kind) {
        DeletedKind.MAIL -> R.drawable.ic_mail
        DeletedKind.EVENT -> R.drawable.ic_calendar
        DeletedKind.CONTACT -> R.drawable.ic_person
        DeletedKind.FILE -> R.drawable.ic_file_other
        DeletedKind.OTHER -> R.drawable.ic_folder
    }

    /** The items grouped by kind in the order of [kindOrder]; the order inside a kind (newest first) is kept; empty kinds are left out. */
    fun byKind(items: List<DeletedItem>): List<Pair<DeletedKind, List<DeletedItem>>> =
        kindOrder.mapNotNull { kind -> items.filter { it.kind == kind }.takeIf { it.isNotEmpty() }?.let { kind to it } }

    /**
     * Whole days an item is still kept: it was deleted at [deletedAt] and is kept [keepsDays] days. Null when either
     * is not known; 0 when it goes today.
     */
    fun daysLeft(deletedAt: Long?, keepsDays: UInt?, nowMillis: Long): Int? {
        if (deletedAt == null || keepsDays == null) return null
        val end = deletedAt + keepsDays.toLong() * 86_400
        val left = end - nowMillis / 1000
        return if (left <= 0) 0 else ((left + 86_399) / 86_400).toInt()
    }

    fun deletedDay(at: Long, zone: ZoneId = ZoneId.systemDefault(), locale: Locale = Locale.getDefault()): String =
        DateTimeFormatter.ofPattern("d MMM", locale).withZone(zone).format(Instant.ofEpochSecond(at))
}
