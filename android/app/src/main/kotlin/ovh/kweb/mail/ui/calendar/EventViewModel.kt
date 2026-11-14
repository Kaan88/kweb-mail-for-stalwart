package ovh.kweb.mail.ui.calendar

import ovh.kweb.mail.accounts.AccountSession
import android.app.Application
import android.util.Log
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.receiveAsFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import ovh.kweb.mail.R
import ovh.kweb.mail.background.Background
import uniffi.kweb_core.EventDetail
import uniffi.kweb_core.Exception as CoreException

enum class EventPhase { LOADING, READY, FAILED, MISSING }

data class EventState(
    val phase: EventPhase = EventPhase.LOADING,
    val detail: EventDetail? = null,
    val calendarName: String? = null,
    /** An answer or a delete is on its way. */
    val busy: Boolean = false,
    val message: CalendarMessage? = null,
)

/** What leaving the screen means for the delete dialog: which part of a repeating event goes. */
enum class DeleteScope { ALL, THIS_ONE }

/** One event or task for its own screen: reads it, answers an invitation, deletes it. */
class EventViewModel(app: Application, private val session: AccountSession, private val eventId: String, val recurrenceId: String?) : AndroidViewModel(app) {
    private val stateFlow = MutableStateFlow(EventState())
    private val leaveChannel = Channel<Unit>(Channel.BUFFERED)
    private var loadJob: Job? = null

    val state: StateFlow<EventState> get() = stateFlow

    /** Sent once the entry is gone: the screen goes back. */
    val left: Flow<Unit> get() = leaveChannel.receiveAsFlow()

    /** Reads the entry. What is on screen stays while it is read again. */
    fun load() {
        loadJob?.cancel()
        loadJob = viewModelScope.launch {
            try {
                val (detail, name) = withContext(Dispatchers.IO) {
                    val core = session.core
                    val detail = core.event(eventId, recurrenceId)
                    val name = try {
                        core.calendars().firstOrNull { it.id == detail.input.calendarId }?.name
                    } catch (e: CancellationException) {
                        throw e
                    } catch (e: Exception) {
                        null
                    }
                    detail to name
                }
                stateFlow.update { it.copy(phase = EventPhase.READY, detail = detail, calendarName = name ?: it.calendarName) }
            } catch (e: CancellationException) {
                throw e
            } catch (e: CoreException.Invalid) {
                Log.w(Background.TAG, "the entry is gone: ${e.msg}")
                stateFlow.update { if (it.detail == null) it.copy(phase = EventPhase.MISSING) else it }
            } catch (e: Exception) {
                Log.w(Background.TAG, "reading an entry failed: ${e.javaClass.simpleName}")
                stateFlow.update { if (it.detail == null) it.copy(phase = EventPhase.FAILED) else it.copy(message = CalendarMessage.Res(R.string.cal_event_failed)) }
            }
        }
    }

    fun retry() {
        stateFlow.update { it.copy(phase = EventPhase.LOADING) }
        load()
    }

    /** Answers the invitation: [status] is "accepted", "tentative" or "declined". */
    fun respond(status: String) {
        if (stateFlow.value.busy) return
        stateFlow.update { it.copy(busy = true, message = null) }
        viewModelScope.launch {
            try {
                withContext(Dispatchers.IO) { session.core.respondToEvent(eventId, status) }
                stateFlow.update { it.copy(busy = false) }
                load()
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                Log.w(Background.TAG, "answering an invitation failed: ${e.javaClass.simpleName}")
                stateFlow.update { it.copy(busy = false, message = CalendarMessage.of(e, R.string.cal_respond_failed)) }
            }
        }
    }

    /** Deletes the whole entry, or only the occurrence that was opened. */
    fun delete(scope: DeleteScope) {
        if (stateFlow.value.busy) return
        stateFlow.update { it.copy(busy = true, message = null) }
        viewModelScope.launch {
            try {
                val occurrence = recurrenceId.takeIf { scope == DeleteScope.THIS_ONE }
                withContext(Dispatchers.IO) { session.core.deleteEvent(eventId, occurrence) }
                leaveChannel.send(Unit)
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                Log.w(Background.TAG, "deleting an entry failed: ${e.javaClass.simpleName}")
                stateFlow.update { it.copy(busy = false, message = CalendarMessage.of(e, R.string.cal_delete_failed)) }
            }
        }
    }

    fun dismissMessage() = stateFlow.update { it.copy(message = null) }
}
