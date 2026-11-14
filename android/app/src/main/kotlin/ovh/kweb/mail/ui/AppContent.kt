package ovh.kweb.mail.ui

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.key
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalResources
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewmodel.compose.viewModel
import ovh.kweb.mail.ui.compose.ComposeScreen
import ovh.kweb.mail.ui.delivery.DeliveryScreen
import ovh.kweb.mail.ui.list.MailListScreen
import ovh.kweb.mail.ui.search.SearchScreen
import ovh.kweb.mail.ui.search.SearchViewModel
import ovh.kweb.mail.ui.settings.SettingsScreen
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
        true -> Box(Modifier.fillMaxSize()) {
            BackHandler(enabled = stack.size > 1) { app.back() }
            // The search is forgotten once its screen is gone from the stack, not when a thread opens over it.
            val search: SearchViewModel = viewModel()
            val searchOpen = Route.Search in stack
            LaunchedEffect(searchOpen) { if (!searchOpen) search.clear() }
            val snackbar = remember { SnackbarHostState() }
            val resources = LocalResources.current
            LaunchedEffect(app) {
                app.messages.collect { snackbar.showSnackbar(resources.getString(it)) }
            }
            when (val route = stack.last()) {
                Route.MailList -> MailListScreen(
                    onOpenThread = { threadId, mailboxId -> app.open(Route.Thread(threadId, mailboxId, null)) },
                    onOpenSearch = { app.open(Route.Search) },
                    onOpenDeliveryCheck = { app.open(Route.DeliveryCheck) },
                    onOpenSettings = { app.open(Route.Settings) },
                    onCompose = { app.open(Route.Compose(it)) },
                )
                // One thread replacing another (a notification tap) must not inherit the first one's state.
                is Route.Thread -> key(route) {
                    ThreadScreen(route, onBack = app::back, onCompose = { app.open(Route.Compose(it)) })
                }
                is Route.Compose -> key(route) { ComposeScreen(route.draftId, onLeave = app::back, onSent = app::sent) }
                Route.Settings -> SettingsScreen(
                    onBack = app::back,
                    onOpenDeliveryCheck = { app.open(Route.DeliveryCheck) },
                )
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
            SnackbarHost(snackbar, Modifier.align(Alignment.BottomCenter).navigationBarsPadding())
        }
    }
}

/** Signing in again keeps the account, so the server address is already known. */
@Composable
private fun SignInAgain(app: AppViewModel) {
    val serverUrl by app.serverUrl.collectAsStateWithLifecycle()
    SignInScreen(suggestedAddress = serverUrl, onBack = app::back, onCompleted = app::back)
}
