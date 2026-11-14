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
import ovh.kweb.mail.ui.list.MailListViewModel
import ovh.kweb.mail.ui.theme.KwebTheme
import ovh.kweb.mail.ui.theme.ThemeState

/** The one activity. Screens and navigation are in `ovh.kweb.mail.ui`. */
class MainActivity : ComponentActivity() {
    private val app: AppViewModel by viewModels()
    private val mailList: MailListViewModel by viewModels()

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
        lifecycleScope.launch(Dispatchers.IO) { Background.onAppForeground(appContext) }
    }

    /**
     * The Reply button of a mail notification: a new answer opens over the Inbox. Like a tap, it
     * takes the notification away and tells the core that was on purpose, once the draft exists.
     */
    private fun handleReplyIntent(intent: Intent): Boolean {
        val messageId = intent.getStringExtra(Notifier.EXTRA_REPLY_MESSAGE_ID) ?: return false
        val androidId = intent.getIntExtra(Notifier.EXTRA_REPLY_ANDROID_ID, 0)
        intent.removeExtra(Notifier.EXTRA_REPLY_MESSAGE_ID)
        intent.removeExtra(Notifier.EXTRA_REPLY_ANDROID_ID)
        mailList.showInbox()
        app.replyTo(messageId, androidId)
        return true
    }

    /** A tap on a mail notification names its message: it opens over the Inbox. */
    private fun handleNotificationIntent(intent: Intent) {
        if (handleReplyIntent(intent)) return
        val messageId = intent.getStringExtra(Notifier.EXTRA_MESSAGE_ID) ?: return
        intent.removeExtra(Notifier.EXTRA_MESSAGE_ID)
        mailList.showInbox()
        app.focusMessage(messageId)
        // The tap removed the notification while the mail is still unread. Tell the core it is
        // gone on purpose, like a swipe, or the next 15-minute check would put it back.
        val appContext = applicationContext
        lifecycleScope.launch(Dispatchers.IO) {
            runCatching { Background.core(appContext).notificationSwiped(messageId) }
        }
    }
}
