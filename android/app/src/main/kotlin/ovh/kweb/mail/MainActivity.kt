package ovh.kweb.mail

import android.content.Intent
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.viewModels
import androidx.lifecycle.lifecycleScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import ovh.kweb.mail.background.Background
import ovh.kweb.mail.background.Notifier
import ovh.kweb.mail.ui.AppContent
import ovh.kweb.mail.ui.AppViewModel
import ovh.kweb.mail.ui.sending.IntentShare
import ovh.kweb.mail.ui.theme.KwebTheme
import ovh.kweb.mail.ui.theme.ThemeState

/** The one activity. Screens and navigation are in `ovh.kweb.mail.ui`. */
class MainActivity : ComponentActivity() {
    private val app: AppViewModel by viewModels()

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        ThemeState.load(this)
        setContent { KwebTheme { AppContent(app) } }
        // After a recreation (rotation) or a relaunch from Recents the intent is the one that was handled already.
        if (savedInstanceState == null && intent.flags and Intent.FLAG_ACTIVITY_LAUNCHED_FROM_HISTORY == 0) {
            handleNotificationIntent(intent)
        }
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        setIntent(intent)
        handleNotificationIntent(intent)
    }

    override fun onStart() {
        super.onStart()
        // Brings the service back if Android killed it. Off the main thread because the first use
        // of the core opens the cache; the app is still in the foreground, which is what counts.
        val appContext = applicationContext
        lifecycleScope.launch(Dispatchers.IO) {
            try {
                Background.onAppForeground(appContext)
            } catch (e: kotlin.coroutines.cancellation.CancellationException) {
                throw e
            } catch (e: Exception) {
                android.util.Log.e(Background.TAG, "the foreground set-up failed", e)
            }
        }
    }

    /**
     * The Reply button of a mail notification: a new answer opens over the Inbox. Like a tap, it
     * takes the notification away and tells the core that was on purpose, once the draft exists.
     */
    private fun handleReplyIntent(intent: Intent): Boolean {
        val messageId = intent.getStringExtra(Notifier.EXTRA_REPLY_MESSAGE_ID) ?: return false
        val androidId = intent.getIntExtra(Notifier.EXTRA_REPLY_ANDROID_ID, 0)
        val account = intent.takeAccount()
        intent.removeExtra(Notifier.EXTRA_REPLY_MESSAGE_ID)
        intent.removeExtra(Notifier.EXTRA_REPLY_ANDROID_ID)
        app.replyTo(account, messageId, androidId)
        return true
    }

    /**
     * The account a notification intent is for (null when it names none: the account that was there before
     * several existed), taken off the intent so that a recreation does not use it twice. [AppViewModel] resolves
     * it, switches to it, and drops the intent when the account is gone.
     */
    private fun Intent.takeAccount(): String? = getStringExtra(Notifier.EXTRA_ACCOUNT).also { removeExtra(Notifier.EXTRA_ACCOUNT) }

    /** A tap on the reminder of an event or task opens that entry. */
    private fun handleReminderIntent(intent: Intent): Boolean {
        val eventId = intent.getStringExtra(Notifier.EXTRA_EVENT_ID) ?: return false
        val recurrenceId = intent.getStringExtra(Notifier.EXTRA_RECURRENCE_ID)
        val androidId = intent.getIntExtra(Notifier.EXTRA_REMINDER_ANDROID_ID, 0)
        val account = intent.takeAccount()
        intent.removeExtra(Notifier.EXTRA_EVENT_ID)
        intent.removeExtra(Notifier.EXTRA_RECURRENCE_ID)
        intent.removeExtra(Notifier.EXTRA_REMINDER_ANDROID_ID)
        app.openReminder(account, eventId, recurrenceId, androidId)
        return true
    }

    /**
     * Another app shared text or files into this one (ACTION_SEND, ACTION_SEND_MULTIPLE). The action is taken off
     * the intent so that a recreation does not start the message a second time.
     */
    private fun handleShareIntent(intent: Intent): Boolean {
        val raw = IntentShare.read(intent) ?: return false
        intent.action = null
        app.share(raw)
        return true
    }

    /** A tap on a mail notification names its message: it opens over the Inbox. */
    private fun handleNotificationIntent(intent: Intent) {
        if (handleShareIntent(intent)) return
        if (handleReplyIntent(intent) || handleReminderIntent(intent)) return
        val messageId = intent.getStringExtra(Notifier.EXTRA_MESSAGE_ID)
        if (messageId == null) {
            // The tap on a group summary names only the account whose mail it counts.
            intent.takeAccount()?.let { app.showAccount(it) }
            return
        }
        val account = intent.takeAccount()
        intent.removeExtra(Notifier.EXTRA_MESSAGE_ID)
        // The view model also tells the core the notification is gone on purpose (like a swipe), or the next
        // 15-minute check would put it back.
        app.focusMessage(account, messageId)
    }

}
