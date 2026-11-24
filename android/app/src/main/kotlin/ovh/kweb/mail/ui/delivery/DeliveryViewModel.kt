package ovh.kweb.mail.ui.delivery

import ovh.kweb.mail.accounts.AccountSession
import android.app.ActivityManager
import android.app.Application
import android.content.Context
import android.content.pm.PackageManager
import android.os.Build
import android.os.PowerManager
import android.util.Log
import androidx.core.app.NotificationManagerCompat
import androidx.core.content.ContextCompat
import androidx.core.net.toUri
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.async
import kotlinx.coroutines.awaitAll
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharingStarted
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.flow.flatMapLatest
import kotlinx.coroutines.flow.flowOf
import kotlinx.coroutines.flow.receiveAsFlow
import kotlinx.coroutines.flow.stateIn
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import ovh.kweb.mail.R
import ovh.kweb.mail.accounts.AccountLabels
import ovh.kweb.mail.accounts.Accounts
import ovh.kweb.mail.background.Background
import ovh.kweb.mail.background.DeliveryMode
import ovh.kweb.mail.background.GooglePush
import ovh.kweb.mail.background.ModeRules
import ovh.kweb.mail.background.PushService
import ovh.kweb.mail.background.ServiceLog
import ovh.kweb.mail.background.CheckRecord
import ovh.kweb.mail.background.CheckScheduler
import ovh.kweb.mail.background.Notifier
import ovh.kweb.mail.background.Settings
import ovh.kweb.mail.background.StartFailure
import ovh.kweb.mail.background.SwitchOutcome
import ovh.kweb.mail.ui.SessionState
import uniffi.kweb_core.ConnectionStatus

/** Everything the Delivery check shows. Texts and times are made from it by the screen. */
data class DeliveryState(
    /** Host of the server, null until the account is read. */
    val host: String? = null,
    val status: ConnectionStatus = ConnectionStatus.Connecting,
    val startFailure: StartFailure? = null,
    val notificationsAllowed: Boolean = true,
    val batteryExempt: Boolean = true,
    val backgroundRestricted: Boolean = false,
    val lastCheck: CheckRecord? = null,
    val mailEnabled: Boolean = true,
    val remindersEnabled: Boolean = true,
    val removeWhenReadElsewhere: Boolean = true,
    val signingOut: Boolean = false,
    /** Push through Google of this account. */
    val push: PushView = PushView(),
    /** Why the always-on entry is shown (accounts that need the connection); empty when it is not. */
    val serviceReasons: List<ModeRules.ServiceReason> = emptyList(),
)

/** The state of the phone that the app cannot observe: re-read on every resume. */
private data class SystemState(val notificationsAllowed: Boolean, val batteryExempt: Boolean, val backgroundRestricted: Boolean)

private data class Extras(val host: String? = null, val signingOut: Boolean = false)

@OptIn(ExperimentalCoroutinesApi::class)
class DeliveryViewModel(app: Application, private val session: AccountSession) : AndroidViewModel(app) {
    /** The switches and the last check of the account this screen is for. */
    private val settings: Settings get() = session.settings
    private val serviceLog = ServiceLog.of(app)
    private val system = MutableStateFlow(readSystem())
    private val extras = MutableStateFlow(Extras())
    private val googleMode = GoogleModeController(
        switchOn = { GooglePush.switchOn(app, session) },
        switchOff = { GooglePush.switchOff(app, session) },
    )
    private val messageChannel = Channel<Int>(Channel.BUFFERED)

    val state: StateFlow<DeliveryState> = combine(
        session.status,
        session.settings.changes,
        combine(serviceLog.changes, googleMode.busy, GooglePush.switching) { tick, busy, others -> Triple(tick, busy, others) },
        system,
        extras,
    ) { status, _, _, sys, extra ->
        val settings = settings
        DeliveryState(
            push = PushView(
                mode = settings.deliveryMode,
                line = ModeRules.line(session.core.webPushState()),
                fallback = settings.pushFallback,
                busy = googleMode.busy.value || session.record.id in GooglePush.switching.value,
            ),
            serviceReasons = ModeRules.serviceReasons(PushService.deliveries(getApplication())),
            host = extra.host,
            status = status,
            startFailure = serviceLog.lastStartFailure,
            notificationsAllowed = sys.notificationsAllowed,
            batteryExempt = sys.batteryExempt,
            backgroundRestricted = sys.backgroundRestricted,
            lastCheck = settings.lastCheck,
            mailEnabled = settings.mailEnabled,
            remindersEnabled = settings.remindersEnabled,
            removeWhenReadElsewhere = settings.removeWhenReadElsewhere,
            signingOut = extra.signingOut,
        )
    }.stateIn(viewModelScope, SharingStarted.WhileSubscribed(5_000), DeliveryState())

    /** Set while the question about push through Google is open: true when it is about switching on. */
    val googlePrompt: StateFlow<Boolean?> get() = googlePromptFlow

    private val googlePromptFlow = MutableStateFlow<Boolean?>(null)

    /** The Settings row and the Delivery check button: asks before switching, with the warning when switching on. */
    fun askGoogleMode() {
        if (googleMode.busy.value || session.record.id in GooglePush.switching.value) return
        googlePromptFlow.value = session.settings.deliveryMode != DeliveryMode.Google
    }

    fun dismissGoogleMode() {
        googlePromptFlow.value = null
    }

    /** The user said yes. The sequence runs on the account's own lock and can take a minute and a half. */
    fun confirmGoogleMode() {
        val turningOn = googlePromptFlow.value ?: return
        googlePromptFlow.value = null
        viewModelScope.launch {
            val result = if (turningOn) googleMode.turnOn() else googleMode.turnOff()
            PushTexts.result(result)?.let { messageChannel.send(it) }
            if (result == GoogleModeResult.SwitchedOn) offerOthers()
        }
    }

    /** Set while the question "the other accounts too?" is open: the accounts that still keep the connection. */
    val othersPrompt: StateFlow<List<OtherAccounts.Other>?> get() = othersPromptFlow

    private val othersPromptFlow = MutableStateFlow<List<OtherAccounts.Other>?>(null)

    /** After a switch on: asks about the accounts that still keep the connection (and with it the always-on entry). */
    private suspend fun offerOthers() {
        val others = withContext(Dispatchers.IO) {
            val app = getApplication<Application>()
            val records = Accounts.load(app)
            val labels = AccountLabels.of(records, Accounts.names.value)
            records.map { OtherAccounts.Other(it.id, labels.getValue(it.id), Settings.of(app, it).deliveryMode) }
        }
        val offer = OtherAccounts.toOffer(session.record.id, others)
        if (offer.isNotEmpty()) othersPromptFlow.value = offer
    }

    fun dismissOthers() {
        othersPromptFlow.value = null
    }

    /**
     * The user said yes for the other accounts: each is switched on by the same sequence as this one, side by side
     * (each under its own account's lock, on the push scope, so leaving the screen does not stop them). One snackbar
     * says how it went; an account that could not switch keeps the connection and its own Delivery check says why.
     */
    fun confirmOthers() {
        val others = othersPromptFlow.value ?: return
        othersPromptFlow.value = null
        val app = getApplication<Application>()
        viewModelScope.launch {
            val outcomes = others.map { other ->
                async(Dispatchers.IO) {
                    val target = Accounts.session(other.id) ?: return@async false
                    GooglePush.switchOn(app, target) is SwitchOutcome.Done
                }
            }.awaitAll()
            messageChannel.send(if (outcomes.all { it }) R.string.push_done_on_others else R.string.push_failed_others)
        }
    }

    /** Short texts for a snackbar, as string resource ids. */
    val messages: Flow<Int> get() = messageChannel.receiveAsFlow()

    /** Set while the sign-out question is open: what signing out would lose. */
    val signOutPrompt: StateFlow<Unsent?> get() = promptFlow

    private val promptFlow = MutableStateFlow<Unsent?>(null)
    private var asking = false

    init {
        viewModelScope.launch {
            val host = withContext(Dispatchers.IO) {
                val account = session.core.account()
                hostOf(account?.serverUrl)
            }
            extras.update { it.copy(host = host) }
        }
    }

    /** Call on every resume: the user may just have come back from the system settings. */
    fun refreshSystem() {
        system.value = readSystem()
        Background.refreshStatus(getApplication())
    }

    fun setMailEnabled(value: Boolean) {
        settings.mailEnabled = value
    }

    fun setRemindersEnabled(value: Boolean) {
        settings.remindersEnabled = value
    }

    fun setRemoveWhenReadElsewhere(value: Boolean) {
        settings.removeWhenReadElsewhere = value
    }

    /** Runs one safety check now. */
    fun runCheck() {
        CheckScheduler.checkSoon(getApplication())
        messageChannel.trySend(R.string.delivery_check_queued)
    }

    /** Asks the service and the live connection to try again. The app is in front, so Android allows it. */
    fun retryConnection() {
        viewModelScope.launch(Dispatchers.IO) {
            try {
                Background.onAppForeground(getApplication())
            } catch (e: Exception) {
                Log.w(Background.TAG, "retry failed", e)
            }
        }
    }

    fun sendTestNotification() {
        val delivered = Notifier(getApplication()).sendTest(session.record)
        if (!delivered) messageChannel.trySend(R.string.delivery_test_off)
    }

    /**
     * The first step of signing out, for both screens: reads what has not reached the server, then
     * [signOutPrompt] opens the question. When that cannot be read the question does not open,
     * because it could not say what is lost.
     */
    fun askSignOut() {
        if (asking || extras.value.signingOut) return
        asking = true
        viewModelScope.launch {
            try {
                promptFlow.value = withContext(Dispatchers.IO) {
                    val core = session.core
                    // A core with no session (a removal that was cut short) has nothing to read and nothing to lose.
                    if (core.account() == null) SignOutRules.unsent(emptyList(), emptyList()) else SignOutRules.unsent(core.pendingSends(), core.drafts())
                }
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                Log.w(Background.TAG, "reading the unsent mail failed", e)
                messageChannel.send(R.string.sign_out_check_failed)
            } finally {
                asking = false
            }
        }
    }

    fun dismissSignOutPrompt() {
        promptFlow.value = null
    }

    /**
     * Removes the active account: signs it out of its core, takes it off the list, cancels its notifications and
     * deletes its files; the other accounts are not touched. Sends the app to the sign-in screen when none is
     * left (otherwise the screens start over on the next account).
     */
    fun signOut() {
        promptFlow.value = null
        if (extras.value.signingOut) return
        extras.update { it.copy(signingOut = true) }
        viewModelScope.launch {
            val removed = try {
                withContext(Dispatchers.IO) {
                    Accounts.remove(session.record.id)
                }
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                Log.w(Background.TAG, "sign-out failed", e)
                false
            }
            extras.update { it.copy(signingOut = false) }
            if (removed) SessionState.set(Accounts.records.value.isNotEmpty()) else messageChannel.send(R.string.delivery_sign_out_failed)
        }
    }

    private fun readSystem(): SystemState {
        val app = getApplication<Application>()
        val allowed = NotificationManagerCompat.from(app).areNotificationsEnabled()
        val exempt = app.getSystemService(PowerManager::class.java).isIgnoringBatteryOptimizations(app.packageName)
        val restricted = app.getSystemService(ActivityManager::class.java).isBackgroundRestricted
        return SystemState(allowed, exempt, restricted)
    }

    companion object {
        /** `https://mail.kweb.ovh:8443/x` becomes `mail.kweb.ovh`; null when there is no host. */
        fun hostOf(serverUrl: String?): String? =
            serverUrl?.let { runCatching { it.toUri().host }.getOrNull() }?.takeIf { it.isNotEmpty() }

        /** True when Android 13's runtime permission is held (always true before Android 13). */
        fun permissionGranted(context: Context): Boolean =
            Build.VERSION.SDK_INT < Build.VERSION_CODES.TIRAMISU ||
                ContextCompat.checkSelfPermission(context, NotificationFix.PERMISSION) ==
                PackageManager.PERMISSION_GRANTED
    }
}
