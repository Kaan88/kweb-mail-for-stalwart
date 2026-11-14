package ovh.kweb.mail.ui.calendar

import android.app.Application
import android.util.Log
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import java.time.LocalDate
import java.time.LocalDateTime
import java.time.ZoneId
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
import uniffi.kweb_core.Calendar
import uniffi.kweb_core.Exception as CoreException

enum class EditPhase { LOADING, READY, FAILED, MISSING, READ_ONLY, NO_CALENDAR }

data class EditState(
    val phase: EditPhase = EditPhase.LOADING,
    val form: EventForm? = null,
    /** The form as it was when it was opened, to tell whether the user changed anything. */
    val initial: EventForm? = null,
    /** The menu of calendars. */
    val calendars: List<Calendar> = emptyList(),
    val creating: Boolean = true,
    val saving: Boolean = false,
    val error: CalendarMessage? = null,
) {
    val dirty: Boolean get() = form != initial
}

/** The form for a new entry ([eventId] null) or for changing one. */
class EditEventViewModel(
    app: Application,
    private val eventId: String?,
    private val startHint: String?,
    /** The owner's account when the entry is in a calendar shared with the user; null for the user's own. */
    account: String? = null,
) : AndroidViewModel(app) {
    private val backend = eventFormBackend({ Background.core(app) }, account)
    private val stateFlow = MutableStateFlow(EditState(creating = eventId == null))
    private val doneChannel = Channel<Unit>(Channel.BUFFERED)
    private var loadJob: Job? = null

    val state: StateFlow<EditState> get() = stateFlow

    /** Sent once the entry is saved: the screen goes back without asking about changes. */
    val saved: Flow<Unit> get() = doneChannel.receiveAsFlow()

    /** Reads the entry. Does nothing while a read runs, and never replaces a form that is open. */
    fun load() {
        if (loadJob?.isActive == true) return
        if (stateFlow.value.let { it.phase == EditPhase.READY || it.phase == EditPhase.READ_ONLY }) return
        stateFlow.update { it.copy(phase = EditPhase.LOADING) }
        loadJob = viewModelScope.launch {
            try {
                val zone = ZoneId.systemDefault()
                val (all, detail) = withContext(Dispatchers.IO) {
                    backend.calendars() to eventId?.let { backend.event(it) }
                }
                val menu = FormChoices.calendars(all, detail?.input?.calendarId)
                val form = if (detail != null) {
                    EventForms.fromInput(detail.input, detail.hasComplexRepeat)
                } else {
                    val calendar = FormChoices.defaultCalendar(menu)
                    if (calendar == null) {
                        stateFlow.update { it.copy(phase = EditPhase.NO_CALENDAR) }
                        return@launch
                    }
                    val day = startHint?.let { runCatching { LocalDate.parse(it) }.getOrNull() } ?: LocalDate.now(zone)
                    EventForms.blank(calendar.id, day, LocalDateTime.now(zone), zone)
                }
                val readOnly = detail != null && !detail.mayWrite
                stateFlow.update {
                    it.copy(
                        phase = if (readOnly) EditPhase.READ_ONLY else EditPhase.READY,
                        form = form,
                        initial = form,
                        calendars = menu,
                    )
                }
            } catch (e: CancellationException) {
                throw e
            } catch (e: CoreException.Invalid) {
                Log.w(Background.TAG, "the entry to edit is gone: ${e.msg}")
                stateFlow.update { it.copy(phase = EditPhase.MISSING) }
            } catch (e: Exception) {
                Log.w(Background.TAG, "opening the form failed: ${e.javaClass.simpleName}")
                stateFlow.update { it.copy(phase = EditPhase.FAILED) }
            }
        }
    }

    /** Changes the form; the error shown is about the form as it was, so it goes. */
    fun edit(change: (EventForm) -> EventForm) = stateFlow.update { state ->
        state.form?.let { state.copy(form = change(it), error = null) } ?: state
    }

    /** The "All day" switch: an entry without a zone that becomes timed gets the phone's. */
    fun setAllDay(on: Boolean) = edit { it.withAllDay(on, ZoneId.systemDefault().id) }

    fun dismissError() = stateFlow.update { it.copy(error = null) }

    fun save() {
        val state = stateFlow.value
        val form = state.form ?: return
        if (state.saving) return
        val problems = EventForms.problems(form, hasCalendar = state.calendars.any { it.id == form.calendarId })
        if (problems.isNotEmpty()) {
            stateFlow.update { it.copy(error = CalendarMessage.Res(CalendarMessage.problem(problems.first()))) }
            return
        }
        stateFlow.update { it.copy(saving = true, error = null) }
        viewModelScope.launch {
            try {
                val input = EventForms.toInput(form)
                withContext(Dispatchers.IO) { backend.save(eventId, input) }
                doneChannel.send(Unit)
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                Log.w(Background.TAG, "saving an entry failed: ${e.javaClass.simpleName}")
                stateFlow.update { it.copy(saving = false, error = CalendarMessage.of(e, R.string.cal_save_failed)) }
            }
        }
    }
}
