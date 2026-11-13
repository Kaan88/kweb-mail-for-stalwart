package ovh.kweb.mail.background

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import uniffi.kweb_core.Cancel
import uniffi.kweb_core.MailNotification
import uniffi.kweb_core.Plan

class PlanRulesTest {
    private fun mail(id: Int, silent: Boolean = false) =
        MailNotification("m$id", id, "Sender", "Subject", "Preview", 1_000L, silent)

    private val plan = Plan(post = listOf(mail(1001), mail(1002)), cancel = listOf(Cancel("m1003", 1003)))

    @Test
    fun everythingPassesWhenAllSwitchesAreOn() {
        assertEquals(plan, plan.honouring(mailEnabled = true, removeWhenReadElsewhere = true))
    }

    @Test
    fun mailOffPostsNothingButStillCancels() {
        val result = plan.honouring(mailEnabled = false, removeWhenReadElsewhere = true)
        assertTrue(result.post.isEmpty())
        assertEquals(plan.cancel, result.cancel)
    }

    @Test
    fun removeElsewhereOffKeepsPostsAndDropsCancels() {
        val result = plan.honouring(mailEnabled = true, removeWhenReadElsewhere = false)
        assertEquals(plan.post, result.post)
        assertTrue(result.cancel.isEmpty())
    }

    @Test
    fun aNotificationAlreadyOnScreenIsRefreshedSilently() {
        assertTrue(isSilent(mail(1001), onScreen = setOf(1001)))
        assertFalse(isSilent(mail(1001), onScreen = setOf(1002)))
    }

    @Test
    fun theCoresSilentFlagIsKept() {
        assertTrue(isSilent(mail(1001, silent = true), onScreen = emptySet()))
    }

    @Test
    fun idsAfterAddsPostsAndRemovesCancels() {
        val after = idsAfter(onScreen = setOf(1003, 1004), plan = plan)
        assertEquals(setOf(1001, 1002, 1004), after)
    }

    @Test
    fun whenIsInMillisecondsAndFallsBackToNow() {
        assertEquals(5_000L, whenMillis(5, nowMillis = 99))
        assertEquals(99L, whenMillis(0, nowMillis = 99))
    }
}
