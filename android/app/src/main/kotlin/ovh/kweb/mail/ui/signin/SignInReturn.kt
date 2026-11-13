package ovh.kweb.mail.ui.signin

import android.app.Activity
import android.content.Intent
import android.os.Bundle
import kotlinx.coroutines.channels.Channel
import ovh.kweb.mail.MainActivity

/**
 * The way back from the browser. The server sends the browser to [REDIRECT_URI], a link only
 * this app opens; Android starts [SignInReturnActivity] with it, which leaves the full URL here
 * for the sign-in that is waiting.
 *
 * A socket on 127.0.0.1 (what the desktop uses) does not work on a phone: Android may freeze
 * the app's process while the browser is in front, and then nobody answers the browser.
 */
object SignInReturn {
    /** Must match the scheme of [SignInReturnActivity]'s intent filter in the manifest. */
    const val REDIRECT_URI = "ovh.kweb.mail:/oauth/callback"

    private val urls = Channel<String>(Channel.CONFLATED)

    fun deliver(callbackUrl: String) {
        urls.trySend(callbackUrl)
    }

    /** Forgets an answer nobody collected, so it cannot be taken for the answer to a new attempt. */
    fun clear() {
        while (urls.tryReceive().isSuccess) Unit
    }

    /** Suspends until the browser comes back. */
    suspend fun await(): String = urls.receive()
}

/**
 * Receives [SignInReturn.REDIRECT_URI] from the browser. It shows nothing: it passes the URL on
 * and brings [MainActivity] to the front, which also closes the browser tab lying over it.
 */
class SignInReturnActivity : Activity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        intent?.data?.let { SignInReturn.deliver(it.toString()) }
        startActivity(
            Intent(this, MainActivity::class.java)
                .addFlags(Intent.FLAG_ACTIVITY_CLEAR_TOP or Intent.FLAG_ACTIVITY_SINGLE_TOP),
        )
        finish()
    }
}
