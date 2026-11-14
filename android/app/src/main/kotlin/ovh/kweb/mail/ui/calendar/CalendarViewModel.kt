package ovh.kweb.mail.ui.calendar

import android.app.Application
import android.util.Log
import androidx.compose.foundation.lazy.LazyListState
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import java.time.LocalDate
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
import ovh.kweb.mail.ui.SessionState
import uniffi.kweb_core.Occurrence

data class AgendaState(
    val zone: ZoneId,
    val today: LocalDate,
    /** The day the week strip and the title are on. */
    val selected: LocalDate,
    val window: AgendaWindow.Window,
    /** Calendar ids in the core's order: a calendar's position gives its colour. */
    val calendarIds: List<String> = emptyList(),
    val entries: List<Occurrence> = emptyList(),
    val rows: List<AgendaRules.Row> = emptyList(),
    /** An answer has come (from the core or from its cache) for the first time. */
    val loaded: Boolean = false,
    val refreshing: Boolean = false,
    /** The last try failed. What was loaded stays on screen. */
    val failed: Boolean = false,
    /** The server could not be reached: the entries are what the phone has. */
    val fromCache: Boolean = false,
)

/**
 * The agenda: the entries around the selected week from the core, grouped by day. It lives as long
 * as the activity so that going to a mail and back keeps the week and the place in the list.
 */
class CalendarViewModel(app: Application) : AndroidViewModel(app) {
    private val stateFlow = MutableStateFlow(initial())
    private val messageChannel = Channel<CalendarMessage>(Channel.BUFFERED)
    private val scrollChannel = Channel<LocalDate>(Channel.BUFFERED)
    private var job: Job? = null

    /** A day to scroll to once the answer of the fetch that was started for it is in. */
    private var pendingScroll: LocalDate? = null

    /** Counts loads: an answer that is not from the newest one is dropped. */
    private var generation = 0

    val state: StateFlow<AgendaState> get() = stateFlow

    val messages: Flow<CalendarMessage> get() = messageChannel.receiveAsFlow()

    /** Days the list is to scroll to (a tap on the week strip, a week change, the first answer). */
    val scrollTo: Flow<LocalDate> get() = scrollChannel.receiveAsFlow()

    /** Kept here so that the place in the list survives a visit to another screen. */
    val listState = LazyListState()

    init {
        viewModelScope.launch {
            SessionState.signedIn.collect { signedIn ->
                if (signedIn == false) {
                    job?.cancel()
                    generation++
                    pendingScroll = null
                    stateFlow.value = initial()
                    listState.requestScrollToItem(0)
                }
            }
        }
    }

    private fun initial(): AgendaState {
        val zone = ZoneId.systemDefault()
        val today = LocalDate.now(zone)
        return AgendaState(zone, today, today, AgendaWindow.around(weekStart(today)))
    }

    private fun weekStart(date: LocalDate) = WeekRules.weekStart(date, WeekRules.firstDayOfWeek(java.util.Locale.getDefault()))

    /** Reads the window again; the screen shows what it has meanwhile. Called whenever the screen comes up. */
    fun load() {
        val zone = ZoneId.systemDefault()
        val today = LocalDate.now(zone)
        stateFlow.update { it.copy(zone = zone, today = today) }
        fetch(refreshing = false)
    }

    /** Pull to refresh. */
    fun refresh() {
        stateFlow.update { it.copy(refreshing = true) }
        fetch(refreshing = true)
    }

    private fun fetch(refreshing: Boolean) {
        val asked = ++generation
        val window = stateFlow.value.window
        val zone = stateFlow.value.zone
        job?.cancel()
        job = viewModelScope.launch {
            try {
                val (calendarIds, agenda) = withContext(Dispatchers.IO) {
                    val core = Background.core(getApplication())
                    val ids = try {
                        core.calendars().map { it.id }
                    } catch (e: CancellationException) {
                        throw e
                    } catch (e: Exception) {
                        null
                    }
                    val (from, to) = AgendaWindow.seconds(window, zone)
                    ids to core.agenda(from, to, zone.id)
                }
                if (asked != generation) return@launch
                val target = pendingScroll ?: stateFlow.value.selected.takeIf { !stateFlow.value.loaded }
                pendingScroll = null
                stateFlow.update {
                    it.copy(
                        calendarIds = calendarIds ?: it.calendarIds,
                        loaded = true,
                        refreshing = false,
                        failed = false,
                        fromCache = agenda.fromCache,
                    ).withEntries(agenda.entries)
                }
                if (target != null) scrollChannel.send(target)
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                Log.w(Background.TAG, "reading the agenda failed: ${e.javaClass.simpleName}")
                if (asked != generation) return@launch
                stateFlow.update { it.copy(refreshing = false, failed = true) }
                if (refreshing) messageChannel.send(CalendarMessage.Res(R.string.cal_agenda_failed))
            }
        }
    }

    private fun AgendaState.withEntries(entries: List<Occurrence>): AgendaState =
        copy(entries = entries, rows = AgendaRules.rows(AgendaRules.group(entries, zone, window.from, window.to), zone))

    /** Selects [date]: the strip and the title follow, the list scrolls to it, and the window grows when the day is near its edge. */
    fun select(date: LocalDate) {
        val before = stateFlow.value
        val window = AgendaWindow.next(before.window, weekStart(date))
        stateFlow.update { it.copy(selected = date, window = window) }
        if (window != before.window) {
            // The new window's entries are not here yet: scrolling now would end at the wrong day.
            pendingScroll = date
            fetch(refreshing = false)
        } else {
            scrollChannel.trySend(date)
        }
    }

    fun goToToday() {
        val zone = ZoneId.systemDefault()
        val today = LocalDate.now(zone)
        stateFlow.update { it.copy(zone = zone, today = today) }
        select(today)
    }

    fun moveWeek(weeks: Int) = select(WeekRules.shift(stateFlow.value.selected, weeks))

    /** Ticks a task off or on. The box changes at once; when the server refuses it changes back. */
    fun setTaskDone(entry: Occurrence, done: Boolean) {
        replace(entry, done)
        viewModelScope.launch {
            try {
                withContext(Dispatchers.IO) { Background.core(getApplication()).setTaskDone(entry.eventId, done) }
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                Log.w(Background.TAG, "marking a task failed: ${e.javaClass.simpleName}")
                replace(entry, !done)
                messageChannel.send(CalendarMessage.Res(R.string.cal_task_failed))
            }
        }
    }

    private fun replace(entry: Occurrence, done: Boolean) = stateFlow.update { state ->
        state.withEntries(
            state.entries.map {
                if (it.eventId == entry.eventId && it.recurrenceId == entry.recurrenceId && it.startsAt == entry.startsAt) {
                    it.copy(isDone = done)
                } else {
                    it
                }
            },
        )
    }
}
