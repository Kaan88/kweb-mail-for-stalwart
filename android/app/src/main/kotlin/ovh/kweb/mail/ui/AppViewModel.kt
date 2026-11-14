package ovh.kweb.mail.ui

import android.app.Application
import android.util.Log
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.receiveAsFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import kotlinx.coroutines.withTimeoutOrNull
import ovh.kweb.mail.R
import androidx.lifecycle.ViewModelStore
import ovh.kweb.mail.accounts.AccountLabels
import ovh.kweb.mail.accounts.AccountRecord
import ovh.kweb.mail.accounts.AccountSession
import ovh.kweb.mail.ui.accounts.Incoming
import ovh.kweb.mail.ui.accounts.SwitchOffer
import ovh.kweb.mail.ui.accounts.SwitchRules
import ovh.kweb.mail.accounts.Accounts
import ovh.kweb.mail.background.Background
import ovh.kweb.mail.background.IntentAccount
import ovh.kweb.mail.background.NotificationTarget
import ovh.kweb.mail.background.Notifier
import ovh.kweb.mail.ui.compose.DraftLauncher
import ovh.kweb.mail.ui.signin.SignInNotices
import ovh.kweb.mail.ui.signin.SignInReturn
import ovh.kweb.mail.ui.sending.IncomingShareLauncher
import ovh.kweb.mail.ui.sending.RawShare
import ovh.kweb.mail.ui.sending.ScheduledNotices
import ovh.kweb.mail.ui.sending.SendLaterWords
import java.time.ZoneId
import java.util.Locale
import uniffi.kweb_core.DraftKind

/** Navigation. The back stack never gets empty. */
class AppViewModel(app: Application) : AndroidViewModel(app) {
    private val stackFlow = MutableStateFlow(BackStack.initial)

    /** The state of the screens that live as long as their route is on the stack. */
    val stores = ScreenStores(app)

    /** The view models of the screens that live as long as the account is shown (the list, search, calendar and so on). Cleared on a switch. */
    val accountStore = ViewModelStore()

    private val sessionFlow = MutableStateFlow<AccountSession?>(null)

    /** The account whose screens are shown, with its Core; null until the accounts are read and when there is none. */
    val session: StateFlow<AccountSession?> get() = sessionFlow

    private val offerChannel = Channel<SwitchOffer>(Channel.BUFFERED)

    /** An intent for another account that would have thrown away what is typed: the offer to switch. */
    val offers: Flow<SwitchOffer> get() = offerChannel.receiveAsFlow()

    private val listCommandFlow = MutableStateFlow<ListCommand?>(null)

    /** What the mail list of [ListCommand.accountId] is to do next (a notification tap names a folder). Kept until that list takes it. */
    val listCommand: StateFlow<ListCommand?> get() = listCommandFlow

    /** The share waiting for the user to say which account it is sent from. */
    private val pendingShareFlow = MutableStateFlow<RawShare?>(null)
    val pendingShare: StateFlow<RawShare?> get() = pendingShareFlow

    private val serverUrlFlow = MutableStateFlow<String?>(null)

    private val messageChannel = Channel<Int>(Channel.BUFFERED)

    private val noticeChannel = Channel<UiMessage>(Channel.BUFFERED)

    val stack: StateFlow<List<Route>> get() = stackFlow

    /** Texts for a snackbar that are not one of the app's fixed ones: a time, a count, the core's words. */
    val notices: Flow<UiMessage> get() = noticeChannel.receiveAsFlow()

    /** Short texts for a snackbar, as string resource ids. */
    val messages: Flow<Int> get() = messageChannel.receiveAsFlow()

    /** Address of the signed-in server, to prefill the field when signing in again. */
    val serverUrl: StateFlow<String?> get() = serverUrlFlow

    init {
        viewModelScope.launch { SessionState.load(app) }
        viewModelScope.launch { stackFlow.collect { stores.retain(it) } }
        // A message was handed over to be sent later: say when, on whichever screen is up.
        viewModelScope.launch {
            ScheduledNotices.scheduled.collect { at ->
                val text = SendLaterWords.confirmation(app.resources, at, System.currentTimeMillis() / 1000, ZoneId.systemDefault(), Locale.getDefault())
                noticeChannel.trySend(UiMessage.Text(text))
            }
        }
        // The account was changed (a sign-in finished, one was removed, a notification of another was tapped).
        viewModelScope.launch {
            Accounts.active.collect { record ->
                // Opening the session may open the cache: off the main thread.
                val opened = record?.let { r -> withContext(Dispatchers.IO) { runCatching { Accounts.session(r.id) }.getOrNull() } }
                if (record != null && opened == null) {
                    // This account's cache cannot be opened: show another one, and say so. With none left, the screen offers to remove it.
                    val fallback = withContext(Dispatchers.IO) {
                        Accounts.records.value.firstOrNull { other ->
                            other.id != record.id && runCatching { Accounts.session(other.id) }.getOrNull() != null
                        }
                    }
                    if (fallback != null) {
                        messageChannel.trySend(R.string.accounts_open_failed)
                        withContext(Dispatchers.IO) { Accounts.setActive(fallback.id) }
                        return@collect
                    }
                }
                adopt(opened)
            }
        }
        // The browser came back after the app was killed while it was in front: the sign-in is gone.
        viewModelScope.launch {
            SignInReturn.interrupted.collect { interrupted ->
                if (interrupted && SessionState.signedIn.value == true) {
                    SignInReturn.acknowledgeInterrupted()
                    messageChannel.trySend(R.string.signin_error_interrupted)
                }
            }
        }
        // Signing in again gave a user the app already has under another entry.
        viewModelScope.launch {
            SignInNotices.duplicateRemoved.receiveAsFlow().collect { messageChannel.trySend(R.string.signin_error_already_added) }
        }
        viewModelScope.launch {
            var wasOut = false
            SessionState.signedIn.collect { signedIn ->
                if (signedIn == false) {
                    wasOut = true
                    stackFlow.value = BackStack.initial
                    serverUrlFlow.value = null
                } else if (signedIn == true) {
                    // Whatever was asked while signed out must not wait under the new session.
                    if (wasOut) stackFlow.value = BackStack.initial
                    wasOut = false
                    serverUrlFlow.value = withContext(Dispatchers.IO) { Accounts.load(app); Accounts.active.value?.serverUrl }
                }
            }
        }
    }

    override fun onCleared() {
        stores.clear()
        accountStore.clear()
    }

    fun open(route: Route) = stackFlow.update { BackStack.push(it, route) }

    /**
     * The account the screens show is [now] (null: none). The first one is only noted; a different one starts the
     * screens over: the back stack is the list again and the view models of the old account are dropped.
     */
    private fun adopt(now: AccountSession?) {
        val before = sessionFlow.value
        if (before?.record?.id == now?.record?.id) {
            // The same account (its login was refreshed): keep the screens.
            if (now != null) {
                sessionFlow.value = now
                serverUrlFlow.value = now.record.serverUrl
            }
            return
        }
        stores.session = now
        if (before != null) {
            stackFlow.value = BackStack.afterAccountSwitch()
            stores.clear()
            accountStore.clear()
            listCommandFlow.value = null
        }
        serverUrlFlow.value = now?.record?.serverUrl
        sessionFlow.value = now
    }

    /** The session of [id]: the adopted one when it is that account, otherwise opened off the main thread. */
    private suspend fun sessionOf(id: String): AccountSession? =
        sessionFlow.value?.takeIf { it.record.id == id } ?: withContext(Dispatchers.IO) { runCatching { Accounts.session(id) }.getOrNull() }

    /** Makes [id] the account the screens show and waits until they do. */
    private suspend fun switchAccount(id: String): Boolean {
        if (sessionFlow.value?.record?.id == id) return true
        withContext(Dispatchers.IO) { Accounts.setActive(id) }
        // An account whose cache cannot be opened is never adopted: do not wait for it for ever.
        return withTimeoutOrNull(SWITCH_WAIT_MILLIS) { sessionFlow.first { it?.record?.id == id } } != null
    }

    /** The account that cannot be opened is taken off the list (its files go, nothing can be revoked). */
    fun removeBrokenAccount() {
        viewModelScope.launch {
            val id = Accounts.active.value?.id ?: return@launch
            runCatching { withContext(Dispatchers.IO) { Accounts.remove(id) } }
            SessionState.set(Accounts.records.value.isNotEmpty())
        }
    }

    /** The user picked [id] in the account list. */
    fun selectAccount(accountId: String) {
        viewModelScope.launch { switchAccount(accountId) }
    }

    /**
     * Something arrived for the account named by [accountExtra] (null: the one that was there before several
     * existed). Unknown account: dropped. Otherwise [SwitchRules] decides whether it is opened, opened after
     * a switch, or offered; [then] opens it, in that account.
     */
    private suspend fun intoAccount(accountExtra: String?, then: suspend (AccountRecord) -> Unit) {
        val records = withContext(Dispatchers.IO) { Accounts.load(getApplication()) }
        val record = IntentAccount.resolve(accountExtra, records) ?: return
        val labels = AccountLabels.of(records)
        val label = labels[record.id] ?: record.username
        val same = sessionFlow.value?.record?.id == record.id
        when (SwitchRules.onIncoming(stackFlow.value.last(), same)) {
            Incoming.OPEN -> then(record)
            Incoming.SWITCH_THEN_OPEN -> {
                if (switchAccount(record.id)) then(record)
            }
            Incoming.SWITCH_SAVING_DRAFT -> {
                // The message on screen saves itself in its own account when the screens are dropped.
                val from = labelOf(sessionFlow.value?.record, labels)
                val switched = switchAccount(record.id)
                noticeChannel.trySend(UiMessage.Text(getApplication<Application>().getString(R.string.accounts_draft_saved_in, from)))
                if (switched) then(record)
            }
            Incoming.ASK -> offerChannel.trySend(
                SwitchOffer(label) { viewModelScope.launch { if (switchAccount(record.id)) then(record) } },
            )
        }
    }

    private fun labelOf(record: AccountRecord?, labels: Map<String, String>): String = record?.let { labels[it.id] ?: it.username }.orEmpty()

    /** The root screen a tab of the bottom bar leads to. Each side keeps its own state in its view model. */
    fun switchTo(root: Route) {
        if (SessionState.signedIn.value != true) return
        stackFlow.value = BackStack.switchTo(root)
    }

    /** The activity was opened from the summary of one account's mail: show that account. */
    fun showAccount(accountExtra: String) {
        viewModelScope.launch {
            val signedIn = SessionState.signedIn.first { it != null }
            if (signedIn == true) intoAccount(accountExtra) { listCommandFlow.value = ListCommand(it.id, null) }
        }
    }

    /**
     * The activity was opened from the reminder of [eventId] (for [recurrenceId] when it repeats): show that
     * event over the agenda. Waits for the first answer of the session; a signed-out app ignores it.
     */
    fun openReminder(accountExtra: String?, eventId: String, recurrenceId: String?, androidId: Int) {
        viewModelScope.launch {
            val signedIn = SessionState.signedIn.first { it != null }
            if (signedIn != true) return@launch
            intoAccount(accountExtra) { record ->
                stackFlow.update { BackStack.showOver(it, BackStack.forReminder(eventId, recurrenceId)) }
                // The Open button does not remove its notification by itself.
                withContext(Dispatchers.IO) { runCatching { Notifier(getApplication()).cancelReminder(record, androidId) } }
            }
        }
    }

    /** An entry was saved: back to the agenda. It reads again when it comes up. */
    fun eventSaved() = stackFlow.update { BackStack.afterEventSaved(it) }

    /** A contact was saved: the form is gone; a changed contact shows again, a new one leaves the list. */
    fun contactSaved() = stackFlow.update { BackStack.afterContactSaved(it) }

    fun back() = stackFlow.update { BackStack.pop(it) }

    /** What was opened from "Shared with me" is not shared any more: back to that screen, which reads itself again, and say so. */
    fun sharedGone() {
        stackFlow.update { BackStack.backToShared(it) }
        messageChannel.trySend(R.string.shared_gone)
    }

    /**
     * Another app shared something into this one: a new message is started from it and shown over whatever is up
     * (a message being written stays under it, saved as usual). A share names no account: with one account it
     * starts there, with several the user says first which one it is sent from ([pendingShare]). Nobody signed
     * in: it says so and drops the share.
     */
    fun share(raw: RawShare) {
        viewModelScope.launch {
            val signedIn = SessionState.signedIn.first { it != null }
            if (signedIn == true && Accounts.records.value.size > 1) pendingShareFlow.value = raw else startShare(raw)
        }
    }

    /** The user picked [accountId] as the sender of the waiting share. */
    fun shareFrom(accountId: String) {
        val raw = pendingShareFlow.value ?: return
        pendingShareFlow.value = null
        viewModelScope.launch {
            switchAccount(accountId)
            startShare(raw)
        }
    }

    /** The user closed the "Send from" sheet: the share is dropped. */
    fun cancelShare() {
        pendingShareFlow.value = null
    }

    private fun startShare(raw: RawShare) {
        IncomingShareLauncher.handle(
            getApplication(), viewModelScope, raw,
            session = { sessionFlow.first { it != null } },
            open = { draftId -> stackFlow.update { BackStack.showOver(it, BackStack.forCompose(draftId)) } },
            notify = { noticeChannel.trySend(it) },
        )
    }

    /** A message was handed over for sending: back to the list. */
    fun sent() {
        stackFlow.value = BackStack.afterSend()
    }

    /**
     * The Reply button of a mail notification (notification [androidId], 0 when unknown): start the
     * answer and show it over the list. When the draft cannot be made (not signed in, the mail is
     * gone) the screen stays, a message says so and the notification stays too. Only on success is
     * the notification taken away and the core told that was on purpose.
     */
    fun replyTo(accountExtra: String?, messageId: String, androidId: Int) {
        viewModelScope.launch {
            val signedIn = SessionState.signedIn.first { it != null }
            if (signedIn != true) return@launch
            intoAccount(accountExtra) { record ->
                val core = sessionOf(record.id)?.core ?: return@intoAccount
                listCommandFlow.value = ListCommand(record.id, null)
                val draftId = try {
                    DraftLauncher.create(core, DraftKind.Reply(messageId))
                } catch (e: CancellationException) {
                    throw e
                } catch (e: kotlin.Exception) {
                    Log.w(Background.TAG, "starting the answer from a notification failed", e)
                    messageChannel.send(R.string.compose_start_failed)
                    return@intoAccount
                }
                stackFlow.update { BackStack.showOver(it, BackStack.forCompose(draftId)) }
                val app = getApplication<Application>()
                withContext(Dispatchers.IO) {
                    runCatching { core.notificationSwiped(messageId) }
                    if (androidId != 0) runCatching { Notifier(app).cancelMail(record, androidId) }
                }
            }
        }
    }

    /**
     * The activity was opened from the notification of [messageId]: show that message over the
     * list. A message the cache does not have (it should) leaves just the list.
     */
    fun focusMessage(accountExtra: String?, messageId: String) {
        viewModelScope.launch {
            val signedIn = SessionState.signedIn.first { it != null }
            if (signedIn != true) return@launch
            intoAccount(accountExtra) { record ->
                // The folder the message is opened from: its own, as mail of a folder other than the Inbox can notify too.
                val core = sessionOf(record.id)?.core
                val found = withContext(Dispatchers.IO) {
                    try {
                        val header = core?.message(messageId)
                        val folder = header?.let { NotificationTarget.folderFor(it.mailboxIds, core.mailboxes()) }
                        header to folder
                    } catch (e: kotlin.Exception) {
                        Log.w(Background.TAG, "looking up the tapped message failed", e)
                        null to null
                    }
                }
                val (header, folder) = found
                val stack = if (header == null) BackStack.initial else BackStack.forNotification(header.threadId, folder?.id, messageId)
                // The list behind it shows that folder too (null: the Inbox).
                listCommandFlow.value = ListCommand(record.id, folder?.takeIf { it.role != "inbox" }?.id)
                stackFlow.update { BackStack.showOver(it, stack) }
                // The tap removed the notification while the mail is still unread. Tell the core it is gone on
                // purpose, like a swipe, or the next 15-minute check would put it back.
                withContext(Dispatchers.IO) { runCatching { core?.notificationSwiped(messageId) } }
            }
        }
    }

    /** The mail list of [accountId] has taken [command]. */
    fun listCommandDone(command: ListCommand) {
        listCommandFlow.compareAndSet(command, null)
    }
}

private const val SWITCH_WAIT_MILLIS = 10_000L

/** An order for the mail list of one account: show [folderId] (null: the Inbox without a filter). */
data class ListCommand(val accountId: String, val folderId: String?)
