package ovh.kweb.mail.ui.home

import android.app.Application
import android.util.Log
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import java.time.LocalDate
import java.time.ZoneId
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import ovh.kweb.mail.accounts.AccountSession
import ovh.kweb.mail.background.Background
import ovh.kweb.mail.ui.SessionState
import ovh.kweb.mail.ui.UiPrefs
import uniffi.kweb_core.ConnectionStatus
import uniffi.kweb_core.FileEntry
import uniffi.kweb_core.FileKind
import uniffi.kweb_core.MessageHeader
import uniffi.kweb_core.Occurrence

data class HomeState(
    val loaded: Boolean = false,
    val at: Long = 0,
    val unread: Int = 0,
    val newest: List<MessageHeader> = emptyList(),
    val next: Occurrence? = null,
    val then: Occurrence? = null,
    val tasksToday: List<Occurrence> = emptyList(),
    val overdue: Int = 0,
    val tomorrow: Int = 0,
    val flagged: Int = 0,
    val oldestFlaggedDays: Int? = null,
    /** null: files are not available (the tile is left out quietly). */
    val files: List<FileEntry>? = null,
    /** The away message is on; [awayUntil] is when it ends, Unix seconds. */
    val away: Boolean = false,
    val awayUntil: Long? = null,
    val problem: HomeRules.Problem? = null,
    /** When the problem was first seen on this run, Unix seconds. */
    val problemSince: Long? = null,
)

/** The tiles of the Home tab: what the cache holds, read when the screen comes up and when the sync wrote something. */
class HomeViewModel(app: Application, private val session: AccountSession) : AndroidViewModel(app) {
    private val stateFlow = MutableStateFlow(HomeState())
    private var generation = 0

    val state: StateFlow<HomeState> get() = stateFlow

    init {
        viewModelScope.launch { session.cacheChanges.collect { load() } }
        viewModelScope.launch {
            session.status.collect { status ->
                val problem = when (status) {
                    is ConnectionStatus.Offline -> HomeRules.Problem.OFFLINE
                    is ConnectionStatus.SignInNeeded -> HomeRules.Problem.SIGN_IN
                    else -> null
                }
                stateFlow.update {
                    when {
                        problem == null -> it.copy(problem = null, problemSince = null)
                        it.problem == problem -> it
                        else -> it.copy(problem = problem, problemSince = System.currentTimeMillis() / 1000)
                    }
                }
            }
        }
        viewModelScope.launch {
            SessionState.signedIn.collect { signedIn ->
                if (signedIn == false) {
                    generation++
                    stateFlow.value = HomeState()
                }
            }
        }
    }

    fun load() {
        val asked = ++generation
        viewModelScope.launch {
            try {
                val zone = ZoneId.systemDefault()
                val now = System.currentTimeMillis() / 1000
                val result = withContext(Dispatchers.IO) { read(zone, now) }
                if (asked != generation) return@launch
                stateFlow.update { result.copy(problem = it.problem, problemSince = it.problemSince) }
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                Log.w(Background.TAG, "reading the home tiles failed: ${e.javaClass.simpleName}")
            }
        }
    }

    private fun read(zone: ZoneId, now: Long): HomeState {
        val core = session.core
        val today = LocalDate.now(zone)
        fun start(d: LocalDate) = d.atStartOfDay(zone).toEpochSecond()
        val boxes = core.mailboxes()
        val inboxBox = boxes.firstOrNull { it.role == "inbox" }
        val inbox = inboxBox?.let { core.messages(it.id, 300u, 0u) }.orEmpty()
        val flagged = HomeRules.flagged(inbox)
        // The unread tile covers the folders the user chose for this account (the Inbox alone by default).
        val covered = HomeRules.covered(boxes, UiPrefs(getApplication()).homeFolders(session.record.id))
        val lists = if (covered.size == 1 && covered[0].id == inboxBox?.id) listOf(inbox) else covered.map { box -> core.messages(box.id, 300u, 0u) }
        val unreadMail = if (lists.size == 1) lists[0] else HomeRules.merge(lists)
        // The agenda call asks the server and falls back to the cache, as the Calendar tab does.
        val agenda = runCatching { core.agenda(start(today.minusDays(60)), start(today.plusDays(8)), zone.id).entries }.getOrDefault(emptyList())
        val (next, then) = HomeRules.nextEvents(agenda, now)
        val tasks = HomeRules.tasks(agenda, start(today), start(today.plusDays(1)), start(today.plusDays(2)))
        val files = runCatching {
            core.files(null, null).entries.filter { it.kind == FileKind.FILE }.sortedByDescending { it.modified ?: 0L }.take(2)
        }.getOrNull()?.takeIf { it.isNotEmpty() }
        val away = runCatching { core.awayMessage() }.getOrNull()
        return HomeState(
            loaded = true,
            at = now,
            unread = HomeRules.unreadCount(covered, lists, unreadMail),
            newest = HomeRules.newestUnread(unreadMail),
            next = next,
            then = then,
            tasksToday = tasks.today,
            overdue = tasks.overdue,
            tomorrow = tasks.tomorrow,
            flagged = flagged.size,
            oldestFlaggedDays = HomeRules.oldestFlaggedDays(flagged, now),
            files = files,
            away = away != null && HomeRules.awayOn(away.isEnabled, away.until, now),
            awayUntil = away?.until,
        )
    }
}
