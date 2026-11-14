package ovh.kweb.mail.background

import ovh.kweb.mail.accounts.AccountStatus
import uniffi.kweb_core.WebPushState
import uniffi.kweb_core.WorkWaiting

/** How the server's news reaches this phone for one account. Per account, default [Connection]. */
enum class DeliveryMode(val key: String) {
    /** A permanent connection held by the foreground service (the entry "Connected"). */
    Connection("connection"),

    /** The server's Web Push through Google's push service; no permanent connection. */
    Google("google"),
    ;

    companion object {
        /** Anything but "google", also a missing value, is the connection. */
        fun parse(key: String?): DeliveryMode = if (key == Google.key) Google else Connection
    }
}

/** Why an account is in connection mode although push through Google was wanted (kept for the Delivery check). */
enum class FallbackReason(val key: String) {
    NoPlayServices("no_play_services"),
    NoServerKey("no_server_key"),
    RegistrationRefused("registration_refused"),
    NotVerified("not_verified"),
    SubscriptionGone("subscription_gone"),

    /** Only while switching on: the server could not be reached, so nothing was changed. */
    Unreachable("unreachable"),
    ;

    companion object {
        fun parse(key: String?): FallbackReason? = entries.firstOrNull { it.key == key }
    }
}

/** The reason an account went back to the connection, when, and the error name from Play services if any. */
data class PushFallback(val reason: FallbackReason, val atMillis: Long, val detail: String?)

/** The pure rules of delivery by mode: who needs the service, when to register again, what to ask for when. */
object ModeRules {
    /** Timings of the sequences (decision 3 and 7 and the contract). */
    const val VERIFY_WAIT_MS = 90_000L
    const val VERIFY_TRIES = 3
    const val REGISTRATION_WAIT_MS = 30_000L
    const val RECEIVER_BUDGET_MS = 8_000L
    const val LIVE_STOP_DELAY_MS = 30_000L
    const val REREGISTER_AFTER_MS = 30L * 24 * 3600 * 1000

    /** A pass for something due within this many ms starts now and waits inside the worker. */
    const val EXPEDITED_WINDOW_MS = 60_000L

    /** Where a worker that found work owed now after its own pass (the pass failed) first waits; it doubles each time. */
    const val RETRY_FLOOR_MS = 60_000L

    /** After this many retries (1, 2, 4, 8 minutes) the 15-minute check is left to it. */
    const val MAX_RETRIES = 4

    /** The service runs for the accounts in connection mode only; with none it must not run. */
    fun needsService(modes: Collection<DeliveryMode>): Boolean = modes.any { it == DeliveryMode.Connection }

    /** The accounts the service holds a connection for. */
    fun serviceItems(items: List<AccountStatus>): List<AccountStatus> = items.filter { !it.google }

    /** Which accounts a service begin starts. */
    fun <T> connectionOnly(items: List<T>, modeOf: (T) -> DeliveryMode): List<T> = items.filter { modeOf(it) == DeliveryMode.Connection }

    /**
     * Whether a Google-mode account registers with Play services again: the token is missing, the app version
     * changed, 30 days passed, the server's key changed, or nothing is subscribed (a setup that stopped half way:
     * no subscription, or one whose verification did not come in the time it is waited for).
     */
    fun needsRegistration(
        mode: DeliveryMode,
        token: String?,
        tokenAtMillis: Long,
        storedVersion: Long,
        currentVersion: Long,
        nowMillis: Long,
        state: WebPushState,
    ): Boolean = mode == DeliveryMode.Google && (
        token.isNullOrEmpty() ||
            storedVersion != currentVersion ||
            nowMillis - tokenAtMillis >= REREGISTER_AFTER_MS ||
            state == WebPushState.KeyChanged ||
            isStuck(state, nowMillis)
        )

    /** A state that nothing will get out of by itself: no subscription, or an unverified one that is overdue. */
    fun isStuck(state: WebPushState, nowMillis: Long): Boolean = when (state) {
        is WebPushState.Off -> true
        is WebPushState.Verifying -> nowMillis - state.since * 1000 >= VERIFY_WAIT_MS
        else -> false
    }

    /** What a Google-mode account's push looks like at the moment, for the Delivery check. */
    sealed interface Line {
        data class Registered(val expiresSeconds: Long, val lastMessageAtSeconds: Long?) : Line
        data class Verifying(val sinceSeconds: Long) : Line
        data object Off : Line
        data object KeyChanged : Line
        data object Gone : Line
    }

    fun line(state: WebPushState): Line = when (state) {
        is WebPushState.Off -> Line.Off
        is WebPushState.Verifying -> Line.Verifying(state.since)
        is WebPushState.Registered -> Line.Registered(state.expires, state.lastMessageAt)
        is WebPushState.KeyChanged -> Line.KeyChanged
        is WebPushState.Gone -> Line.Gone
    }

    /** What to do with the verification wait: [tries] used so far. */
    enum class VerifyNext { TryAgain, GiveUp }

    fun afterWaiting(triesUsed: Int): VerifyNext = if (triesUsed >= VERIFY_TRIES) VerifyNext.GiveUp else VerifyNext.TryAgain
}

/** How a pass for owed work is scheduled. */
sealed interface DuePlan {
    /** An expedited one-off that starts at once and waits inside the worker until [waitMs] have passed. */
    data class Expedited(val waitMs: Long) : DuePlan

    /** A plain one-off with this initial delay and the network constraint. */
    data class Delayed(val delayMs: Long) : DuePlan
}

/** Decision 4: asking the cores what is owed and turning the earliest answer into one scheduled pass. */
object DueRules {
    const val SLOTS = 2

    /** The earliest `Due` among [waitings] (ms from now), or null when nothing is owed. */
    fun earliest(waitings: List<WorkWaiting>): Long? =
        waitings.mapNotNull { (it as? WorkWaiting.Due)?.inMs?.toLong() }.minOrNull()

    /**
     * Up to 60 s ahead the pass starts now and waits in the worker (WorkManager refuses a delay on expedited
     * work, and a timer of a process without a service may be frozen). Further ahead it is a plain delayed one.
     * [fromWorker]: asked at the end of a pass. Work owed *now* after a pass means the pass could not do it, so the
     * retry waits a minute, then two, four, eight ([attempt] counts the retries so far), and then stops (null): the
     * 15-minute check carries on. Work owed a few seconds from now (an Undo window, a send time) is no failure and
     * is scheduled for its real time, as when the app asked.
     */
    fun plan(inMs: Long, fromWorker: Boolean = false, attempt: Int = 0): DuePlan? {
        if (fromWorker && inMs <= 0L) return retryDelay(attempt)?.let { DuePlan.Delayed(it) }
        return if (inMs <= ModeRules.EXPEDITED_WINDOW_MS) DuePlan.Expedited(inMs.coerceAtLeast(0)) else DuePlan.Delayed(inMs)
    }

    /** The wait before retry number [attempt] (0 first), null when the retries are used up. */
    fun retryDelay(attempt: Int): Long? =
        if (attempt >= ModeRules.MAX_RETRIES) null else ModeRules.RETRY_FLOOR_MS shl attempt.coerceAtLeast(0)

    /**
     * Unique work names of the due check. A worker that runs under one name must not enqueue that name with
     * REPLACE (it would cancel itself), so it uses the other.
     */
    fun workName(slot: Int): String = if (slot == 0) "kweb.check.due" else "kweb.check.due.b"

    fun otherSlot(slot: Int): Int = if (slot == 0) 1 else 0

    /** How long the worker still waits at [nowMillis] for a [notBeforeMillis] it was given, at most the window. */
    fun waitLeft(notBeforeMillis: Long, nowMillis: Long): Long =
        (notBeforeMillis - nowMillis).coerceIn(0L, ModeRules.EXPEDITED_WINDOW_MS)
}
