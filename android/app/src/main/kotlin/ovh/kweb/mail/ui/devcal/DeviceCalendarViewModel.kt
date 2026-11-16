package ovh.kweb.mail.ui.devcal

import android.app.Application
import android.content.ContentResolver
import android.util.Log
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
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import ovh.kweb.mail.R
import ovh.kweb.mail.accounts.AccountLabels
import ovh.kweb.mail.accounts.AccountSession
import ovh.kweb.mail.accounts.Accounts
import ovh.kweb.mail.background.Background
import ovh.kweb.mail.devcal.DeviceCalendarAccounts
import ovh.kweb.mail.devcal.DeviceCalendarController
import ovh.kweb.mail.devcal.DeviceCalendarRules
import ovh.kweb.mail.devcal.DeviceRun
import uniffi.kweb_core.DeviceCalendarChoice

/** What the Device calendar screen shows for one account. [calendars] is null until they are read. */
data class DeviceCalendarState(
    /** The account's name where several are shown; null with one. */
    val account: String? = null,
    val on: Boolean = false,
    /** Android's own switch for this account's calendar sync is off (the user turned it off in Android's settings). */
    val pausedInAndroid: Boolean = false,
    val permission: Boolean = false,
    val calendars: List<DeviceCalendarChoice>? = null,
    val lastRunAt: Long = 0L,
    val lastRun: DeviceRun? = null,
    val syncing: Boolean = false,
    /** Switching on or off is under way. */
    val busy: Boolean = false,
)

class DeviceCalendarViewModel(app: Application, private val session: AccountSession) : AndroidViewModel(app) {
    private val settings get() = session.settings
    private val calendars = MutableStateFlow<List<DeviceCalendarChoice>?>(null)
    private val syncing = MutableStateFlow(false)
    private val busy = MutableStateFlow(false)
    private val permission = MutableStateFlow(DeviceCalendarController.hasPermission(app))
    private val messageChannel = Channel<Int>(Channel.BUFFERED)
    private val blockedFlow = MutableStateFlow<DeviceCalendarController.Disabled.Waiting?>(null)

    /**
     * Set when switching off found edits made on the phone that could not be sent: nothing was removed and the screen asks
     * whether to keep the device calendar on or to remove it anyway (and lose them). Null otherwise.
     */
    val blocked: StateFlow<DeviceCalendarController.Disabled.Waiting?> get() = blockedFlow

    val state: StateFlow<DeviceCalendarState> = combine(
        settings.changes, calendars, syncing, busy, permission,
    ) { _, list, sync, working, allowed ->
        val s = settings
        val run = s.deviceCalendarRun
        DeviceCalendarState(
            account = AccountLabels.forNotice(session.record, Accounts.records.value, Accounts.names.value),
            on = s.deviceCalendar,
            pausedInAndroid = s.deviceCalendar && !DeviceCalendarController.androidSyncOn(getApplication(), session.record),
            permission = allowed,
            calendars = list,
            lastRunAt = run?.first ?: 0L,
            lastRun = DeviceRun.decode(run?.second),
            syncing = sync,
            busy = working,
        )
    }.stateIn(viewModelScope, SharingStarted.WhileSubscribed(5_000), DeviceCalendarState())

    /** Short texts for a snackbar, as string resource ids. */
    val messages: Flow<Int> get() = messageChannel.receiveAsFlow()

    private val listener = ContentResolver.addStatusChangeListener(
        ContentResolver.SYNC_OBSERVER_TYPE_ACTIVE,
    ) { updateSyncing() }

    init {
        updateSyncing()
    }

    override fun onCleared() {
        ContentResolver.removeStatusChangeListener(listener)
    }

    private fun updateSyncing() {
        val account = DeviceCalendarAccounts.find(getApplication(), session.record.id)
        syncing.value = account != null && try {
            ContentResolver.isSyncActive(account, DeviceCalendarRules.AUTHORITY)
        } catch (e: SecurityException) {
            false
        }
    }

    /** Reads what can have changed while the screen was away: the permission and the calendars. */
    fun load() {
        permission.value = DeviceCalendarController.hasPermission(getApplication())
        updateSyncing()
        viewModelScope.launch {
            try {
                calendars.value = withContext(Dispatchers.IO) { session.core.deviceCalendars() }
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                Log.w(Background.TAG, "reading the calendars failed (${e.javaClass.simpleName})")
            }
        }
    }

    fun permissionResult(granted: Boolean) {
        permission.value = DeviceCalendarController.hasPermission(getApplication())
        if (granted && permission.value) turnOn()
    }

    /** The permission is granted (the screen asked and explained): everything else is set up. */
    fun turnOn() {
        if (busy.value) return
        busy.value = true
        viewModelScope.launch {
            try {
                val done = withContext(Dispatchers.IO) { DeviceCalendarController.enable(getApplication(), session.record) }
                if (!done) messageChannel.send(R.string.devcal_on_failed)
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                Log.w(Background.TAG, "switching the device calendar on failed (${e.javaClass.simpleName})")
                messageChannel.send(R.string.devcal_on_failed)
            } finally {
                busy.value = false
                updateSyncing()
            }
        }
    }

    /** The dialog was answered "Keep it on". */
    fun keepOn() {
        blockedFlow.value = null
    }

    /** Switches off; with [force] (the user was told what is lost) the calendars go even when edits could not be sent. */
    fun turnOff(force: Boolean = false) {
        if (busy.value) return
        busy.value = true
        blockedFlow.value = null
        viewModelScope.launch {
            try {
                when (val done = withContext(Dispatchers.IO) { DeviceCalendarController.disable(getApplication(), session.record, force) }) {
                    DeviceCalendarController.Disabled.Removed -> Unit
                    DeviceCalendarController.Disabled.NotRemoved -> messageChannel.send(R.string.devcal_off_failed)
                    is DeviceCalendarController.Disabled.Waiting -> blockedFlow.value = done
                }
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                Log.w(Background.TAG, "switching the device calendar off failed (${e.javaClass.simpleName})")
                messageChannel.send(R.string.devcal_off_failed)
            } finally {
                busy.value = false
                updateSyncing()
            }
        }
    }

    /** One calendar on or off for the mirror; shown at once and put back if the core refuses. */
    fun setCalendar(id: String, on: Boolean) {
        val before = calendars.value ?: return
        calendars.value = before.map { if (it.id == id) it.copy(on = on) else it }
        viewModelScope.launch {
            try {
                withContext(Dispatchers.IO) { session.core.deviceCalendarSet(id, on) }
                DeviceCalendarController.syncNow(getApplication(), session.record)
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                Log.w(Background.TAG, "setting a calendar failed (${e.javaClass.simpleName})")
                calendars.value = before
                messageChannel.send(R.string.devcal_choice_failed)
            }
        }
    }

    fun syncNow() {
        DeviceCalendarController.syncNow(getApplication(), session.record)
        updateSyncing()
    }
}
