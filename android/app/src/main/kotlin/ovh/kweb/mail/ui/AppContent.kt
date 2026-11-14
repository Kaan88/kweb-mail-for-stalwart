package ovh.kweb.mail.ui

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.ui.unit.dp
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.key
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalResources
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewmodel.compose.LocalViewModelStoreOwner
import kotlinx.coroutines.flow.distinctUntilChanged
import kotlinx.coroutines.flow.map
import androidx.lifecycle.viewmodel.compose.viewModel
import ovh.kweb.mail.ui.assistant.AssistantScreen
import ovh.kweb.mail.ui.away.AwayScreen
import ovh.kweb.mail.ui.calendar.CalendarScreen
import ovh.kweb.mail.ui.calendar.EditEventScreen
import ovh.kweb.mail.ui.calendar.EventScreen
import ovh.kweb.mail.ui.compose.ComposeScreen
import ovh.kweb.mail.ui.contacts.ContactScreen
import ovh.kweb.mail.ui.contacts.ContactsScreen
import ovh.kweb.mail.ui.contacts.EditContactScreen
import ovh.kweb.mail.ui.delivery.DeliveryScreen
import ovh.kweb.mail.ui.files.FilesScreen
import ovh.kweb.mail.ui.account.MaskedScreen
import ovh.kweb.mail.ui.account.SecurityScreen
import ovh.kweb.mail.ui.filters.FiltersScreen
import ovh.kweb.mail.ui.folders.FoldersScreen
import ovh.kweb.mail.ui.list.MailListScreen
import ovh.kweb.mail.ui.list.MailListViewModel
import ovh.kweb.mail.ui.mailextras.LabelScreen
import ovh.kweb.mail.ui.search.SearchScreen
import ovh.kweb.mail.ui.search.SearchViewModel
import ovh.kweb.mail.ui.sending.ScheduledScreen
import ovh.kweb.mail.ui.sending.SendersScreen
import ovh.kweb.mail.ui.settings.SettingsScreen
import ovh.kweb.mail.ui.shared.SharedCalendarScreen
import ovh.kweb.mail.ui.shared.SharedContactScreen
import ovh.kweb.mail.ui.shared.SharedContactsScreen
import ovh.kweb.mail.ui.shared.SharedEventScreen
import ovh.kweb.mail.ui.shared.SharedFolderScreen
import ovh.kweb.mail.ui.shared.SharedFoldersScreen
import ovh.kweb.mail.ui.shared.SharedMessageScreen
import ovh.kweb.mail.ui.shared.SharedScreen
import ovh.kweb.mail.ui.sharing.LocalOpenShare
import ovh.kweb.mail.ui.sharing.ShareScreen
import ovh.kweb.mail.ui.thread.ThreadScreen
import android.app.Application
import androidx.compose.material3.SnackbarDuration
import androidx.compose.material3.SnackbarResult
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import ovh.kweb.mail.R
import ovh.kweb.mail.ui.accounts.AccountSheet
import ovh.kweb.mail.ui.accounts.AccountStoreOwner
import ovh.kweb.mail.ui.accounts.AccountSwitcherViewModel
import ovh.kweb.mail.ui.accounts.AccountsUi
import ovh.kweb.mail.ui.accounts.LocalAccountSession
import ovh.kweb.mail.ui.accounts.LocalAccountsUi
import ovh.kweb.mail.ui.signin.SignInScreen
import ovh.kweb.mail.ui.signin.SignInTarget

/**
 * The whole app: the sign-in screen while there is no account, otherwise the top of the back
 * stack. System back goes back one screen, and from the mail list out of the app.
 */
@Composable
fun AppContent(app: AppViewModel = viewModel()) {
    val signedIn by SessionState.signedIn.collectAsStateWithLifecycle()
    val session by app.session.collectAsStateWithLifecycle()
    val switcher: AccountSwitcherViewModel = viewModel()
    val rows by switcher.rows.collectAsStateWithLifecycle()
    val application = LocalContext.current.applicationContext as Application

    when (signedIn) {
        null -> Box(Modifier.fillMaxSize().background(MaterialTheme.colorScheme.background))
        false -> SignInScreen()
        // The screens of one account: its own view models (they hold its session), dropped on a switch.
        true -> {
          // The snackbar and its collectors are not inside the key of the account: a message sent right after a switch must reach the screen.
          val snackbar = remember { SnackbarHostState() }
          val resources = LocalResources.current
          LaunchedEffect(app) { app.messages.collect { snackbar.showSnackbar(resources.getString(it)) } }
          LaunchedEffect(app) { app.notices.collect { snackbar.showSnackbar(it.resolve(resources)) } }
          // Something arrived for another account while a form is open: offer the switch instead of dropping the form.
          LaunchedEffect(app) {
              app.offers.collect { offer ->
                  val result = snackbar.showSnackbar(
                      resources.getString(R.string.accounts_this_is_for, offer.label),
                      actionLabel = resources.getString(R.string.accounts_switch),
                      duration = SnackbarDuration.Long,
                  )
                  if (result == SnackbarResult.ActionPerformed) offer.accept()
              }
          }
          session.let { current ->
            if (current == null) {
                BrokenAccount(app, snackbar)
            } else {
                key(current.record.id) {
                    val owner = remember(current.record.id) { AccountStoreOwner(application, current, app.accountStore) }
                    val ui = remember(rows) { AccountsUi(rows, app::selectAccount) { app.open(Route.AddAccount) } }
                    CompositionLocalProvider(
                        LocalAccountSession provides current,
                        LocalViewModelStoreOwner provides owner,
                        LocalAccountsUi provides ui,
                    ) { SignedInContent(app, snackbar) }
                }
            }
          }
        }
    }
}

/** The active account's cache cannot be opened and no other account can take its place: the one thing to do is to remove it. */
@Composable
private fun BrokenAccount(app: AppViewModel, snackbar: SnackbarHostState) {
    Box(Modifier.fillMaxSize().background(MaterialTheme.colorScheme.background)) {
        androidx.compose.foundation.layout.Column(
            Modifier.align(Alignment.Center).padding(24.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
        ) {
            androidx.compose.material3.Text(stringResource(R.string.accounts_broken_title), style = MaterialTheme.typography.titleMedium)
            androidx.compose.material3.Button(onClick = app::removeBrokenAccount, modifier = Modifier.padding(top = 16.dp)) {
                androidx.compose.material3.Text(stringResource(R.string.accounts_broken_remove))
            }
        }
        SnackbarHost(snackbar, Modifier.align(Alignment.BottomCenter).navigationBarsPadding())
    }
}

/** The signed-in screens: the top of the back stack, with the snackbars and the sheets that belong to the whole app. */
@Composable
private fun SignedInContent(app: AppViewModel, snackbar: SnackbarHostState) {
    val stack by app.stack.collectAsStateWithLifecycle()
    Box(Modifier.fillMaxSize()) {
            BackHandler(enabled = stack.size > 1) { app.back() }
            // The search is forgotten once its screen is gone from the stack, not when a thread opens over it.
            val search: SearchViewModel = viewModel()
            val searchOpen = Route.Search in stack
            LaunchedEffect(searchOpen) { if (!searchOpen) search.clear() }
            val accounts = LocalAccountsUi.current
            val account = LocalAccountSession.current.record.id
            // A notification tap named a folder of this account: the list shows it.
            val command by app.listCommand.collectAsStateWithLifecycle()
            // The Inbox's unread count, the badge on Mail; read from the list's own state, so both sides agree.
            val mailList: MailListViewModel = viewModel()
            val unread by remember(mailList) {
                mailList.state.map { s -> s.mailboxes.firstOrNull { it.role == "inbox" }?.unreadEmails?.toInt() ?: 0 }.distinctUntilChanged()
            }.collectAsStateWithLifecycle(0)
            LaunchedEffect(mailList, command) {
                val c = command ?: return@LaunchedEffect
                if (c.accountId != account) return@LaunchedEffect
                mailList.selectFolder(c.folderId)
                app.listCommandDone(c)
            }
            // A share names no account: with several, say which one it is sent from.
            val waitingShare by app.pendingShare.collectAsStateWithLifecycle()
            if (waitingShare != null) {
                AccountSheet(
                    title = stringResource(R.string.accounts_send_from),
                    onDismiss = app::cancelShare,
                    ui = AccountsUi(accounts.rows, app::shareFrom) {
                        app.cancelShare()
                        app.open(Route.AddAccount)
                    },
                )
            }
            val bottomBar: @Composable () -> Unit = { MainNavBar(stack.last(), unread, app::switchTo) }
            val openShare = remember(app) { { kind: uniffi.kweb_core.ShareKind, id: String, name: String -> app.open(Route.Share(kind, id, name)) } }
            CompositionLocalProvider(LocalOpenShare provides openShare) {
            when (val route = stack.last()) {
                Route.MailList -> MailListScreen(
                    onOpenThread = { threadId, mailboxId -> app.open(Route.Thread(threadId, mailboxId, null)) },
                    onOpenSearch = { app.open(Route.Search) },
                    onOpenDeliveryCheck = { app.open(Route.DeliveryCheck) },
                    onOpenSettings = { app.open(Route.Settings) },
                    onCompose = { app.open(Route.Compose(it)) },
                    onOpenShared = { app.open(Route.Shared) },
                    onOpenFolders = { folderId, action -> app.open(Route.Folders(folderId, action)) },
                    onOpenLabel = { id, name -> app.open(Route.Label(id, name)) },
                    bottomBar = bottomBar,
                    appSnackbar = snackbar,
                )
                Route.Calendar -> CalendarScreen(
                    onOpenEvent = { eventId, recurrenceId -> app.open(Route.Event(eventId, recurrenceId)) },
                    onAddEvent = { app.open(Route.EditEvent(null, it)) },
                    bottomBar = bottomBar,
                    appSnackbar = snackbar,
                )
                Route.Contacts -> ContactsScreen(
                    onOpenContact = { app.open(Route.Contact(it)) },
                    onAddContact = { app.open(Route.EditContact(null)) },
                    bottomBar = bottomBar,
                    appSnackbar = snackbar,
                )
                Route.Files -> FilesScreen(bottomBar = bottomBar, appSnackbar = snackbar)
                // These keep their state in a store of their own that goes when the route does (see ScreenStores).
                is Route.Event -> key(route) {
                    CompositionLocalProvider(LocalViewModelStoreOwner provides app.stores.owner(route)) {
                        EventScreen(route.eventId, route.recurrenceId, onBack = app::back, onEdit = { app.open(Route.EditEvent(it, null)) })
                    }
                }
                is Route.EditEvent -> key(route) {
                    CompositionLocalProvider(LocalViewModelStoreOwner provides app.stores.owner(route)) {
                        EditEventScreen(route.eventId, route.startHint, onBack = app::back, onSaved = app::eventSaved, account = route.account)
                    }
                }
                is Route.Contact -> key(route) {
                    CompositionLocalProvider(LocalViewModelStoreOwner provides app.stores.owner(route)) {
                        ContactScreen(
                            route.contactId,
                            onBack = app::back,
                            onEdit = { app.open(Route.EditContact(it)) },
                            onWrite = { id, to -> app.open(Route.Compose(id, to)) },
                            onOpenMail = { threadId, messageId -> app.open(Route.Thread(threadId, null, messageId)) },
                        )
                    }
                }
                is Route.EditContact -> key(route) {
                    CompositionLocalProvider(LocalViewModelStoreOwner provides app.stores.owner(route)) {
                        EditContactScreen(route.contactId, onBack = app::back, onSaved = app::contactSaved, account = route.account)
                    }
                }
                // The shared screens: one store per route, and the account is part of every route.
                Route.Shared -> key(route) {
                    CompositionLocalProvider(LocalViewModelStoreOwner provides app.stores.owner(route)) {
                        SharedScreen(
                            onBack = app::back,
                            onOpenMail = { app.open(Route.SharedFolders(it.accountId, it.name)) },
                            onOpenCalendar = { app.open(Route.SharedCalendar(it.accountId, it.name)) },
                            onOpenContacts = { app.open(Route.SharedContacts(it.accountId, it.name)) },
                        )
                    }
                }
                is Route.SharedFolders -> key(route) {
                    CompositionLocalProvider(LocalViewModelStoreOwner provides app.stores.owner(route)) {
                        SharedFoldersScreen(
                            route,
                            onBack = app::back,
                            onGone = app::sharedGone,
                            onOpenFolder = { app.open(Route.SharedFolder(route.account, it)) },
                        )
                    }
                }
                is Route.SharedFolder -> key(route) {
                    CompositionLocalProvider(LocalViewModelStoreOwner provides app.stores.owner(route)) {
                        SharedFolderScreen(
                            route,
                            onBack = app::back,
                            onGone = app::sharedGone,
                            onOpenMessage = { app.open(Route.SharedMail(route.account, route.mailboxId, it)) },
                        )
                    }
                }
                is Route.SharedMail -> key(route) {
                    CompositionLocalProvider(LocalViewModelStoreOwner provides app.stores.owner(route)) {
                        SharedMessageScreen(route, onBack = app::back, onCompose = { app.open(Route.Compose(it)) })
                    }
                }
                is Route.SharedCalendar -> key(route) {
                    CompositionLocalProvider(LocalViewModelStoreOwner provides app.stores.owner(route)) {
                        SharedCalendarScreen(
                            route,
                            onBack = app::back,
                            onGone = app::sharedGone,
                            onOpenEvent = { eventId, recurrenceId -> app.open(Route.SharedEventView(route.account, eventId, recurrenceId)) },
                            onAddEvent = { app.open(Route.EditEvent(null, it, route.account)) },
                        )
                    }
                }
                is Route.SharedEventView -> key(route) {
                    CompositionLocalProvider(LocalViewModelStoreOwner provides app.stores.owner(route)) {
                        SharedEventScreen(route, onBack = app::back, onEdit = { app.open(Route.EditEvent(it, null, route.account)) })
                    }
                }
                is Route.SharedContacts -> key(route) {
                    CompositionLocalProvider(LocalViewModelStoreOwner provides app.stores.owner(route)) {
                        SharedContactsScreen(
                            route,
                            onBack = app::back,
                            onGone = app::sharedGone,
                            onOpenContact = { app.open(Route.SharedContactView(route.account, it)) },
                            onAddContact = { app.open(Route.EditContact(null, route.account)) },
                        )
                    }
                }
                is Route.SharedContactView -> key(route) {
                    CompositionLocalProvider(LocalViewModelStoreOwner provides app.stores.owner(route)) {
                        SharedContactScreen(
                            route,
                            onBack = app::back,
                            onWrite = { id, to -> app.open(Route.Compose(id, to)) },
                            onEdit = { app.open(Route.EditContact(it, route.account)) },
                        )
                    }
                }
                is Route.Share -> key(route) {
                    CompositionLocalProvider(LocalViewModelStoreOwner provides app.stores.owner(route)) {
                        ShareScreen(route, onBack = app::back)
                    }
                }
                Route.Away -> CompositionLocalProvider(LocalViewModelStoreOwner provides app.stores.owner(route)) {
                    AwayScreen(onBack = app::back)
                }
                is Route.Admin -> key(route) { CompositionLocalProvider(LocalViewModelStoreOwner provides app.stores.owner(route)) { ovh.kweb.mail.ui.admin.AdminRoutes(route.screen, onBack = app::back, onOpen = { app.open(Route.Admin(it)) }, onOpenEnterprise = { app.open(Route.Enterprise(it)) }) } }
                is Route.Filters -> CompositionLocalProvider(LocalViewModelStoreOwner provides app.stores.owner(route)) {
                    FiltersScreen(route, onBack = app::back)
                }
                is Route.Enterprise -> key(route) { CompositionLocalProvider(LocalViewModelStoreOwner provides app.stores.owner(route)) { ovh.kweb.mail.ui.enterprise.EnterpriseScreen(route.page, onBack = app::back, onOpen = { app.open(Route.Enterprise(it)) }) } }
                is Route.Folders -> key(route) {
                    CompositionLocalProvider(LocalViewModelStoreOwner provides app.stores.owner(route)) {
                        FoldersScreen(route, onBack = app::back)
                    }
                }
                is Route.Label -> key(route) {
                    CompositionLocalProvider(LocalViewModelStoreOwner provides app.stores.owner(route)) {
                        LabelScreen(route, onBack = app::back, onOpen = { threadId, messageId -> app.open(Route.Thread(threadId, null, messageId)) })
                    }
                }
                Route.Scheduled -> CompositionLocalProvider(LocalViewModelStoreOwner provides app.stores.owner(route)) {
                    ScheduledScreen(onBack = app::back, onOpenDraft = { app.open(Route.Compose(it)) })
                }
                Route.Senders -> CompositionLocalProvider(LocalViewModelStoreOwner provides app.stores.owner(route)) {
                    SendersScreen(onBack = app::back)
                }
                Route.Assistant -> CompositionLocalProvider(LocalViewModelStoreOwner provides app.stores.owner(route)) {
                    AssistantScreen(onBack = app::back)
                }
                Route.MaskedAddresses -> CompositionLocalProvider(LocalViewModelStoreOwner provides app.stores.owner(route)) {
                    MaskedScreen(onBack = app::back)
                }
                Route.AccountSecurity -> CompositionLocalProvider(LocalViewModelStoreOwner provides app.stores.owner(route)) {
                    SecurityScreen(onBack = app::back)
                }
                // One thread replacing another (a notification tap) must not inherit the first one's state.
                is Route.Thread -> key(route) {
                    ThreadScreen(
                        route,
                        onBack = app::back,
                        onCompose = { app.open(Route.Compose(it)) },
                        onFilterLike = { app.open(Route.Filters(it)) },
                        onOpenFilters = { app.open(Route.Filters()) },
                    )
                }
                is Route.Compose -> key(route) { ComposeScreen(route.draftId, route.prefillTo, onLeave = app::back, onSent = app::sent) }
                Route.Settings -> SettingsScreen(
                    onBack = app::back,
                    onOpenDeliveryCheck = { app.open(Route.DeliveryCheck) },
                    onOpenAway = { app.open(Route.Away) },
                    onOpenFilters = { app.open(Route.Filters()) },
                    onOpenAdmin = { app.open(Route.Admin(ovh.kweb.mail.ui.admin.AdminScreen.Home)) },
                    onOpenAssistant = { app.open(Route.Assistant) },
                    onOpenScheduled = { app.open(Route.Scheduled) },
                    onOpenSenders = { app.open(Route.Senders) },
                    onOpenMasked = { app.open(Route.MaskedAddresses) },
                    onOpenSecurity = { app.open(Route.AccountSecurity) },
                    onSignInAgain = { app.open(Route.SignInAgain) },
                    onAddAccount = { app.open(Route.AddAccount) },
                )
                Route.Search -> SearchScreen(
                    onBack = app::back,
                    onOpen = { threadId, messageId -> app.open(Route.Thread(threadId, null, messageId)) },
                )
                Route.DeliveryCheck -> DeliveryScreen(
                    onBack = app::back,
                    onSignInAgain = { app.open(Route.SignInAgain) },
                )
                // Their sign-in is cancelled when the route leaves the stack (its store is cleared with it).
                Route.SignInAgain -> key(route) {
                    CompositionLocalProvider(LocalViewModelStoreOwner provides app.stores.owner(route)) { SignInAgain(app) }
                }
                Route.AddAccount -> key(route) {
                    CompositionLocalProvider(LocalViewModelStoreOwner provides app.stores.owner(route)) { AddAccount(app) }
                }
            }
            }
            // The root screens put it above their bottom bar themselves.
            if (!BackStack.isRoot(stack.last())) {
                SnackbarHost(snackbar, Modifier.align(Alignment.BottomCenter).navigationBarsPadding())
            }
    }
}

/** Signing in again keeps the account, so the server address is already known. */
@Composable
private fun SignInAgain(app: AppViewModel) {
    val serverUrl by app.serverUrl.collectAsStateWithLifecycle()
    val accountId = LocalAccountSession.current.record.id
    SignInScreen(
        suggestedAddress = serverUrl,
        onBack = app::back,
        onCompleted = app::back,
        target = SignInTarget.Again(accountId),
    )
}

/** Adding an account: the server may be the same as the one signed in to, so its address is suggested. */
@Composable
private fun AddAccount(app: AppViewModel) {
    val serverUrl by app.serverUrl.collectAsStateWithLifecycle()
    // A finished sign-in makes the new account the active one, which resets the stack by itself.
    SignInScreen(suggestedAddress = serverUrl, onBack = app::back, target = SignInTarget.New)
}
