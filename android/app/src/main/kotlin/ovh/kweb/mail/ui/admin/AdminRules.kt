package ovh.kweb.mail.ui.admin

import java.util.Locale
import uniffi.kweb_core.AdminAccess
import uniffi.kweb_core.LogLine
import uniffi.kweb_core.ServerAccount
import uniffi.kweb_core.ServerAccountKind

/** The areas of the admin home. The last five are the Enterprise areas (screens another part of the app builds). */
enum class AdminArea(val enterprise: Boolean) {
    Queue(false),
    Accounts(false),
    Blocked(false),
    Logs(false),
    Maintenance(false),
    Dashboard(true),
    History(true),
    Recovery(true),
    Alerts(true),
    Tenants(true),
}

/** What the core says (`Error::Invalid`) when a queued message is not in the queue any more; the host compares it. */
const val QUEUE_GONE = "that message is no longer in the queue"

/** What the access screen shows. */
enum class AccessKind { Locked, Refused, Open }

/** What a key may do, as the rows of "What this key may do". */
enum class MayKind { Queue, Accounts, Blocked, Logs, Maintenance, Enterprise }

data class MayRow(val kind: MayKind, val allowed: Boolean)

/** The rows of the admin home: the upper group, and the Enterprise group with whether its rows are dimmed (no licence). */
data class HomeAreas(val upper: List<AdminArea>, val lower: List<AdminArea>, val lowerDimmed: Boolean)

enum class NameProblem { Empty, BadCharacters }

enum class PasswordProblem { Empty, TooShort, Different }

/** What was typed as a storage limit. */
sealed interface QuotaInput {
    data object NoLimit : QuotaInput

    data class Limit(val bytes: Long) : QuotaInput

    data object Invalid : QuotaInput
}

/** The state of one recipient of a queued message, from the core's plain words. */
enum class RecipientState { Waiting, WillRetry, Delivered, Failed, Other }

enum class LevelFilter { All, Errors, Warnings }

enum class SpanUnit { Minutes, Hours, Days }

data class Span(val unit: SpanUnit, val value: Int)

/** The line under a queued message about its tries. */
sealed interface TryLine {
    data object NotTried : TryLine

    /** Tried [tries] times; the next try is in [next] (null: no next try is planned). */
    data class Tried(val tries: Int, val next: Span?) : TryLine
}

/** The line above the queue's list. */
sealed interface QueueLine {
    data object Empty : QueueLine

    data class Waiting(val count: Int, val next: Span?) : QueueLine

    data class Paused(val count: Int) : QueueLine
}

enum class AccountPill { Admin, Suspended, Group }

/** The decisions of the admin screens that need no Android: they are tested on their own. */
object AdminRules {
    /** How many of a pasted key's characters are shown, at most. */
    const val KEY_SHOWN = 8
    const val MIN_PASSWORD = 8
    const val PAGE = 50u
    const val LOG_LIMIT = 200u

    fun accessKind(access: AdminAccess?, refused: Boolean): AccessKind = when {
        access?.connected == true -> AccessKind.Open
        refused || access?.keyRefused == true -> AccessKind.Refused
        else -> AccessKind.Locked
    }

    /**
     * A key for the confirmation: its first eight characters and dots for the rest. A short key shows less
     * (half of it, at most eight), so that the dialog never gives away a whole key.
     */
    fun maskKey(key: String): String {
        val shown = minOf(KEY_SHOWN, key.length / 2)
        val dots = minOf(key.length - shown, 24)
        return key.take(shown) + "•".repeat(dots)
    }

    /** The clipboard's text as a key: trimmed; null when there is nothing or it cannot be a key (spaces or control characters inside, very long). */
    fun keyFromClipboard(text: CharSequence?): String? {
        val key = text?.toString()?.trim().orEmpty()
        // Printable ASCII only: a control character cannot go into a header and the core refuses such a key.
        if (key.isEmpty() || key.length > 512 || key.any { it.code !in 0x21..0x7e }) return null
        return key
    }

    /** The rows of "What this key may do". */
    fun mayRows(access: AdminAccess): List<MayRow> = listOf(
        MayRow(MayKind.Queue, access.mayQueue),
        MayRow(MayKind.Accounts, access.mayAccounts),
        MayRow(MayKind.Blocked, access.mayBlocked),
        MayRow(MayKind.Logs, access.mayLogs),
        MayRow(MayKind.Maintenance, access.mayMaintenance),
        MayRow(MayKind.Enterprise, enterpriseAreas(access).isNotEmpty()),
    )

    private fun may(access: AdminAccess, area: AdminArea): Boolean = when (area) {
        AdminArea.Queue -> access.mayQueue
        AdminArea.Accounts -> access.mayAccounts
        AdminArea.Blocked -> access.mayBlocked
        AdminArea.Logs -> access.mayLogs
        AdminArea.Maintenance -> access.mayMaintenance
        AdminArea.Dashboard -> access.mayDashboard
        AdminArea.History -> access.mayHistory
        AdminArea.Recovery -> access.mayRecovery
        AdminArea.Alerts -> access.mayAlerts
        AdminArea.Tenants -> access.mayTenants
    }

    private fun enterpriseAreas(access: AdminAccess): List<AdminArea> = AdminArea.entries.filter { it.enterprise && may(access, it) }

    /**
     * The rows of the home. The upper group lists what the key may do. On a licensed server the lower group lists the
     * Enterprise areas the key may open; on a server without a licence it lists all five, dimmed; on a build without
     * the Enterprise parts it is left out.
     */
    fun homeAreas(access: AdminAccess): HomeAreas {
        val upper = AdminArea.entries.filter { !it.enterprise && may(access, it) }
        val enterprise = AdminArea.entries.filter { it.enterprise }
        return when (access.edition) {
            "enterprise" -> HomeAreas(upper, enterpriseAreas(access), lowerDimmed = false)
            "oss" -> HomeAreas(upper, emptyList(), lowerDimmed = false)
            else -> HomeAreas(upper, enterprise, lowerDimmed = true)
        }
    }

    // ---- accounts ----

    /** A name for a new account: letters, digits, dot, hyphen and underscore (the server quietly rewrites other names). */
    fun nameProblem(name: String): NameProblem? = when {
        name.isEmpty() -> NameProblem.Empty
        name.length > 64 || !name.all { it.isAsciiLetterOrDigit() || it == '.' || it == '-' || it == '_' } -> NameProblem.BadCharacters
        name.first() == '.' || name.last() == '.' -> NameProblem.BadCharacters
        else -> null
    }

    private fun Char.isAsciiLetterOrDigit() = this in 'a'..'z' || this in 'A'..'Z' || this in '0'..'9'

    fun passwordProblem(password: String, repeat: String): PasswordProblem? = when {
        password.isEmpty() -> PasswordProblem.Empty
        password.length < MIN_PASSWORD -> PasswordProblem.TooShort
        password != repeat -> PasswordProblem.Different
        else -> null
    }

    /** The first letter of a name, in capitals, for the round picture of a row. */
    fun initial(name: String): String = name.trim().firstOrNull { it.isLetterOrDigit() }?.uppercaseChar()?.toString() ?: "?"

    fun pills(account: ServerAccount): List<AccountPill> = buildList {
        if (account.kind == ServerAccountKind.GROUP) add(AccountPill.Group)
        if (account.isAdmin) add(AccountPill.Admin)
        if (account.isSuspended) add(AccountPill.Suspended)
    }

    /** What is shown under an account's name: its address, else its description. */
    fun secondLine(account: ServerAccount): String = account.email.ifBlank { account.description }

    /** Delete asks for the account's name: a typed name counts when it is the same ignoring spaces around it and capitals. */
    fun deleteConfirmed(typed: String, name: String): Boolean = name.isNotBlank() && typed.trim().equals(name.trim(), ignoreCase = true)

    /** The fraction of the limit that is used, 0 to 1; null without a limit or without a known use. */
    fun usedFraction(used: ULong?, quota: ULong?): Float? {
        if (used == null || quota == null || quota == 0uL) return null
        return (used.toDouble() / quota.toDouble()).coerceIn(0.0, 1.0).toFloat()
    }

    fun nearlyFull(used: ULong?, quota: ULong?): Boolean = (usedFraction(used, quota) ?: 0f) >= 0.9f

    /** Bytes in the units people use, with one decimal where it helps: "5 GB", "1.5 GB", "48 KB". */
    fun formatBytes(bytes: ULong, locale: Locale = Locale.getDefault()): String {
        val units = arrayOf("B", "KB", "MB", "GB", "TB")
        var value = bytes.toDouble()
        var unit = 0
        while (value >= 1024.0 && unit < units.lastIndex) {
            value /= 1024.0
            unit++
        }
        val text = if (unit == 0 || value >= 100 || value == Math.floor(value)) String.format(locale, "%.0f", value) else String.format(locale, "%.1f", value)
        return "$text ${units[unit]}"
    }

    /** What the storage limit field holds for [quota]: gigabytes (or megabytes when smaller), empty for no limit. */
    fun quotaText(quota: ULong?): String {
        if (quota == null) return ""
        val gb = 1024.0 * 1024 * 1024
        val mb = 1024.0 * 1024
        return if (quota.toDouble() >= gb) trim(quota.toDouble() / gb) + " GB" else trim(quota.toDouble() / mb) + " MB"
    }

    private fun trim(v: Double): String = if (v == Math.floor(v)) v.toLong().toString() else String.format(Locale.ROOT, "%.2f", v).trimEnd('0').trimEnd('.')

    /** "5", "5 GB", "500 MB", "1.5tb" or nothing (no limit). A bare number is gigabytes. */
    fun parseQuota(text: String): QuotaInput {
        val t = text.trim().replace(',', '.')
        if (t.isEmpty()) return QuotaInput.NoLimit
        val match = Regex("""^(\d+(?:\.\d+)?)\s*([a-zA-Z]{0,2})$""").matchEntire(t) ?: return QuotaInput.Invalid
        val number = match.groupValues[1].toDoubleOrNull() ?: return QuotaInput.Invalid
        val factor = when (match.groupValues[2].lowercase()) {
            "", "g", "gb" -> 1024.0 * 1024 * 1024
            "m", "mb" -> 1024.0 * 1024
            "t", "tb" -> 1024.0 * 1024 * 1024 * 1024
            else -> return QuotaInput.Invalid
        }
        val bytes = number * factor
        if (bytes < 1 || bytes > Long.MAX_VALUE / 2.0) return QuotaInput.Invalid
        return QuotaInput.Limit(bytes.toLong())
    }

    // ---- queue ----

    /** The core's words for one recipient's state. */
    fun recipientState(status: String): RecipientState = when (status.trim().lowercase()) {
        "waiting" -> RecipientState.Waiting
        "delivered" -> RecipientState.Delivered
        "failed for now" -> RecipientState.WillRetry
        "failed" -> RecipientState.Failed
        else -> RecipientState.Other
    }

    /** "in 4 minutes": [seconds] from now, rounded up, at least one minute. */
    fun span(seconds: Long): Span {
        val minutes = maxOf(1L, (seconds + 59) / 60)
        return when {
            minutes < 60 -> Span(SpanUnit.Minutes, minutes.toInt())
            minutes < 60 * 48 -> Span(SpanUnit.Hours, ((minutes + 59) / 60).toInt())
            else -> Span(SpanUnit.Days, ((minutes + 1439) / 1440).toInt())
        }
    }

    /** A planned try that is due now or already past (the server writes the year 2000 for "at once"): no clock time is shown for it. */
    fun isDue(epochSeconds: Long, now: Long): Boolean = epochSeconds <= now + 30

    fun tryLine(tries: UInt, nextTryAt: Long?, now: Long): TryLine {
        if (tries == 0u) return TryLine.NotTried
        return TryLine.Tried(tries.toInt(), nextTryAt?.let { span(it - now) })
    }

    /** The first recipient and how many more there are. */
    fun recipientsSummary(addresses: List<String>): Pair<String, Int> = (addresses.firstOrNull().orEmpty()) to maxOf(0, addresses.size - 1)

    /** The earliest planned next try among [nextTries] (unix seconds), as a span from [now]; null when none is planned. */
    fun nextTry(nextTries: List<Long?>, now: Long): Span? = nextTries.filterNotNull().minOrNull()?.let { span(it - now) }

    fun queueLine(total: Int, nextTries: List<Long?>, now: Long, paused: Boolean): QueueLine = when {
        total == 0 -> QueueLine.Empty
        paused -> QueueLine.Paused(total)
        else -> QueueLine.Waiting(total, nextTry(nextTries, now))
    }

    // ---- logs ----

    fun matches(filter: LevelFilter, line: LogLine): Boolean = when (filter) {
        LevelFilter.All -> true
        LevelFilter.Errors -> line.level == "error"
        LevelFilter.Warnings -> line.level == "warning"
    }

    /** The log's own text trimmed to what the screen may hold: the server's lines can be very long. */
    fun clip(text: String, max: Int = 600): String = if (text.length <= max) text else text.take(max) + "…"
}
