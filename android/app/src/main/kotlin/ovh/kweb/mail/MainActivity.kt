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
import ovh.kweb.mail.ui.theme.KwebTheme

/** The one activity. Screens and navigation are in `ovh.kweb.mail.ui`. */
class MainActivity : ComponentActivity() {
    private val app: AppViewModel by viewModels()

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        setContent { KwebTheme { AppContent(app) } }
        // After a recreation (rotation) the intent is the one that was handled already.
        if (savedInstanceState == null) handleNotificationIntent(intent)
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

    /** A tap on a mail notification names its message; the Inbox scrolls to it but does not mark it read. */
    private fun handleNotificationIntent(intent: Intent) {
        val messageId = intent.getStringExtra(Notifier.EXTRA_MESSAGE_ID) ?: return
        intent.removeExtra(Notifier.EXTRA_MESSAGE_ID)
        app.focusMessage(messageId)
        // The tap removed the notification while the mail is still unread. Tell the core it is
        // gone on purpose, like a swipe, or the next 15-minute check would put it back.
        val appContext = applicationContext
        lifecycleScope.launch(Dispatchers.IO) {
            runCatching { Background.core(appContext).notificationSwiped(messageId) }
        }
    }
}
