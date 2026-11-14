package ovh.kweb.mail.ui.signin

import android.app.Application
import android.util.Log
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.NonCancellable
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.receiveAsFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import kotlinx.coroutines.withTimeoutOrNull
import ovh.kweb.mail.accounts.AccountAlreadyAdded
import ovh.kweb.mail.accounts.Accounts
import ovh.kweb.mail.background.Background
import ovh.kweb.mail.ui.SessionState
import uniffi.kweb_core.Core
import uniffi.kweb_core.Exception as CoreException

enum class SignInPhase { Form, Starting, Waiting }

data class SignInUiState(
    val address: String = "",
    val phase: SignInPhase = SignInPhase.Form,
    val error: SignInError? = null,
)

/** What a sign-in is for: a new account, or the sign-in again of one the app already has. */
sealed interface SignInTarget {
    /** The key that keeps the view models of two targets apart. */
    val key: String

    data object New : SignInTarget {
        override val key: String get() = "signin:new"
    }

    /** The account [accountId] (its id in the registry) needs a new grant. */
    data class Again(val accountId: String) : SignInTarget {
        override val key: String get() = "signin:again:$accountId"
    }
}

/**
 * Runs the sign-in: asks the core for the page to open, waits for the browser to come back
 * through [SignInReturn] (up to five minutes) and hands the answer to the core. Cancel ends the
 * wait; once the answer is with the core, the sign-in is seen through.
 *
 * A new account is signed in on its own candidate core (its own cache file and secrets), so a refresh token
 * can never overwrite another account's; the candidate joins the list only when the sign-in is complete and
 * is not a duplicate. Cancel, failure and timeout discard it.
 */
class SignInViewModel(
    app: Application,
    private val target: SignInTarget = SignInTarget.New,
) : AndroidViewModel(app) {
    private val stateFlow = MutableStateFlow(
        SignInUiState(
            address = "",
            error = if (SignInReturn.interrupted.value) SignInError.Interrupted else null,
        ),
    )
    private val browserUrls = Channel<String>(Channel.BUFFERED)
    private val finished = Channel<Unit>(Channel.BUFFERED)

    /** Counts attempts; a result of an older one is ignored, except a success. */
    private var attempt = 0
    private var job: Job? = null

    val state: StateFlow<SignInUiState> get() = stateFlow

    /** The authorize URLs to open in the browser, one per attempt. */
    val urlsToOpen: Flow<String> get() = browserUrls.receiveAsFlow()

    /** One item per sign-in that completed, for a screen that has to leave itself (signing in again). */
    val completed: Flow<Unit> get() = finished.receiveAsFlow()

    init {
        // The message is shown once, here; a signed-in app shows it as a snackbar instead (see AppViewModel).
        if (SignInReturn.interrupted.value) SignInReturn.acknowledgeInterrupted()
    }

    /** Fills the field with [suggestion] when the user has typed nothing yet. */
    fun suggest(suggestion: String?) {
        if (suggestion != null && stateFlow.value.address.isBlank()) setAddress(suggestion)
    }

    fun setAddress(address: String) {
        stateFlow.update { it.copy(address = address, error = null) }
    }

    fun signIn() {
        val current = stateFlow.value
        if (current.phase != SignInPhase.Form) return
        val url = ServerAddress.normalise(current.address)
            ?: return stateFlow.update { it.copy(error = SignInError.InvalidAddress) }
        val mine = ++attempt
        stateFlow.update { it.copy(phase = SignInPhase.Starting, error = null) }
        job = viewModelScope.launch { run(mine, url) }
    }

    /** Back to the form. The answer of a browser that comes back later is not used. */
    fun cancel() {
        attempt++
        job?.cancel()
        stateFlow.update { it.copy(phase = SignInPhase.Form, error = null) }
    }

    /** No browser could be opened for the URL. */
    fun browserUnavailable() {
        // The browser will not come back: end the wait and throw the candidate away.
        fail(attempt, SignInError.NoBrowser)
        job?.cancel()
    }

    private suspend fun run(mine: Int, url: String) {
        var candidate: Accounts.Candidate? = null
        var done = false
        var token = 0L
        try {
            val core: Core
            try {
                token = SignInReturn.expect()
                val prepared = withContext(Dispatchers.IO) {
                    when (target) {
                        SignInTarget.New -> Accounts.load(getApplication()).let { Accounts.beginAdd() }.let { it to it.core }
                        is SignInTarget.Again ->
                            null to (Accounts.session(target.accountId)?.core ?: error("the account is gone"))
                    }
                }
                candidate = prepared.first
                core = prepared.second
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                return fail(mine, SignInError.Other(e.message.orEmpty()))
            }
            val authorizeUrl = try {
                withContext(Dispatchers.IO) { core.beginSignInWithRedirect(url, SignInReturn.REDIRECT_URI) }
            } catch (e: CancellationException) {
                throw e
            } catch (e: CoreException) {
                return fail(mine, SignInError.fromBegin(e))
            } catch (e: Exception) {
                return fail(mine, SignInError.Other(e.message.orEmpty()))
            }
            if (mine != attempt) return
            stateFlow.update { it.copy(phase = SignInPhase.Waiting) }
            browserUrls.send(authorizeUrl)
            val callbackUrl = withTimeoutOrNull(WAIT_MILLIS) { SignInReturn.await() }
                ?: return fail(mine, SignInError.NoBrowserAnswer)
            // From here on a Cancel comes too late: the answer is used up either way.
            done = withContext(NonCancellable) { complete(mine, core, candidate, callbackUrl) }
        } finally {
            if (token != 0L) SignInReturn.stopExpecting(token)
            val unfinished = candidate
            if (!done && unfinished != null) {
                withContext(NonCancellable + Dispatchers.IO) { Accounts.abandon(unfinished) }
            }
        }
    }

    /** Returns true when the sign-in is complete (the account is in the list). */
    private suspend fun complete(mine: Int, core: Core, candidate: Accounts.Candidate?, callbackUrl: String): Boolean {
        try {
            withContext(Dispatchers.IO) {
                core.completeSignIn(callbackUrl)
                if (candidate != null) Accounts.finishAdd(candidate) else Accounts.refreshRecord((target as SignInTarget.Again).accountId)
            }
        } catch (e: AccountAlreadyAdded) {
            // Signing in again as a user the app has under another entry removes this entry, and with it this screen.
            if (candidate == null) SignInNotices.duplicateRemoved.trySend(Unit)
            fail(mine, SignInError.AlreadyAdded)
            return false
        } catch (e: CoreException) {
            fail(mine, SignInError.fromFinish(e))
            return false
        } catch (e: Exception) {
            fail(mine, SignInError.Other(e.message.orEmpty()))
            return false
        }
        // The account exists from here on, whatever the set-up of the background half does.
        try {
            withContext(Dispatchers.IO) { Background.afterAccountAdded(getApplication()) }
        } catch (e: Exception) {
            Log.e(Background.TAG, "set-up after sign-in failed", e)
        }
        SessionState.set(true)
        stateFlow.update { it.copy(phase = SignInPhase.Form, error = null) }
        finished.send(Unit)
        return true
    }

    private fun fail(mine: Int, error: SignInError) {
        if (mine != attempt) return
        stateFlow.update { it.copy(phase = SignInPhase.Form, error = error) }
    }

    private companion object {
        const val WAIT_MILLIS = 5 * 60 * 1000L
    }
}
