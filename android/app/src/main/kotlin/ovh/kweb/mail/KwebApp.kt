package ovh.kweb.mail

import android.app.Application

/**
 * Runs before any activity, service or receiver of the process, which is what the
 * one-time set-up of the core needs.
 */
class KwebApp : Application() {
    override fun onCreate() {
        super.onCreate()
        Tls.init(this)
    }
}
