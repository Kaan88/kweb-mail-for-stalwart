package ovh.kweb.mail.ui.admin

import androidx.compose.runtime.Composable

/** The screens of the admin area. A route of the app ([ovh.kweb.mail.ui.Route.Admin]) holds one of these. */
sealed interface AdminScreen {
    /** The screen holds what the user typed (a new account, a password or limit in a dialog): it is treated as a form. */
    val holdsInput: Boolean get() = this is NewAccount || this is Account
    /** The start: the figures, or the access screen's content while no key is kept. */
    data object Home : AdminScreen

    /** What the stored key opens, and Forget this key. */
    data object Access : AdminScreen

    data object Queue : AdminScreen

    data class QueuedMessage(val id: String) : AdminScreen

    data object Accounts : AdminScreen

    data class Account(val id: String) : AdminScreen

    data object NewAccount : AdminScreen

    data object Blocked : AdminScreen

    data object Logs : AdminScreen

    data object Maintenance : AdminScreen

    /** An Enterprise area: its screen is in ui/enterprise; [EnterpriseArea] shows it. */
    data class Enterprise(val area: AdminArea) : AdminScreen
}

/**
 * Shows one admin screen. [onOpen] goes to another one (it pushes a route). The view models come from the store
 * of the route this is shown for, so they live as long as the route is on the stack.
 */
@Composable
fun AdminRoutes(
    screen: AdminScreen,
    onBack: () -> Unit,
    onOpen: (AdminScreen) -> Unit,
    onOpenEnterprise: (ovh.kweb.mail.ui.enterprise.EnterprisePage) -> Unit,
) {
    when (screen) {
        AdminScreen.Home -> AdminHomeScreen(onBack = onBack, onOpen = onOpen)
        AdminScreen.Access -> AdminAccessScreen(onBack = onBack)
        AdminScreen.Queue -> QueueScreen(onBack = onBack, onOpen = onOpen)
        is AdminScreen.QueuedMessage -> QueuedMessageScreen(screen.id, onBack = onBack)
        AdminScreen.Accounts -> AccountsScreen(onBack = onBack, onOpen = onOpen)
        is AdminScreen.Account -> AccountScreen(screen.id, onBack = onBack)
        AdminScreen.NewAccount -> NewAccountScreen(onBack = onBack)
        AdminScreen.Blocked -> BlockedScreen(onBack = onBack)
        AdminScreen.Logs -> LogsScreen(onBack = onBack)
        AdminScreen.Maintenance -> MaintenanceScreen(onBack = onBack)
        is AdminScreen.Enterprise -> EnterpriseArea(screen.area, onBack = onBack, onOpenPage = onOpenEnterprise)
    }
}
