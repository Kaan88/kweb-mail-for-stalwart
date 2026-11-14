package ovh.kweb.mail.background

import org.junit.Assert.assertEquals
import org.junit.Test
import uniffi.kweb_core.ConnectionStatus

class ServiceEntryRulesTest {
    private fun plan(vararg s: Pair<String, ConnectionStatus>) = ServiceEntryRules.plan(s.toList())

    @Test
    fun oneAccountKeepsTodaysFourTexts() {
        for (status in listOf(ConnectionStatus.Live(1L), ConnectionStatus.Offline("no network"), ConnectionStatus.SignInNeeded, ConnectionStatus.Connecting)) {
            assertEquals(ServiceEntryPlan.Single(status), plan("kaan" to status))
        }
    }

    @Test
    fun severalAccountsAllLiveSayHowMany() {
        assertEquals(ServiceEntryPlan.AllConnected(3), plan("a" to ConnectionStatus.Live(1L), "b" to ConnectionStatus.Live(1L), "c" to ConnectionStatus.Live(1L)))
    }

    @Test
    fun severalAccountsSomeNotLiveCountAndListTheOnesInTrouble() {
        val result = plan(
            "a" to ConnectionStatus.Live(1L),
            "bob@example.org" to ConnectionStatus.SignInNeeded,
            "c" to ConnectionStatus.Offline("timeout"),
        )
        assertEquals(
            ServiceEntryPlan.Partial(
                1, 3,
                listOf(Trouble("bob@example.org", ConnectionStatus.SignInNeeded), Trouble("c", ConnectionStatus.Offline("timeout"))),
            ),
            result,
        )
    }

    @Test
    fun anAccountStillConnectingIsNotConnectedButNotInTrouble() {
        assertEquals(
            ServiceEntryPlan.Partial(1, 2, emptyList()),
            plan("a" to ConnectionStatus.Live(1L), "b" to ConnectionStatus.Connecting),
        )
    }

    @Test
    fun noAccountIsTheConnectingText() {
        assertEquals(ServiceEntryPlan.Single(ConnectionStatus.Connecting), ServiceEntryRules.plan(emptyList()))
    }
}
