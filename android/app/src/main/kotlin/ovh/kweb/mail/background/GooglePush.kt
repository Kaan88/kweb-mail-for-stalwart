package ovh.kweb.mail.background

import android.content.Context
import android.util.Log
import java.util.concurrent.ConcurrentHashMap
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Deferred
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.async
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withContext
import ovh.kweb.mail.accounts.AccountRecord
import ovh.kweb.mail.accounts.AccountSession
import ovh.kweb.mail.accounts.Accounts
import uniffi.kweb_core.ConnectionStatus
import uniffi.kweb_core.WebPushRegistration
import uniffi.kweb_core.WebPushState

/** The real [PushBackend] of one account: its core, Play services, the service and the clock. */
internal class AndroidPushBackend(private val context: Context, private val session: AccountSession) : PushBackend {
    private val core get() = session.core

    override fun prepare(): WebPushRegistration? = core.webPushPrepare()

    override suspend fun register(registration: WebPushRegistration): RegistrationResult = GoogleRegistration.request(
        context, session.record.id, registration.sender, registration.subtype, delete = false, timeoutMs = ModeRules.REGISTRATION_WAIT_MS,
    )

    override suspend fun unregister(subtype: String, sender: String) {
        GoogleRegistration.request(context, session.record.id, sender, subtype, delete = true, timeoutMs = UNREGISTER_WAIT_MS)
    }

    override fun subscribe(token: String): WebPushState = core.webPushSubscribe(token)
    override fun state(): WebPushState = core.webPushState()
    override fun renew(): WebPushState = core.webPushRenew()
    override fun drop() = core.webPushDrop()
    override fun forget() = core.webPushForget()
    override fun startService(): Boolean = PushService.start(context)
    override fun isLive(): Boolean = core.status() is ConnectionStatus.Live

    override fun versionCode(): Long = try {
        context.packageManager.getPackageInfo(context.packageName, 0).longVersionCode
    } catch (e: Exception) {
        0L
    }

    override fun nowMillis(): Long = System.currentTimeMillis()
    override suspend fun delay(ms: Long) = kotlinx.coroutines.delay(ms)
    override fun modeChanged() = Accounts.modeChanged()

    private companion object {
        const val UNREGISTER_WAIT_MS = 10_000L
    }
}

/**
 * Push through Google, the sequences that need Android: switching on and off, the renewal step of the
 * 15-minute check, registering again at app start, and clean-up on sign-out. The rules are in [GoogleSwitch]
 * (tested with a fake); here it only gets its backend and a lock per account, so two sequences of one account
 * never run at once. Everything blocks: call from a background thread or a coroutine on `Dispatchers.IO`.
 */
object GooglePush {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)
    private val locks = ConcurrentHashMap<String, Mutex>()

    private fun lock(id: String) = locks.getOrPut(id) { Mutex() }

    private fun switchOf(context: Context, session: AccountSession) = GoogleSwitch(session.settings, AndroidPushBackend(context.applicationContext, session))

    /** The accounts with a switch under way, for every screen: one switch per account at a time. */
    private val switchingFlow = MutableStateFlow<Set<String>>(emptySet())
    val switching: StateFlow<Set<String>> get() = switchingFlow

    /** Everything that runs for an account on the scope of its own, so a removal can cancel it. */
    private val running = ConcurrentHashMap<String, MutableSet<Job>>()

    private fun track(id: String, job: Job) {
        running.getOrPut(id) { java.util.Collections.synchronizedSet(HashSet()) }.add(job)
        job.invokeOnCompletion { running[id]?.remove(job) }
    }

    private fun <T> startSwitch(context: Context, session: AccountSession, block: suspend () -> T): Deferred<T>? {
        val id = session.record.id
        var taken = false
        switchingFlow.update { if (id in it) it else { taken = true; it + id } }
        if (!taken) return null
        // On the scope of its own: a screen that is left must not cancel a switch half way.
        val job = scope.async { lock(id).withLock { block() } }
        track(id, job)
        job.invokeOnCompletion {
            switchingFlow.update { it - id }
            // A switch on that ended after the app was left: the loop it left running stops, owed work is scheduled.
            LiveWindow.settle(context.applicationContext)
        }
        return job
    }

    suspend fun switchOn(context: Context, session: AccountSession): SwitchOutcome =
        startSwitch(context, session, { switchOf(context, session).switchOn() })?.await() ?: SwitchOutcome.Busy

    suspend fun switchOff(context: Context, session: AccountSession): SwitchOutcome =
        startSwitch(context, session, { switchOf(context, session).switchOff() })?.await() ?: SwitchOutcome.Busy

    /**
     * Call from the app start and every foreground: for each Google-mode account whose registration is due
     * (token missing, new version, 30 days, key changed), and for a setup a dead process left, registers again.
     * Returns at once: the sequences (which can wait up to 90 seconds for a verification) run on a scope of their own.
     */
    fun onAppStart(context: Context) {
        scope.launch { startAll(context.applicationContext) }
    }

    private suspend fun startAll(context: Context) {
        for (record in Accounts.load(context)) {
            try {
                val session = Accounts.session(record.id) ?: continue
                val job = scope.async { lock(record.id).withLock { startOne(context, session) } }
                track(record.id, job)
                job.await()
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                Log.w(Background.TAG, "registering again failed for an account", e)
            }
        }
    }

    private suspend fun startOne(context: Context, session: AccountSession) {
        val settings = session.settings
        val switch = switchOf(context, session)
        if (settings.pushSetup) {
            // A process died in the middle of a setup: the same steps finish it. In connection mode this is a switch on.
            if (settings.deliveryMode == DeliveryMode.Google) switch.reregister() else switch.switchOn()
            return
        }
        if (settings.deliveryMode != DeliveryMode.Google) {
            // A switch off or a fallback that was cut short: its clean-up is finished once the connection is up.
            switch.finishCleanUp()
            return
        }
        val state = session.core.webPushState()
        if (ModeRules.needsRegistration(
                settings.deliveryMode, settings.pushToken, settings.pushTokenAt, settings.pushAppVersion,
                AndroidPushBackend(context.applicationContext, session).versionCode(), System.currentTimeMillis(), state,
            )
        ) {
            switch.reregister()
        }
    }

    /** One step of [CheckWorker]'s run for [record], after its check: renew, fall back or register again. Never throws. */
    suspend fun renewStep(context: Context, record: AccountRecord) {
        try {
            val session = Accounts.session(record.id) ?: return
            if (session.settings.deliveryMode != DeliveryMode.Google) {
                if (session.settings.pushSubtype != null) lock(record.id).withLock { switchOf(context, session).finishCleanUp() }
                return
            }
            lock(record.id).withLock { switchOf(context, session).renewStep() }
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            Log.w(Background.TAG, "the push renewal step failed", e)
        }
    }

    /**
     * Before an account is signed out or removed: the subscription on the server first, then Play services. The
     * keys are deleted by `signOut` itself. Errors do not stop the removal. Blocks (a few seconds at most).
     */
    fun tearDownForRemoval(context: Context, session: AccountSession) {
        val settings = session.settings
        if (settings.deliveryMode != DeliveryMode.Google && !settings.pushSetup && settings.pushSubtype == null) return
        // A switch that is waiting for its verification would write into the settings of an account that is gone.
        running[session.record.id]?.toList()?.forEach { it.cancel() }
        try {
            // No lock: what was cancelled may still be inside a call of the core; the removal does not wait for it.
            runBlocking { switchOf(context, session).tearDownForRemoval() }
        } catch (e: Exception) {
            Log.w(Background.TAG, "ending push through Google failed", e)
        }
    }

    /**
     * For an account that cannot be opened (its cache is damaged), so there is no core to ask: tells Play services to
     * forget the registration the settings still know. The server's subscription ends by itself within seven days.
     */
    fun unregisterFromSettings(context: Context, record: AccountRecord) {
        val settings = Settings.of(context, record)
        val subtype = settings.pushSubtype ?: return
        val sender = settings.pushSender ?: return
        try {
            runBlocking {
                GoogleRegistration.request(context.applicationContext, record.id, sender, subtype, delete = true, timeoutMs = 10_000L)
            }
        } catch (e: Exception) {
            Log.w(Background.TAG, "unregistering from Play services failed", e)
        }
    }
}
