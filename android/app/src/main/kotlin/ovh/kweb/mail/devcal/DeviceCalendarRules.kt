package ovh.kweb.mail.devcal

import android.provider.CalendarContract
import ovh.kweb.mail.accounts.AccountRecord
import uniffi.kweb_core.DeviceSyncOutcome
import uniffi.kweb_core.DeviceSyncReport

/**
 * What does not need Android: the names, the timing and the line a run leaves behind.
 *
 * The device calendar mirrors an account's own calendars into the phone's calendar provider through a sync adapter. Its
 * design follows Bulwark Mobile's device sync (github.com/bulwarkmail/native, AGPL-3.0 with an app store permission);
 * only the design was taken, no code.
 */
object DeviceCalendarRules {
    /** The Android account type: `<applicationId>.account`. */
    const val ACCOUNT_TYPE = "ovh.kweb.mail.account"

    const val AUTHORITY = CalendarContract.AUTHORITY

    /** The one thing an Android account holds: which app account it is for. No password, no token. */
    const val USER_DATA_ACCOUNT = "appAccountId"

    /** A periodic sync, in seconds. */
    const val PERIOD_SECONDS = 3600L

    /** At most one requested sync a minute for an account (a burst of changes is one). */
    const val MIN_GAP_MS = 60_000L

    /** The permissions the provider asks for. */
    val PERMISSIONS = arrayOf(android.Manifest.permission.READ_CALENDAR, android.Manifest.permission.WRITE_CALENDAR)

    /** The name of the Android account: the login, with the server's name added when two accounts have the same login. */
    fun accountName(record: AccountRecord, all: List<AccountRecord>): String {
        val same = all.count { it.username == record.username }
        if (same <= 1) return record.username
        val host = record.serverUrl.substringAfter("://").substringBefore('/').substringBefore(':')
        return "${record.username} ($host)"
    }

    fun mayRequest(lastMs: Long, nowMs: Long): Boolean = lastMs == 0L || nowMs - lastMs >= MIN_GAP_MS || nowMs < lastMs

    /** The id every sync id of an account starts with. */
    fun syncPrefix(appAccountId: String): String = "$appAccountId/"
}

/** What the last run was, in the form the settings keep and the screen shows. */
data class DeviceRun(val kind: Kind, val numbers: List<Int> = emptyList()) {
    enum class Kind(val key: String) {
        Ok("ok"),
        StoppedDeletions("deletions"),
        StoppedEmpty("empty"),
        Denied("denied"),
        SignIn("signin"),
        Failed("failed"),
        Other("other"),
    }

    /** True when the run left the phone as the server has it (a screen shows no problem). */
    val good: Boolean get() = kind == Kind.Ok

    fun encode(): String = (listOf(kind.key) + numbers.map { it.toString() }).joinToString(":")

    companion object {
        fun of(report: DeviceSyncReport): DeviceRun = when (report.outcome) {
            DeviceSyncOutcome.DONE -> DeviceRun(
                Kind.Ok,
                listOf(
                    report.written.toInt(),
                    report.uploaded.toInt() + report.created.toInt() + report.destroyed.toInt(),
                    report.conflicts.toInt(),
                    report.reverted.toInt(),
                    report.notSent.toInt(),
                ),
            )
            DeviceSyncOutcome.STOPPED_TOO_MANY_DELETIONS -> DeviceRun(Kind.StoppedDeletions, listOf(report.heldBack.toInt()))
            DeviceSyncOutcome.STOPPED_SERVER_EMPTY -> DeviceRun(Kind.StoppedEmpty, listOf(report.heldBack.toInt()))
        }

        fun decode(text: String?): DeviceRun? {
            val parts = text?.split(':') ?: return null
            val kind = Kind.entries.firstOrNull { it.key == parts.firstOrNull() } ?: return null
            return DeviceRun(kind, parts.drop(1).mapNotNull { it.toIntOrNull() })
        }
    }
}
