package ovh.kweb.mail

import android.app.Application
import android.util.Log
import java.io.File
import kotlin.concurrent.thread
import ovh.kweb.mail.background.Background
import ovh.kweb.mail.background.Channels
import ovh.kweb.mail.background.CheckScheduler
import ovh.kweb.mail.background.KeystoreSecrets
import uniffi.kweb_core.Core

/**
 * Runs before any activity, service or receiver of the process, which is what the
 * one-time set-up of the core needs.
 *
 * It also owns the one [Core] of the process; everyone else gets it through
 * [Background.core].
 */
class KwebApp : Application() {
    /**
     * Created on first use, once, whichever thread asks first. Opening the cache takes a moment, so
     * [onCreate] asks from a background thread and the main thread rarely pays for it.
     */
    val core: Core by lazy(LazyThreadSafetyMode.SYNCHRONIZED) {
        Core(File(filesDir, "cache.db").path, KeystoreSecrets(this))
    }

    override fun onCreate() {
        super.onCreate()
        Tls.init(this)
        Channels.ensure(this)
        thread(name = "kweb-warm-up") { warmUp() }
    }

    private fun warmUp() {
        try {
            Background.refreshStatus(this)
            if (core.account() != null) CheckScheduler.schedulePeriodic(this)
        } catch (e: Exception) {
            Log.e(Background.TAG, "warm-up failed", e)
        }
    }
}
