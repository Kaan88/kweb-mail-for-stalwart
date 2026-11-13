package ovh.kweb.mail.ui.signin

import android.app.Application
import android.util.Log
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.SavedStateHandle
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
import ovh.kweb.mail.background.Background
import ovh.kweb.mail.ui.SessionState
import uniffi.kweb_core.Exception as CoreException

enum class SignInPhase { Form, Starting, Waiting }

data class SignInUiState(
    val address: String = "",
    val phase: SignInPhase = SignInPhase.Form,
    val error: SignInError? = null,
)

/**
 * Runs the sign-in: asks the core for the page to open, waits for the browser to come back
 * through [SignInReturn] (up to five minutes) and hands the answer to the core. Cancel ends the
 * wait; once the answer is with the core, the sign-in is seen through.
 */
class SignInViewModel(app: Application, private val saved: SavedStateHandle) : AndroidViewModel(app) {
    private val stateFlow = MutableStateFlow(SignInUiState(address = saved[ADDRESS] ?: ""))
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

    /** Fills the field with [suggestion] when the user has typed nothing yet. */
    fun suggest(suggestion: String?) {
        if (suggestion != null && stateFlow.value.address.isBlank()) setAddress(suggestion)
    }

    fun setAddress(address: String) {
        saved[ADDRESS] = address
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
    fun browserUnavailable() = fail(attempt, SignInError.NoBrowser)

    private suspend fun run(mine: Int, url: String) {
        val core = Background.core(getApplication())
        val authorizeUrl = try {
            SignInReturn.clear()
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
        withContext(NonCancellable) { complete(mine, callbackUrl) }
    }

    private suspend fun complete(mine: Int, callbackUrl: String) {
        val core = Background.core(getApplication())
        try {
            withContext(Dispatchers.IO) { core.completeSignIn(callbackUrl) }
        } catch (e: CoreException) {
            return fail(mine, SignInError.fromFinish(e))
        } catch (e: Exception) {
            return fail(mine, SignInError.Other(e.message.orEmpty()))
        }
        // The account exists from here on, whatever the set-up of the background half does.
        try {
            withContext(Dispatchers.IO) { Background.afterSignIn(getApplication()) }
        } catch (e: Exception) {
            Log.e(Background.TAG, "set-up after sign-in failed", e)
        }
        SessionState.set(true)
        stateFlow.update { it.copy(phase = SignInPhase.Form, error = null) }
        finished.send(Unit)
    }

    private fun fail(mine: Int, error: SignInError) {
        if (mine != attempt) return
        stateFlow.update { it.copy(phase = SignInPhase.Form, error = error) }
    }

    private companion object {
        const val ADDRESS = "address"
        const val WAIT_MILLIS = 5 * 60 * 1000L
    }
}
