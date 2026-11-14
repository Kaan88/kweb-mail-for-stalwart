package ovh.kweb.mail.ui.away

import ovh.kweb.mail.accounts.AccountSession
import android.app.Application
import android.util.Log
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
import ovh.kweb.mail.ui.UiMessage

enum class AwayPhase { LOADING, READY, FAILED }

data class AwayState(
    val phase: AwayPhase = AwayPhase.LOADING,
    val form: AwayForm = AwayForm(),
    /** The form as it was when it was read, to tell whether the user changed anything. */
    val initial: AwayForm = AwayForm(),
    val zone: ZoneId = ZoneId.systemDefault(),
    val saving: Boolean = false,
    /** The mail filters are on, so switching the away message on pauses them; false while that is unknown. */
    val filtersOn: Boolean = false,
    val error: UiMessage? = null,
) {
    val dirty: Boolean get() = phase == AwayPhase.READY && AwayRules.isChanged(form, initial, zone)
}

/** The away message screen: reads the message from the server, changes the form, saves. It lives as long as its route. */
class AwayViewModel(app: Application, private val session: AccountSession) : AndroidViewModel(app) {
    private val stateFlow = MutableStateFlow(AwayState())
    private val doneChannel = Channel<Unit>(Channel.BUFFERED)
    private var loadJob: Job? = null

    val state: StateFlow<AwayState> get() = stateFlow

    /** Sent once the message is saved: the screen goes back without asking about changes. */
    val saved: Flow<Unit> get() = doneChannel.receiveAsFlow()

    /** Reads the message. Does nothing while a read runs, and never replaces a form that is open. */
    fun load() {
        if (loadJob?.isActive == true || stateFlow.value.phase == AwayPhase.READY) return
        stateFlow.update { it.copy(phase = AwayPhase.LOADING) }
        viewModelScope.launch {
            // Only a hint on the screen: when the filters cannot be read, nothing is shown.
            try {
                val filters = withContext(Dispatchers.IO) { session.core.filters() }
                stateFlow.update { it.copy(filtersOn = filters.isActive) }
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                Log.w(Background.TAG, "reading the mail filters failed: ${e.javaClass.simpleName}")
            }
        }
        loadJob = viewModelScope.launch {
            try {
                val message = withContext(Dispatchers.IO) { session.core.awayMessage() }
                val zone = ZoneId.systemDefault()
                val form = AwayRules.fromMessage(message, zone)
                stateFlow.update { it.copy(phase = AwayPhase.READY, form = form, initial = form, zone = zone, error = null) }
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                Log.w(Background.TAG, "reading the away message failed: ${e.javaClass.simpleName}")
                stateFlow.update { it.copy(phase = AwayPhase.FAILED) }
            }
        }
    }

    /** Changes the form; the error shown is about the form as it was, so it goes. */
    fun edit(change: (AwayForm) -> AwayForm) = stateFlow.update { it.copy(form = change(it.form), error = null) }

    fun setFrom(date: LocalDate?) = edit { it.copy(from = date) }

    fun setUntil(date: LocalDate?) = edit { it.copy(until = date) }

    fun dismissError() = stateFlow.update { it.copy(error = null) }

    fun save() {
        val state = stateFlow.value
        if (state.saving || state.phase != AwayPhase.READY) return
        stateFlow.update { it.copy(saving = true, error = null) }
        viewModelScope.launch {
            try {
                val message = AwayRules.toMessage(state.form, state.zone)
                withContext(Dispatchers.IO) { session.core.setAwayMessage(message) }
                doneChannel.send(Unit)
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                Log.w(Background.TAG, "saving the away message failed: ${e.javaClass.simpleName}")
                stateFlow.update { it.copy(saving = false, error = UiMessage.of(e, R.string.away_save_failed)) }
            }
        }
    }
}
