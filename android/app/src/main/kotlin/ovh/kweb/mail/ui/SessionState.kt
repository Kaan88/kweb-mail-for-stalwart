package ovh.kweb.mail.ui

import android.content.Context
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.distinctUntilChanged
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import ovh.kweb.mail.accounts.Accounts

/**
 * Whether an account is signed in, as the screens see it: the app has at least one account. Null until the
 * accounts have been read.
 *
 * It is a process-wide flow and not a field of one ViewModel so that a sign-in that finishes while
 * the activity is stopped (the browser is in front) has already moved the app on when it comes back.
 */
object SessionState {
    private val flow = MutableStateFlow<Boolean?>(null)
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)
    private var following = false

    val signedIn: StateFlow<Boolean?> get() = flow

    /** Reads the accounts once; from then on the answer follows them. Later calls change nothing. */
    suspend fun load(context: Context) {
        if (flow.value != null) return
        val any = withContext(Dispatchers.IO) { Accounts.load(context).isNotEmpty() }
        flow.compareAndSet(null, any)
        follow()
    }

    /** Whoever adds or removes an account says so right away; the follower below only keeps the two in step. */
    fun set(signedIn: Boolean) {
        flow.value = signedIn
    }

    private fun follow() {
        if (following) return
        following = true
        scope.launch { Accounts.records.map { it.isNotEmpty() }.distinctUntilChanged().collect { flow.value = it } }
    }
}
