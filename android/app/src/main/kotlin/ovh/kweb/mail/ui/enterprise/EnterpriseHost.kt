package ovh.kweb.mail.ui.enterprise

import androidx.compose.runtime.Composable

/**
 * The screen of one [EnterprisePage]. `AppContent` calls it from the single branch for `Route.Enterprise`, inside the
 * route's own view model store (`app.stores.owner(route)`), so each screen's state lives as long as it is on the stack.
 * [onOpen] opens another page (the delivery history opens one delivery).
 */
@Composable
fun EnterpriseScreen(page: EnterprisePage, onBack: () -> Unit, onOpen: (EnterprisePage) -> Unit) {
    when (page) {
        EnterprisePage.Dashboard -> DashboardScreen(onBack)
        EnterprisePage.Deliveries -> DeliveriesScreen(onBack, onOpen = { onOpen(EnterprisePage.Delivery(it)) })
        is EnterprisePage.Delivery -> DeliveryDetailScreen(page.id, onBack)
        EnterprisePage.Recovery -> RecoveryScreen(onBack)
        EnterprisePage.Alerts -> AlertsScreen(onBack)
        EnterprisePage.Tenants -> TenantsScreen(onBack)
    }
}
