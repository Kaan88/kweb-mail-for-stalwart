package ovh.kweb.mail.background

import kotlinx.coroutines.CancellationException
import uniffi.kweb_core.WebPushRegistration
import uniffi.kweb_core.WebPushState
import uniffi.kweb_core.Exception as CoreException

/** What Play services answered to a registration request. */
sealed interface RegistrationResult {
    data class Token(val token: String) : RegistrationResult

    /**
     * The intent threw, or nothing answered in time. [absent]: Play services is not there to ask (no such service,
     * or the intent threw), which no waiting cures; false when it only stayed silent.
     */
    data class NoPlayServices(val detail: String?, val absent: Boolean = false) : RegistrationResult

    /** Play services answered with an `error` extra. */
    data class Refused(val error: String) : RegistrationResult
}

/** Everything the switch needs from the outside, so the sequences are tested with a fake. All calls may block. */
interface PushBackend {
    /** `webPushPrepare`: null when the server has no usable key. */
    fun prepare(): WebPushRegistration?

    suspend fun register(registration: WebPushRegistration): RegistrationResult

    /** Tells Play services to forget [subtype] (best effort; never throws). */
    suspend fun unregister(subtype: String, sender: String)

    fun subscribe(token: String): WebPushState
    fun state(): WebPushState
    fun renew(): WebPushState
    fun drop()
    fun forget()

    /** Asks the service to look again; false when Android did not accept. */
    fun startService(): Boolean

    /** The account's live connection is up. */
    fun isLive(): Boolean
    fun versionCode(): Long
    fun nowMillis(): Long
    suspend fun delay(ms: Long)

    /** The mode (or the registration) of the account changed: summaries and the service entry follow. */
    fun modeChanged()
}

/** The end of a sequence. [Done] means the account now works through Google. */
sealed interface SwitchOutcome {
    data object Done : SwitchOutcome

    /**
     * Switching on did not work; nothing was changed. [reason] is also kept in the settings. [dead]: the failure
     * itself shows that nothing can arrive through Google any more (Play services is not there).
     */
    data class Failed(val reason: FallbackReason, val detail: String?, val dead: Boolean = false) : SwitchOutcome

    /** Switching off: the service did not start or no connection came up, so the account stays in Google mode. */
    data object ConnectionNotUp : SwitchOutcome

    /** Another switch of this account is under way (on any screen); nothing was done. */
    data object Busy : SwitchOutcome
}

/**
 * The mode switch of one account, in both directions, with no gap: the new road works before the old one is
 * stopped. Settings are written in an order that a process death in the middle leaves a working account:
 * `delivery_mode` goes to `google` only after `Verified`, and to `connection` before anything of Google is torn down.
 *
 * Not thread safe: callers run one sequence per account at a time ([GooglePush] holds a lock per account).
 */
class GoogleSwitch(private val settings: Settings, private val backend: PushBackend) {
    /** Switching on (Settings, app on screen; the connection stays up throughout). */
    suspend fun switchOn(): SwitchOutcome {
        // A second request (another screen, a view model that was left and opened again) for an account that is
        // already in Google mode must not tear down what works: it only checks, and finishes a setup that is owed.
        if (settings.deliveryMode == DeliveryMode.Google) {
            return if (settings.pushSetup || backend.state() !is WebPushState.Registered) reregister() else SwitchOutcome.Done
        }
        settings.pushSetup = true
        settings.pushTries = 0
        settings.clearFallback()
        val failure = establish()
        if (failure != null) {
            cleanUp()
            settings.recordFallback(failure.reason, backend.nowMillis(), failure.detail)
            return failure
        }
        settings.deliveryMode = DeliveryMode.Google
        settings.pushSetup = false
        settings.clearFallback()
        backend.modeChanged()
        // The service looks again and ends if no account needs it.
        backend.startService()
        return SwitchOutcome.Done
    }

    /** Switching off by the user (the app is on screen). */
    suspend fun switchOff(): SwitchOutcome {
        settings.deliveryMode = DeliveryMode.Connection
        backend.modeChanged()
        if (!backend.startService() || !waitForLive()) {
            settings.deliveryMode = DeliveryMode.Google
            backend.modeChanged()
            backend.startService()
            return SwitchOutcome.ConnectionNotUp
        }
        cleanUp()
        settings.clearFallback()
        return SwitchOutcome.Done
    }

    /**
     * The automatic way back. In the background Android may refuse the service: it is recorded as ever and the
     * 15-minute check carries the account; the next time the app is opened the service starts.
     */
    suspend fun fallBack(reason: FallbackReason, detail: String? = null) {
        settings.deliveryMode = DeliveryMode.Connection
        backend.modeChanged()
        settings.recordFallback(reason, backend.nowMillis(), detail)
        // The subscription and the registration go only once the connection is really up (no gap, also here). When it
        // is not (the service was refused in the background, no network), they stay, and [finishCleanUp] ends it at the
        // next start or check.
        finishCleanUp()
    }

    /**
     * For an account in connection mode that still has a registration of Google's push (a switch off or a fallback
     * that was cut short, or whose connection was not up yet): once the connection is up, destroys the subscription,
     * unregisters, deletes the keys. Does nothing while a setup is under way or when there is nothing to end.
     */
    suspend fun finishCleanUp() {
        if (settings.deliveryMode != DeliveryMode.Connection || settings.pushSetup || settings.pushSubtype == null) return
        if (!backend.startService() || !waitForLive()) return
        cleanUp()
    }

    /**
     * Registers again for a Google-mode account (token missing, new version, 30 days, key changed, nothing
     * subscribed), or finishes a setup that was cut short. `pushSetup` stays true until it worked, so the next start or
     * check tries again. A failure that proves nothing about the road that worked before (Play services silent, the
     * server out of reach) leaves that road alone; only a road that is dead (not verified, key gone, Play services
     * not there) falls back. Play services staying silent or refusing is let pass [ModeRules.REGISTER_FAILURES] - 1
     * times in a row and then counts as dead too, so an account never stays in Google mode for good with a
     * registration nobody answers for and no reason shown.
     */
    suspend fun reregister(): SwitchOutcome {
        settings.pushSetup = true
        settings.pushTries = 0
        val failure = establish()
        if (failure != null) {
            if (ModeRules.fallsBack(failure.reason, failure.dead, stillWorks(), failuresAfter(failure))) {
                // The setup is over, so the fallback is a whole one: it starts the service and, once the connection
                // is up, ends what is left of the registration. Left true, the clean-up would be skipped and the next
                // app start would silently try to switch on again.
                settings.pushSetup = false
                settings.pushFailures = 0
                fallBack(failure.reason, failure.detail)
            }
            return failure
        }
        settings.pushSetup = false
        settings.pushFailures = 0
        backend.modeChanged()
        return SwitchOutcome.Done
    }

    private fun stillWorks(): Boolean = runCatching { backend.state() is WebPushState.Registered }.getOrDefault(false)

    /** Counts a failure of Play services itself (not of the server or the network) and returns the count in a row. */
    private fun failuresAfter(failure: SwitchOutcome.Failed): Int {
        val playServices = failure.reason == FallbackReason.NoPlayServices || failure.reason == FallbackReason.RegistrationRefused
        if (playServices) settings.pushFailures = settings.pushFailures + 1
        return settings.pushFailures
    }

    /** A step of the 15-minute check for a Google-mode account. */
    suspend fun renewStep() {
        val local = runCatching { backend.state() }.getOrNull()
        // A setup that was cut short, or an account with nothing subscribed (or nothing verified for too long): again.
        if (settings.pushSetup || (local != null && ModeRules.isStuck(local, backend.nowMillis()))) {
            reregister()
            return
        }
        val state = try {
            backend.renew()
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            return // left to the next run
        }
        when (state) {
            is WebPushState.Gone -> fallBack(FallbackReason.SubscriptionGone)
            is WebPushState.KeyChanged -> reregister()
            is WebPushState.Off, is WebPushState.Verifying -> if (ModeRules.isStuck(state, backend.nowMillis())) reregister()
            else -> Unit
        }
    }

    /** Sign-out and removal of the account: the server first, then Play services. The keys go with `signOut`. */
    suspend fun tearDownForRemoval() {
        runCatching { backend.drop() }
        unregisterKnown()
    }

    /** Registered, subscribed and verified; null on success. */
    private suspend fun establish(): SwitchOutcome.Failed? {
        val registration = try {
            backend.prepare()
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            return failureOf(e)
        } ?: return SwitchOutcome.Failed(FallbackReason.NoServerKey, null)

        when (val r = backend.register(registration)) {
            is RegistrationResult.NoPlayServices -> return SwitchOutcome.Failed(FallbackReason.NoPlayServices, r.detail, dead = r.absent)
            is RegistrationResult.Refused -> return SwitchOutcome.Failed(FallbackReason.RegistrationRefused, r.error)
            is RegistrationResult.Token -> settings.recordPushRegistration(
                registration.subtype, registration.sender, r.token, backend.nowMillis(), backend.versionCode(),
            )
        }
        val token = settings.pushToken ?: return SwitchOutcome.Failed(FallbackReason.RegistrationRefused, null)
        while (true) {
            val state = try {
                backend.subscribe(token)
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                return failureOf(e)
            }
            if (state is WebPushState.Registered) return null
            if (waitForVerified()) return null
            settings.pushTries = settings.pushTries + 1
            if (ModeRules.afterWaiting(settings.pushTries) == ModeRules.VerifyNext.GiveUp) {
                return SwitchOutcome.Failed(FallbackReason.NotVerified, null)
            }
        }
    }

    /** Polls the core's state (the receiver writes `Verified` into it, possibly in another process life) for 90 s. */
    private suspend fun waitForVerified(): Boolean {
        var waited = 0L
        while (waited < ModeRules.VERIFY_WAIT_MS) {
            backend.delay(POLL_MS)
            waited += POLL_MS
            if (backend.state() is WebPushState.Registered) return true
        }
        return false
    }

    private suspend fun waitForLive(): Boolean {
        var waited = 0L
        while (waited < LIVE_WAIT_MS) {
            if (backend.isLive()) return true
            backend.delay(POLL_MS)
            waited += POLL_MS
        }
        return backend.isLive()
    }

    /** Server first, then Play services, then the keys and the state; each step on its own, none stops the rest. */
    private suspend fun cleanUp() {
        runCatching { backend.drop() }
        unregisterKnown()
        runCatching { backend.forget() }
        settings.clearPushRegistration()
    }

    private suspend fun unregisterKnown() {
        val subtype = settings.pushSubtype
        val sender = settings.pushSender
        if (subtype != null && sender != null) runCatching { backend.unregister(subtype, sender) }
    }

    private fun failureOf(e: Exception): SwitchOutcome.Failed = when (e) {
        // The server said no: its own words (the core never puts secrets into them).
        is CoreException.Invalid -> SwitchOutcome.Failed(FallbackReason.RegistrationRefused, e.msg)
        else -> SwitchOutcome.Failed(FallbackReason.Unreachable, null)
    }

    companion object {
        const val POLL_MS = 1_000L
        const val LIVE_WAIT_MS = 20_000L
    }
}
