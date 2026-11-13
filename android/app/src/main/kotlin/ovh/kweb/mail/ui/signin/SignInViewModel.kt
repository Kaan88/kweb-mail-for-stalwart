package ovh.kweb.mail.ui.signin

import android.app.Application
import android.util.Log
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.SavedStateHandle
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.receiveAsFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
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
 * Runs the sign-in. `finishSignIn()` blocks for up to five minutes, so it lives in [viewModelScope]:
 * it goes on while the activity is stopped behind the browser, and a sign-in that completes after
 * the user pressed Cancel is still accepted (the core has no way to cancel it).
 */
class SignInViewModel(app: Application, private val saved: SavedStateHandle) : AndroidViewModel(app) {
    private val stateFlow = MutableStateFlow(SignInUiState(address = saved[ADDRESS] ?: ""))
    private val browserUrls = Channel<String>(Channel.BUFFERED)
    private val finished = Channel<Unit>(Channel.BUFFERED)

    /** Counts attempts; a result of an older one is ignored, except a success. */
    private var attempt = 0

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
        viewModelScope.launch { run(mine, url) }
    }

    /** Back to the form. A sign-in still waiting in the core is left to time out or succeed. */
    fun cancel() {
        attempt++
        stateFlow.update { it.copy(phase = SignInPhase.Form, error = null) }
    }

    /** No browser could be opened for the URL. */
    fun browserUnavailable() = fail(attempt, SignInError.NoBrowser)

    private suspend fun run(mine: Int, url: String) {
        val core = Background.core(getApplication())
        val authorizeUrl = try {
            withContext(Dispatchers.IO) { core.beginSignIn(url) }
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
        try {
            withContext(Dispatchers.IO) { core.finishSignIn() }
        } catch (e: CancellationException) {
            throw e
        } catch (e: CoreException) {
            return fail(mine, SignInError.fromFinish(e))
        } catch (e: Exception) {
            return fail(mine, SignInError.Other(e.message.orEmpty()))
        }
        // The account exists from here on, whatever the set-up of the background half does.
        try {
            withContext(Dispatchers.IO) { Background.afterSignIn(getApplication()) }
        } catch (e: CancellationException) {
            throw e
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
    }
}
