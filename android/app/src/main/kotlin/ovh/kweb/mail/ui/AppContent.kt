package ovh.kweb.mail.ui

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Modifier
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewmodel.compose.viewModel
import ovh.kweb.mail.ui.delivery.DeliveryScreen
import ovh.kweb.mail.ui.inbox.InboxScreen
import ovh.kweb.mail.ui.signin.SignInScreen

/**
 * The whole app: the sign-in screen while there is no account, otherwise the top of the back
 * stack. System back goes Delivery check, then Inbox, then out of the app.
 */
@Composable
fun AppContent(app: AppViewModel = viewModel()) {
    val signedIn by SessionState.signedIn.collectAsStateWithLifecycle()
    val stack by app.stack.collectAsStateWithLifecycle()
    val focus by app.focus.collectAsStateWithLifecycle()

    when (signedIn) {
        null -> Box(Modifier.fillMaxSize().background(MaterialTheme.colorScheme.background))
        false -> SignInScreen()
        true -> {
            BackHandler(enabled = stack.size > 1) { app.back() }
            when (stack.last()) {
                Route.Inbox -> InboxScreen(
                    focus = focus,
                    onFocusHandled = app::focusHandled,
                    onOpenDeliveryCheck = { app.open(Route.DeliveryCheck) },
                )
                Route.DeliveryCheck -> DeliveryScreen(
                    onBack = app::back,
                    onSignInAgain = { app.open(Route.SignInAgain) },
                )
                Route.SignInAgain -> SignInAgain(app)
            }
        }
    }
}

/** Signing in again keeps the account, so the server address is already known. */
@Composable
private fun SignInAgain(app: AppViewModel) {
    val serverUrl by app.serverUrl.collectAsStateWithLifecycle()
    SignInScreen(suggestedAddress = serverUrl, onBack = app::back, onCompleted = app::back)
}
