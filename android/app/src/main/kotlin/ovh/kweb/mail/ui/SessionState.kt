package ovh.kweb.mail.ui

import android.content.Context
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.withContext
import ovh.kweb.mail.background.Background

/**
 * Whether an account is signed in, as the screens see it. Null until the core has answered.
 *
 * It is a process-wide flow and not a field of one ViewModel so that a sign-in that finishes while
 * the activity is stopped (the browser is in front) has already moved the app on when it comes back.
 */
object SessionState {
    private val flow = MutableStateFlow<Boolean?>(null)

    val signedIn: StateFlow<Boolean?> get() = flow

    /** Asks the core once; later calls keep the answer the screens have set since. */
    suspend fun load(context: Context) {
        if (flow.value != null) return
        val signedIn = withContext(Dispatchers.IO) { Background.core(context).account() != null }
        flow.compareAndSet(null, signedIn)
    }

    fun set(signedIn: Boolean) {
        flow.value = signedIn
    }
}
