package ovh.kweb.mail.ui.attention

import android.app.Application
import android.util.Log
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import java.time.LocalDate
import java.time.ZoneId
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.receiveAsFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import ovh.kweb.mail.R
import ovh.kweb.mail.accounts.AccountSession
import ovh.kweb.mail.background.Background
import ovh.kweb.mail.ui.SessionState
import uniffi.kweb_core.AttentionPlace
import uniffi.kweb_core.AttentionSettings
import uniffi.kweb_core.Finding

/** The service and model the sparkle buttons would use: shown once in the footer of the sheet. */
data class ServiceInfo(val baseUrl: String, val model: String)

data class AttentionState(
    /** An answer (from the core's cache) has come at least once. */
    val loaded: Boolean = false,
    /** Every finding the core has for this account, any place; [AttentionRules.forPlace] picks. */
    val findings: List<Finding> = emptyList(),
    val settings: AttentionSettings? = null,
    /** Unix seconds of the last answer: "found on the phone at ...". */
    val at: Long = 0,
    /** Set only while some finding has a service action, i.e. while an assistant is set up. */
    val service: ServiceInfo? = null,
)

/** A line for a snackbar, with an Undo when the change can be taken back. */
data class AttentionNotice(val text: String, val undo: (() -> Unit)? = null)

/**
 * The attention layer of one account: the findings the core works out from the cache (no network, no model), what the
 * phone does on a tap, and the settings. It lives as long as the account is shown, so every screen that wears a
 * sparkle (Home, the agenda, tasks, a thread) reads the same list.
 */
class AttentionViewModel(app: Application, private val session: AccountSession) : AndroidViewModel(app) {
    private val stateFlow = MutableStateFlow(AttentionState())
    private val noticeChannel = Channel<AttentionNotice>(Channel.BUFFERED)
    private var generation = 0
    private var lastRefresh: Long? = null

    val state: StateFlow<AttentionState> get() = stateFlow

    val notices: Flow<AttentionNotice> get() = noticeChannel.receiveAsFlow()

    init {
        // Anything the sync wrote can change what is worth a look: read the cache again (never the server).
        viewModelScope.launch { session.cacheChanges.collect { reload(refresh = false) } }
        viewModelScope.launch {
            SessionState.signedIn.collect { signedIn ->
                if (signedIn == false) {
                    generation++
                    lastRefresh = null
                    stateFlow.value = AttentionState()
                }
            }
        }
    }

    private fun zone(): String = ZoneId.systemDefault().id

    private fun now(): Long = System.currentTimeMillis() / 1000

    /** The screen came up: reads the findings, asking the server for the agenda first when the last time was a while ago. */
    fun load() = reload(refresh = AttentionRules.dueForRefresh(lastRefresh, now()))

    private fun reload(refresh: Boolean) {
        val asked = ++generation
        val at = now()
        if (refresh) lastRefresh = at
        viewModelScope.launch {
            try {
                val (settings, found, service) = withContext(Dispatchers.IO) {
                    val core = session.core
                    val settings = core.attentionSettings()
                    val found = core.attentionFindings(AttentionPlace.Home, zone(), at, refresh)
                    val service = if (found.any { it.service != null }) {
                        runCatching { core.aiSettings() }.getOrNull()?.let { ServiceInfo(it.baseUrl, it.model) }
                    } else {
                        null
                    }
                    Triple(settings, found, service)
                }
                if (asked != generation) return@launch
                HomeSetting.set(settings.home)
                stateFlow.value = AttentionState(loaded = true, findings = found, settings = settings, at = at, service = service)
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                Log.w(Background.TAG, "reading what is worth a look failed: ${e.javaClass.simpleName}")
            }
        }
    }

    /** The user swiped [finding] away: it stays away. */
    fun dismiss(finding: Finding) {
        stateFlow.update { s -> s.copy(findings = s.findings.filterNot { it.id == finding.id }) }
        viewModelScope.launch {
            try {
                withContext(Dispatchers.IO) { session.core.attentionDismiss(finding.id, now()) }
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                Log.w(Background.TAG, "dismissing a finding failed: ${e.javaClass.simpleName}")
            }
        }
    }

    /** Accept, Maybe or Decline: [status] is "accepted", "tentative" or "declined". */
    fun respond(eventId: String, status: String) {
        viewModelScope.launch {
            try {
                withContext(Dispatchers.IO) { session.core.respondToEvent(eventId, status) }
                notice(R.string.att_answered)
                reload(refresh = true)
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                Log.w(Background.TAG, "answering an invitation failed: ${e.javaClass.simpleName}")
                notice(R.string.att_answer_failed)
            }
        }
    }

    fun markDone(taskId: String) {
        viewModelScope.launch {
            try {
                withContext(Dispatchers.IO) { session.core.setTaskDone(taskId, true) }
                notice(R.string.att_done_marked)
                reload(refresh = true)
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                Log.w(Background.TAG, "marking a task failed: ${e.javaClass.simpleName}")
                notice(R.string.att_done_failed)
            }
        }
    }

    /** Sets these tasks due today, keeping the time of day each had. One Undo puts every due time back. */
    fun moveToday(taskIds: List<String>) {
        viewModelScope.launch {
            val today = LocalDate.now(ZoneId.systemDefault())
            val before = LinkedHashMap<String, String>()
            try {
                withContext(Dispatchers.IO) {
                    val core = session.core
                    for (id in taskIds) {
                        val input = core.event(id, null).input
                        before[id] = input.start
                        core.saveEvent(id, input.copy(start = AttentionRules.movedToToday(input.start, today)))
                    }
                }
                noticeWithUndo(app().resources.getQuantityString(R.plurals.att_moved_today, taskIds.size, taskIds.size)) { undoMove(before) }
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                Log.w(Background.TAG, "moving tasks to today failed: ${e.javaClass.simpleName}")
                notice(R.string.att_moved_failed)
                // What was changed before the failure can still be taken back by the user in the agenda; the list tells the truth.
            }
            reload(refresh = true)
        }
    }

    private fun undoMove(before: Map<String, String>) {
        viewModelScope.launch {
            try {
                withContext(Dispatchers.IO) {
                    val core = session.core
                    for ((id, start) in before) core.saveEvent(id, core.event(id, null).input.copy(start = start))
                }
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                Log.w(Background.TAG, "taking tasks back failed: ${e.javaClass.simpleName}")
                notice(R.string.att_moved_failed)
            }
            reload(refresh = true)
        }
    }

    /** Saves the settings (the screen shows the new ones at once; when the core refuses, the old ones come back). */
    fun saveSettings(settings: AttentionSettings) {
        val old = stateFlow.value.settings
        stateFlow.update { it.copy(settings = settings) }
        HomeSetting.set(settings.home)
        viewModelScope.launch {
            try {
                withContext(Dispatchers.IO) { session.core.attentionSetSettings(settings) }
                reload(refresh = false)
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                Log.w(Background.TAG, "saving the attention settings failed: ${e.javaClass.simpleName}")
                if (old != null) {
                    stateFlow.update { it.copy(settings = old) }
                    HomeSetting.set(old.home)
                }
                notice(R.string.att_settings_failed)
            }
        }
    }

    /** Brings back what was swiped away. */
    fun resetDismissed() {
        viewModelScope.launch {
            try {
                withContext(Dispatchers.IO) { session.core.attentionResetDismissed() }
                notice(R.string.att_settings_reset_done)
                reload(refresh = false)
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                Log.w(Background.TAG, "bringing findings back failed: ${e.javaClass.simpleName}")
            }
        }
    }

    /** The conversation of a cached message, for a button that opens "the mail": [then] runs on the main thread when it is found. */
    fun openMessage(messageId: String, then: (threadId: String) -> Unit) {
        viewModelScope.launch {
            val thread = try {
                withContext(Dispatchers.IO) { session.core.message(messageId)?.threadId }
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                null
            }
            if (thread != null) then(thread)
        }
    }

    /** A short line for a snackbar. */
    fun say(res: Int) = notice(res)

    private fun app() = getApplication<Application>()

    private fun notice(res: Int) {
        noticeChannel.trySend(AttentionNotice(app().getString(res)))
    }

    private fun noticeWithUndo(text: String, undo: () -> Unit) {
        noticeChannel.trySend(AttentionNotice(text, undo))
    }
}
