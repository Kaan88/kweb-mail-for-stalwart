package ovh.kweb.mail.ui

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class RouteStateRulesTest {
    @Test fun equalRoutesShareAKey() =
        assertEquals(RouteStateRules.key("a", Route.Thread("t", "m", null)), RouteStateRules.key("a", Route.Thread("t", "m", null)))

    @Test fun differentRoutesHaveDifferentKeys() {
        assertNotEquals(RouteStateRules.key("a", Route.Settings), RouteStateRules.key("a", Route.Search))
        assertNotEquals(RouteStateRules.key("a", Route.Thread("t1", null, null)), RouteStateRules.key("a", Route.Thread("t2", null, null)))
    }

    @Test fun theSameScreenOfAnotherAccountHasAnotherKey() =
        assertNotEquals(RouteStateRules.key("a", Route.Settings), RouteStateRules.key("b", Route.Settings))

    @Test fun aRouteThatLeftTheStackIsGone() {
        val before = RouteStateRules.keys("a", listOf(Route.MailList, Route.Settings, Route.Away))
        val gone = RouteStateRules.gone(before, "a", listOf(Route.MailList, Route.Settings))
        assertEquals(setOf(RouteStateRules.key("a", Route.Away)), gone)
    }

    @Test fun nothingIsGoneWhileTheRouteIsUnderneath() =
        assertTrue(RouteStateRules.gone(RouteStateRules.keys("a", listOf(Route.MailList, Route.Settings)), "a", listOf(Route.MailList, Route.Settings, Route.Away)).isEmpty())
}
