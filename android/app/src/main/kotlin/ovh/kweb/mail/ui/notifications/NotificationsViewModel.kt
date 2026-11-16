package ovh.kweb.mail.ui.notifications

import android.app.Application
import android.content.Intent
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
import ovh.kweb.mail.background.ChannelRules
import ovh.kweb.mail.background.Channels
import ovh.kweb.mail.background.Background
import ovh.kweb.mail.background.PreviewChoice
import uniffi.kweb_core.AddressChoice
import uniffi.kweb_core.AddressMode

/** Everything the Notifications screen shows for one account. [addresses] is null until they are read. */
data class NotificationsState(
    /** The account's name where several are shown; null with one. */
    val account: String? = null,
    val mailEnabled: Boolean = true,
    val remindersEnabled: Boolean = true,
    val removeWhenReadElsewhere: Boolean = true,
    val preview: PreviewChoice = PreviewChoice.Full,
    /** The buttons of this account's mail notifications, in order (chosen on their own screen). */
    val buttons: List<ovh.kweb.mail.background.MailAction> = emptyList(),
    val addresses: List<AddressChoice>? = null,
)

/** The kinds of channel the screen opens in the system settings (an address's own channel is opened by [NotificationsViewModel.addressChannelSettings]). */
enum class ChannelKind { Mail, Reminders }

/** The Notifications screen of one account: its switches (the same ones the Delivery check used), its addresses, its channels. */
class NotificationsViewModel(app: Application, private val session: AccountSession) : AndroidViewModel(app) {
    private val settings get() = session.settings
    private val addresses = MutableStateFlow<List<AddressChoice>?>(null)
    private val messageChannel = Channel<Int>(Channel.BUFFERED)

    val state: StateFlow<NotificationsState> = combine(settings.changes, addresses, Accounts.records, Accounts.names) { _, list, records, names ->
        val s = settings
        NotificationsState(
            account = AccountLabels.forNotice(session.record, records, names),
            mailEnabled = s.mailEnabled,
            remindersEnabled = s.remindersEnabled,
            removeWhenReadElsewhere = s.removeWhenReadElsewhere,
            preview = s.previewChoice,
            buttons = s.notificationActions,
            addresses = list,
        )
    }.stateIn(viewModelScope, SharingStarted.WhileSubscribed(5_000), NotificationsState())

    /** Short texts for a snackbar, as string resource ids. */
    val messages: Flow<Int> get() = messageChannel.receiveAsFlow()

    init {
        // The addresses come with the first sync and change with the server's identities: read them again whenever the cache moves.
        viewModelScope.launch { session.cacheChanges.collect { load() } }
    }

    /** Reads the addresses and their choices from the core (the cache; no network). */
    fun load() {
        viewModelScope.launch {
            try {
                addresses.value = withContext(Dispatchers.IO) {
                    session.core.addressNotify().also(::reconcileChannels)
                }
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                Log.w(Background.TAG, "reading the address choices failed", e)
            }
        }
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

    fun setPreview(value: PreviewChoice) {
        settings.previewChoice = value
    }

    /** Sets what happens to mail that came to [address]. Shown at once; put back if the core refuses. */
    fun setAddress(address: String, mode: AddressMode) {
        val before = addresses.value ?: return
        addresses.value = NotificationsRules.with(before, address, mode)
        viewModelScope.launch {
            try {
                withContext(Dispatchers.IO) {
                    session.core.setAddressNotify(address, mode)
                    // An address set to "Do not notify" (or changed between loud and quiet) no longer has its old channel.
                    reconcileChannels(NotificationsRules.with(before, address, mode))
                }
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                Log.w(Background.TAG, "setting the address choice failed", e)
                addresses.value = before
                messageChannel.send(R.string.notifications_address_failed)
            }
        }
    }

    /** The phone has no system screen for a channel: say so (rare, and it must not crash). */
    fun noChannelScreen() {
        messageChannel.trySend(R.string.notifications_channel_failed)
    }

    /** The intent that opens the system's settings for one of this account's channels, made first if need be. */
    fun channelSettings(kind: ChannelKind): Intent {
        val app = getApplication<Application>()
        val record = session.record
        val records = Accounts.records.value
        Channels.ensure(app)
        Channels.ensureAccount(app, record, AccountLabels.forNotice(record, records, Accounts.names.value))
        val id = when (kind) {
            ChannelKind.Mail -> ChannelRules.mail(record)
            ChannelKind.Reminders -> Channels.REMINDERS
        }
        return Channels.settingsIntent(app, id)
    }

    /** The intent that opens the system's settings for the channel of [choice] (the loud or the quiet one, made first if need be). */
    fun addressChannelSettings(choice: AddressChoice): Intent {
        val app = getApplication<Application>()
        val record = session.record
        val records = Accounts.records.value
        val key = NotificationsRules.key(choice.address)
        val quiet = choice.mode == AddressMode.SILENT
        Channels.ensureAddress(app, record, AccountLabels.forNotice(record, records, Accounts.names.value), key, quiet)
        return Channels.settingsIntent(app, ChannelRules.forMail(record, quiet, key))
    }

    /** Deletes the channels of addresses that no longer notify (or notify the other way), keeping any a notification is still in. */
    private fun reconcileChannels(choices: List<AddressChoice>) {
        val wanted = NotificationsRules.wantedChannels(session.record, choices)
        Channels.reconcileAddresses(getApplication(), session.record, wanted)
    }
}
