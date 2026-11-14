package ovh.kweb.mail.ui

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.key
import androidx.compose.runtime.getValue
import androidx.compose.ui.Modifier
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewmodel.compose.viewModel
import ovh.kweb.mail.ui.delivery.DeliveryScreen
import ovh.kweb.mail.ui.list.MailListScreen
import ovh.kweb.mail.ui.search.SearchScreen
import ovh.kweb.mail.ui.search.SearchViewModel
import ovh.kweb.mail.ui.thread.ThreadScreen
import ovh.kweb.mail.ui.signin.SignInScreen

/**
 * The whole app: the sign-in screen while there is no account, otherwise the top of the back
 * stack. System back goes back one screen, and from the mail list out of the app.
 */
@Composable
fun AppContent(app: AppViewModel = viewModel()) {
    val signedIn by SessionState.signedIn.collectAsStateWithLifecycle()
    val stack by app.stack.collectAsStateWithLifecycle()

    when (signedIn) {
        null -> Box(Modifier.fillMaxSize().background(MaterialTheme.colorScheme.background))
        false -> SignInScreen()
        true -> {
            BackHandler(enabled = stack.size > 1) { app.back() }
            // The search is forgotten once its screen is gone from the stack, not when a thread opens over it.
            val search: SearchViewModel = viewModel()
            val searchOpen = Route.Search in stack
            LaunchedEffect(searchOpen) { if (!searchOpen) search.clear() }
            when (val route = stack.last()) {
                Route.MailList -> MailListScreen(
                    onOpenThread = { threadId, mailboxId -> app.open(Route.Thread(threadId, mailboxId, null)) },
                    onOpenSearch = { app.open(Route.Search) },
                    onOpenDeliveryCheck = { app.open(Route.DeliveryCheck) },
                )
                // One thread replacing another (a notification tap) must not inherit the first one's state.
                is Route.Thread -> key(route) { ThreadScreen(route, onBack = app::back) }
                Route.Search -> SearchScreen(
                    onBack = app::back,
                    onOpen = { threadId, messageId -> app.open(Route.Thread(threadId, null, messageId)) },
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
