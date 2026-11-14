package ovh.kweb.mail.ui.signin

import android.app.Activity
import android.content.Intent
import android.os.Bundle
import java.util.concurrent.atomic.AtomicLong
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
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
    private val counter = AtomicLong(0)

    /** The token of the sign-in that waits now; 0 when none does. */
    private val expecting = AtomicLong(0)
    private val interruptedFlow = MutableStateFlow(false)

    /**
     * True when the browser came back and nobody was waiting for it (the process was killed while the browser was
     * in front, so the sign-in it belongs to is gone). The URL is dropped, never handed to another sign-in. Whoever
     * tells the user calls [acknowledgeInterrupted].
     */
    val interrupted: StateFlow<Boolean> get() = interruptedFlow

    fun acknowledgeInterrupted() {
        interruptedFlow.value = false
    }

    fun deliver(callbackUrl: String) {
        if (expecting.get() != 0L) urls.trySend(callbackUrl) else interruptedFlow.value = true
    }

    /**
     * A sign-in is about to send the user to the browser: forgets an answer nobody collected, so it cannot be
     * taken for the answer to this attempt, and starts accepting one. Returns the token of this sign-in; give it
     * to [stopExpecting]. A newer sign-in takes over, and the older one ending later does not switch it off.
     */
    fun expect(): Long {
        while (urls.tryReceive().isSuccess) Unit
        val token = counter.incrementAndGet()
        expecting.set(token)
        return token
    }

    /** The sign-in of [token] is over (answered, cancelled or failed): an answer that arrives now has no owner, unless a newer sign-in waits. */
    fun stopExpecting(token: Long) {
        if (expecting.compareAndSet(token, 0L)) while (urls.tryReceive().isSuccess) Unit
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
