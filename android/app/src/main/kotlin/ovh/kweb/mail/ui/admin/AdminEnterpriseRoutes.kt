package ovh.kweb.mail.ui.admin

import androidx.compose.runtime.Composable
import ovh.kweb.mail.ui.enterprise.EnterprisePage
import ovh.kweb.mail.ui.enterprise.EnterpriseScreen

/** The Enterprise page an area of the admin home opens; the plain areas have none. */
internal fun enterprisePageOf(area: AdminArea): EnterprisePage? = when (area) {
    AdminArea.Dashboard -> EnterprisePage.Dashboard
    AdminArea.History -> EnterprisePage.Deliveries
    AdminArea.Recovery -> EnterprisePage.Recovery
    AdminArea.Alerts -> EnterprisePage.Alerts
    AdminArea.Tenants -> EnterprisePage.Tenants
    else -> null
}

/**
 * An Enterprise area of the admin home. The screens come from `ui/enterprise` and build their view models under the
 * route's own store, which hands them the session of the account the admin home was opened for. [onOpenPage] opens
 * another Enterprise page (the delivery history opens one delivery).
 */
@Composable
internal fun EnterpriseArea(area: AdminArea, onBack: () -> Unit, onOpenPage: (EnterprisePage) -> Unit) {
    val page = enterprisePageOf(area) ?: return
    EnterpriseScreen(page, onBack = onBack, onOpen = onOpenPage)
}
