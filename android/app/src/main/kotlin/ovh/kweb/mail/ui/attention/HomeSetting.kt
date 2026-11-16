package ovh.kweb.mail.ui.attention

import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow

/**
 * Whether the Home tab is on for the account that is shown. The app reads it when an account is adopted (so the app
 * can open on Home) and the settings screen sets it when it is changed; the bottom bar and the back stack follow it.
 */
object HomeSetting {
    private val flow = MutableStateFlow(false)

    val enabled: StateFlow<Boolean> get() = flow

    fun set(on: Boolean) {
        flow.value = on
    }
}
