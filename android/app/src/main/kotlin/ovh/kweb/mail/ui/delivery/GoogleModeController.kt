package ovh.kweb.mail.ui.delivery

import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import ovh.kweb.mail.background.FallbackReason
import ovh.kweb.mail.background.SwitchOutcome

/** What the user is told after a switch. */
sealed interface GoogleModeResult {
    data object SwitchedOn : GoogleModeResult
    data object SwitchedOff : GoogleModeResult
    data class Failed(val reason: FallbackReason) : GoogleModeResult
    data object ConnectionNotUp : GoogleModeResult

    /** A switch is already under way; nothing was done. */
    data object Busy : GoogleModeResult
}

/**
 * The switch of one account between the permanent connection and push through Google, as the screens use it: one
 * switch at a time, and an answer to put into words. The sequences themselves are `GoogleSwitch`; here they are
 * two functions so a test uses fakes.
 */
class GoogleModeController(
    private val switchOn: suspend () -> SwitchOutcome,
    private val switchOff: suspend () -> SwitchOutcome,
) {
    private val busyFlow = MutableStateFlow(false)

    /** True while a switch runs (it can take a minute and a half while the server's verification is awaited). */
    val busy: StateFlow<Boolean> get() = busyFlow

    suspend fun turnOn(): GoogleModeResult = run(switchOn, GoogleModeResult.SwitchedOn)

    suspend fun turnOff(): GoogleModeResult = run(switchOff, GoogleModeResult.SwitchedOff)

    private suspend fun run(block: suspend () -> SwitchOutcome, success: GoogleModeResult): GoogleModeResult {
        if (!busyFlow.compareAndSet(false, true)) return GoogleModeResult.Busy
        try {
            return when (val outcome = block()) {
                is SwitchOutcome.Done -> success
                is SwitchOutcome.Failed -> GoogleModeResult.Failed(outcome.reason)
                is SwitchOutcome.ConnectionNotUp -> GoogleModeResult.ConnectionNotUp
                is SwitchOutcome.Busy -> GoogleModeResult.Busy
            }
        } finally {
            busyFlow.value = false
        }
    }
}
