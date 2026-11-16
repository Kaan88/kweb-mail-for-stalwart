package ovh.kweb.mail

import android.app.Application
import android.util.Log
import kotlin.concurrent.thread
import ovh.kweb.mail.accounts.Accounts
import ovh.kweb.mail.background.Background
import ovh.kweb.mail.background.Channels
import ovh.kweb.mail.background.CheckScheduler
import ovh.kweb.mail.background.GooglePush

/**
 * Runs before any activity, service or receiver of the process, which is what the
 * one-time set-up needs. The accounts and their cores are made on first use by
 * [Accounts]; the screens get the one of their account from `AccountSession`.
 */
class KwebApp : Application() {
    override fun onCreate() {
        super.onCreate()
        Accounts.attach(this)
        AppVisibility.register(this)
        Tls.init(this)
        Channels.ensure(this)
        // Opening the accounts takes a moment (the first start after an update also migrates), so the main thread does not wait.
        thread(name = "kweb-warm-up") { warmUp() }
    }

    private fun warmUp() {
        try {
            val accounts = Accounts.load(this)
            if (accounts.isEmpty()) return
            // Files of an opened encrypted message are only kept while its screen is open; a crash or kill may have left some.
            for (account in accounts) ovh.kweb.mail.ui.crypto.CryptoTemp.clear(this, account.id)
            // Part files of a download that a dead process left; there is no resume.
            ovh.kweb.mail.ui.transfer.TransferHub.sweepParts(cacheDir)
            Accounts.sessions()
            Background.refreshStatus(this)
            CheckScheduler.schedulePeriodic(this)
            GooglePush.onAppStart(this)
        } catch (e: Exception) {
            Log.e(Background.TAG, "warm-up failed", e)
        }
    }
}
