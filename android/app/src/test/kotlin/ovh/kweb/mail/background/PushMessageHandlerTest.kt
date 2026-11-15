package ovh.kweb.mail.background

import java.util.concurrent.CountDownLatch
import java.util.concurrent.Executors
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test
import uniffi.kweb_core.Plan
import uniffi.kweb_core.Reminder
import uniffi.kweb_core.WebPushMessage

class PushMessageHandlerTest {
    @Test
    fun aMessageWithoutABodyEndsInACheckWhenItMayMeanNews() {
        // Google discarded pending messages: no account is named, news may be lost.
        org.junit.Assert.assertTrue(PushMessageHandler.dropNeedsCheck("deleted_messages", accountKnown = false))
        // A known account, the body missing.
        org.junit.Assert.assertTrue(PushMessageHandler.dropNeedsCheck(null, accountKnown = true))
        // For nobody we know: a plain drop, so another sender's noise costs nothing.
        org.junit.Assert.assertFalse(PushMessageHandler.dropNeedsCheck(null, accountKnown = false))
        org.junit.Assert.assertFalse(PushMessageHandler.dropNeedsCheck("gcm", accountKnown = false))
    }

    private val executor = Executors.newCachedThreadPool { r -> Thread(r).apply { isDaemon = true } }
    private val plan = Plan(post = emptyList(), cancel = emptyList())
    private val reminder = Reminder("e", "a", null, false, "Title", "", null, null, false)

    private val applied = mutableListOf<Plan>()
    private val shown = mutableListOf<Reminder>()
    private var handedOver = 0

    @After fun stop() = executor.shutdownNow().let { }

    private fun handler(budgetMs: Long = 1000, receive: () -> WebPushMessage) = PushMessageHandler(
        executor, budgetMs, receive, { applied += it }, { shown += it }, { handedOver++ },
    )

    @Test
    fun aCheckedMessageAppliesThePlan() {
        assertEquals(MessageOutcome.Applied, handler { WebPushMessage.Checked(plan) }.run())
        assertEquals(listOf(plan), applied)
        assertEquals(0, handedOver)
    }

    @Test
    fun aReminderIsShown() {
        assertEquals(MessageOutcome.ReminderShown, handler { WebPushMessage.Reminder(reminder) }.run())
        assertEquals(listOf(reminder), shown)
        assertEquals(0, handedOver)
    }

    @Test
    fun aVerificationNeedsNothingFromTheReceiver() {
        assertEquals(MessageOutcome.Verified, handler { WebPushMessage.Verified }.run())
        assertEquals(0, handedOver)
    }

    @Test
    fun ignoredAndUndecryptableMessagesDoNothing() {
        assertEquals(MessageOutcome.Nothing, handler { WebPushMessage.Ignored }.run())
        assertEquals(MessageOutcome.Nothing, handler { WebPushMessage.Undecryptable }.run())
        assertEquals(0, handedOver)
        assertEquals(emptyList<Plan>(), applied)
    }

    @Test
    fun anErrorHandsOverToTheExpeditedCheck() {
        assertEquals(MessageOutcome.HandedOver, handler { throw IllegalStateException("temporary") }.run())
        assertEquals(1, handedOver)
        assertEquals(emptyList<Plan>(), applied)
    }

    @Test
    fun aPassThatIsNotFinishedInTheBudgetHandsOverAndStillShowsItsOwnPlanOnce() {
        val release = CountDownLatch(1)
        val outcome = handler(budgetMs = 50) { release.await(); WebPushMessage.Checked(plan) }.run()
        assertEquals(MessageOutcome.HandedOver, outcome)
        assertEquals(1, handedOver)
        release.countDown()
        val until = System.currentTimeMillis() + 2000
        while (applied.isEmpty() && System.currentTimeMillis() < until) Thread.sleep(10)
        assertEquals("the late plan is shown, with its alert", listOf(plan), applied)
    }

    @Test
    fun aLateReminderIsStillShown() {
        val release = CountDownLatch(1)
        handler(budgetMs = 50) { release.await(); WebPushMessage.Reminder(reminder) }.run()
        release.countDown()
        val until = System.currentTimeMillis() + 2000
        while (shown.isEmpty() && System.currentTimeMillis() < until) Thread.sleep(10)
        assertEquals(listOf(reminder), shown)
    }

    @Test
    fun aFailureWhileShowingHandsOver() {
        val h = PushMessageHandler(executor, 1000, { WebPushMessage.Checked(plan) }, { error("boom") }, {}, { handedOver++ })
        assertEquals(MessageOutcome.HandedOver, h.run())
        assertEquals(1, handedOver)
    }

    @Test
    fun theMessageFindsItsAccountBySubtype() {
        val subtypes = listOf("a" to "wp:https://x/#1", "b" to null, "c" to "wp:https://x/#2")
        assertEquals("c", PushMessageHandler.accountFor("wp:https://x/#2", subtypes))
        assertEquals("a", PushMessageHandler.accountFor("wp:https://x/#1", subtypes))
    }

    @Test
    fun aMessageForAnUnknownOrMissingSubtypeIsDropped() {
        val subtypes = listOf("a" to "wp:https://x/#1", "b" to null)
        assertNull(PushMessageHandler.accountFor("wp:https://x/#9", subtypes))
        assertNull(PushMessageHandler.accountFor(null, subtypes))
        assertNull(PushMessageHandler.accountFor("", listOf("b" to "")))
    }
}
