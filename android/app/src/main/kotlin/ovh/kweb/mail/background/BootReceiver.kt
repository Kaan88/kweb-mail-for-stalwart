package ovh.kweb.mail.background

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.util.Log
import kotlin.concurrent.thread
import ovh.kweb.mail.accounts.Accounts

/**
 * After a restart of the phone or an update of the app: starts the service and the checks.
 *
 * After a boot the notification shade is empty, so the check is told so (`Shade.Cleared`) and
 * puts the unread mail back silently. After an update the shade is intact; that check reads the
 * real shade instead.
 */
class BootReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        val action = intent.action
        if (action != Intent.ACTION_BOOT_COMPLETED && action != Intent.ACTION_MY_PACKAGE_REPLACED) return
        val app = context.applicationContext
        val pending = goAsync()
        thread(name = "kweb-boot") {
            try {
                if (Accounts.load(app).isNotEmpty()) {
                    PushService.start(app)
                    CheckScheduler.schedulePeriodic(app)
                    CheckScheduler.checkSoon(app, shadeCleared = action == Intent.ACTION_BOOT_COMPLETED)
                }
            } catch (e: Exception) {
                Log.e(Background.TAG, "restart after $action failed", e)
            } finally {
                pending.finish()
            }
        }
    }
}
