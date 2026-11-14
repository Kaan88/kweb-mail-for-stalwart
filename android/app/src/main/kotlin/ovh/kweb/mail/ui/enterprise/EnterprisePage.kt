package ovh.kweb.mail.ui.enterprise

/**
 * The screens a licensed server adds. The admin home opens one with
 * `app.open(Route.Enterprise(EnterprisePage.Dashboard))` (and so on); [Delivery] is opened by the delivery history
 * itself. [EnterprisePages.rows] lists the five the home shows, in its order.
 */
sealed interface EnterprisePage {
    data object Dashboard : EnterprisePage

    data object Deliveries : EnterprisePage

    /** One message followed through the server, by the id of [uniffi.kweb_core.Delivery]. */
    data class Delivery(val id: String) : EnterprisePage

    data object Recovery : EnterprisePage

    data object Alerts : EnterprisePage

    data object Tenants : EnterprisePage
}

object EnterprisePages {
    /** The rows of the "Enterprise" group of the admin home, in the order of the mock-up. */
    val rows: List<EnterprisePage> =
        listOf(EnterprisePage.Dashboard, EnterprisePage.Deliveries, EnterprisePage.Recovery, EnterprisePage.Alerts, EnterprisePage.Tenants)
}
