package ovh.kweb.mail.ui.delivery

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
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharingStarted
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.flow.receiveAsFlow
import kotlinx.coroutines.flow.stateIn
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import ovh.kweb.mail.R
import ovh.kweb.mail.background.Background
import ovh.kweb.mail.background.CheckRecord
import ovh.kweb.mail.background.CheckScheduler
import ovh.kweb.mail.background.Notifier
import ovh.kweb.mail.background.Settings
import ovh.kweb.mail.background.StartFailure
import ovh.kweb.mail.ui.SessionState
import ovh.kweb.mail.ui.thread.BlobFiles
import uniffi.kweb_core.ConnectionStatus
import uniffi.kweb_core.coreVersion

/** Everything the Delivery check shows. Texts and times are made from it by the screen. */
data class DeliveryState(
    /** Host of the server, null until the account is read. */
    val host: String? = null,
    val status: ConnectionStatus = ConnectionStatus.Connecting,
    val startFailure: StartFailure? = null,
    val notificationsAllowed: Boolean = true,
    val batteryExempt: Boolean = true,
    val lastCheck: CheckRecord? = null,
    val mailEnabled: Boolean = true,
    val remindersEnabled: Boolean = true,
    val removeWhenReadElsewhere: Boolean = true,
    val coreVersion: String? = null,
    val signingOut: Boolean = false,
)

/** The state of the phone that the app cannot observe: re-read on every resume. */
private data class SystemState(val notificationsAllowed: Boolean, val batteryExempt: Boolean)

private data class Extras(val host: String? = null, val version: String? = null, val signingOut: Boolean = false)

class DeliveryViewModel(app: Application) : AndroidViewModel(app) {
    private val settings = Settings.of(app)
    private val system = MutableStateFlow(readSystem())
    private val extras = MutableStateFlow(Extras())
    private val messageChannel = Channel<Int>(Channel.BUFFERED)

    val state: StateFlow<DeliveryState> = combine(
        Background.status,
        settings.changes,
        system,
        extras,
    ) { status, _, sys, extra ->
        DeliveryState(
            host = extra.host,
            status = status,
            startFailure = settings.lastStartFailure,
            notificationsAllowed = sys.notificationsAllowed,
            batteryExempt = sys.batteryExempt,
            lastCheck = settings.lastCheck,
            mailEnabled = settings.mailEnabled,
            remindersEnabled = settings.remindersEnabled,
            removeWhenReadElsewhere = settings.removeWhenReadElsewhere,
            coreVersion = extra.version,
            signingOut = extra.signingOut,
        )
    }.stateIn(viewModelScope, SharingStarted.WhileSubscribed(5_000), DeliveryState())

    /** Short texts for a snackbar, as string resource ids. */
    val messages: Flow<Int> get() = messageChannel.receiveAsFlow()

    /** Set while the sign-out question is open: what signing out would lose. */
    val signOutPrompt: StateFlow<Unsent?> get() = promptFlow

    private val promptFlow = MutableStateFlow<Unsent?>(null)
    private var asking = false

    init {
        viewModelScope.launch {
            val (host, version) = withContext(Dispatchers.IO) {
                val account = Background.core(getApplication()).account()
                hostOf(account?.serverUrl) to coreVersion()
            }
            extras.update { it.copy(host = host, version = version) }
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
        val delivered = Notifier(getApplication()).sendTest()
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
                    val core = Background.core(getApplication())
                    SignOutRules.unsent(core.pendingSends(), core.drafts())
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

    /** Signs out of the core, tidies the background half, and sends the app to the sign-in screen. */
    fun signOut() {
        promptFlow.value = null
        if (extras.value.signingOut) return
        extras.update { it.copy(signingOut = true) }
        viewModelScope.launch {
            val signedOut = try {
                withContext(Dispatchers.IO) {
                    val core = Background.core(getApplication())
                    try {
                        core.signOut()
                    } catch (e: CancellationException) {
                        throw e
                    } catch (e: Exception) {
                        Log.w(Background.TAG, "sign-out reported an error", e)
                    }
                    // Whatever the error was, an account that is gone from the core is signed out.
                    val gone = core.account() == null
                    if (gone) {
                        Background.afterSignOut(getApplication())
                        // Downloaded attachments and pictures are mail too; this block runs off the main thread.
                        BlobFiles.clear(getApplication())
                    }
                    gone
                }
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                Log.w(Background.TAG, "sign-out failed", e)
                false
            }
            extras.update { it.copy(signingOut = false) }
            if (signedOut) SessionState.set(false) else messageChannel.send(R.string.delivery_sign_out_failed)
        }
    }

    private fun readSystem(): SystemState {
        val app = getApplication<Application>()
        val allowed = NotificationManagerCompat.from(app).areNotificationsEnabled()
        val exempt = app.getSystemService(PowerManager::class.java).isIgnoringBatteryOptimizations(app.packageName)
        return SystemState(allowed, exempt)
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
