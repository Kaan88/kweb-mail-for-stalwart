package ovh.kweb.mail.background

import uniffi.kweb_core.ConnectionStatus

/** What the "Connected" entry of the service says, before it is put into words. */
internal sealed interface ServiceEntryPlan {
    /** One account: exactly the four texts there always were. */
    data class Single(val status: ConnectionStatus) : ServiceEntryPlan

    /** Several accounts, all live. */
    data class AllConnected(val count: Int) : ServiceEntryPlan

    /** Several accounts, some not live: how many are, and the ones in trouble with the reason. */
    data class Partial(val connected: Int, val total: Int, val troubles: List<Trouble>) : ServiceEntryPlan
}

/** One account that is not connected for a reason the user can act on. [status] is Offline or SignInNeeded. */
internal data class Trouble(val label: String, val status: ConnectionStatus)

internal object ServiceEntryRules {
    /** [statuses] pairs each account's label with its status; at least one pair. */
    fun plan(statuses: List<Pair<String, ConnectionStatus>>): ServiceEntryPlan {
        if (statuses.size <= 1) return ServiceEntryPlan.Single(statuses.firstOrNull()?.second ?: ConnectionStatus.Connecting)
        val connected = statuses.count { it.second is ConnectionStatus.Live }
        if (connected == statuses.size) return ServiceEntryPlan.AllConnected(connected)
        val troubles = statuses.filter { it.second is ConnectionStatus.Offline || it.second is ConnectionStatus.SignInNeeded }
            .map { Trouble(it.first, it.second) }
        return ServiceEntryPlan.Partial(connected, statuses.size, troubles)
    }
}
