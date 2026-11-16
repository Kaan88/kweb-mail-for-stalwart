package ovh.kweb.mail.ui.attention

import java.time.LocalDate
import java.time.ZoneId
import java.util.Locale
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import uniffi.kweb_core.AiApplied
import uniffi.kweb_core.AiBatchApplied
import uniffi.kweb_core.AiBatchFailure
import uniffi.kweb_core.AiBatchItem
import uniffi.kweb_core.AiChange
import uniffi.kweb_core.AiProposal
import uniffi.kweb_core.AiRisk
import uniffi.kweb_core.AiSparkle
import uniffi.kweb_core.AiSparklePreview
import uniffi.kweb_core.AiSparkleResult
import uniffi.kweb_core.AiUsage
import uniffi.kweb_core.AttentionFamily
import uniffi.kweb_core.AttentionRule
import uniffi.kweb_core.EventInput
import uniffi.kweb_core.HomeTile
import uniffi.kweb_core.PhoneAction
import uniffi.kweb_core.ServiceAction

private object FakeWords : SparkleWords {
    override fun mails(count: Int, senders: String) = "$count mails from [$senders]"
    override fun events(count: Int) = "$count events"
    override fun tasks(count: Int) = "$count tasks"
    override fun theEvent() = "the event"
    override fun moreMail() = "more mail"
    override fun and(first: String, second: String) = "$first and $second"
    override fun shortened(scope: String) = "$scope, shortened"
    override fun sendersMore(names: String, more: Int) = "$names and $more more"
    override fun sentMails(count: Int) = "Sent $count mails"
    override fun sentDay() = "Sent the day"
    override fun sentEvent(lookups: Int) = if (lookups > 0) "Sent the event and lookups" else "Sent the event"
    override fun sentTaskMail() = "Sent the task and a mail"
    override fun cost(what: String, service: String, model: String, time: String) = "$what to $service · $model at $time"
    override fun tokens(input: String?, output: String?) = when {
        input != null && output != null -> "$input in, $output out"
        input != null -> "$input in"
        output != null -> "$output out"
        else -> null
    }
    override fun withTokens(note: String, tokens: String) = "$note · $tokens"
    override fun added(events: Int, tasks: Int) = "added $events events $tasks tasks"
    override fun batchFailed(count: Int, reason: String) = "$count failed: $reason"
    override fun due(text: String) = "Due $text"
    override fun whenAt(day: String, time: String) = "$day, $time"
    override fun whenRange(day: String, from: String, to: String) = "$day, $from to $to"
}

class SparkleRulesTest {
    private val today = LocalDate.of(2026, 11, 17)
    private val zone = ZoneId.of("Europe/Berlin")

    private fun f(rule: AttentionRule, family: AttentionFamily, service: ServiceAction?, times: List<Long> = emptyList(), phone: List<PhoneAction> = emptyList()) =
        finding(rule, family, HomeTile.UNREAD, times = times, phone = phone, service = service)

    private fun preview(messages: Int = 0, events: Int = 0, tasks: Int = 0, reads: Boolean = false, senders: List<String> = emptyList(), cut: Boolean = false) = AiSparklePreview(
        host = "api.example.com", model = "m", messages = messages.toUInt(), events = events.toUInt(), tasks = tasks.toUInt(), characters = 10u,
        isCut = cut, senders = senders, readsMail = reads, text = "x",
    )

    // ---- the finding to the request ----

    @Test fun addDatesTakesTheFindingsMailsUpToFive() {
        val many = f(AttentionRule.DATE_IN_MAIL, AttentionFamily.MAIL, ServiceAction.ADD_DATES).copy(messageIds = (1..7).map { "m$it" })
        val task = SparkleRules.task(many, ServiceAction.ADD_DATES, today, zone) as AiSparkle.AddDates
        assertEquals(listOf("m1", "m2", "m3", "m4", "m5"), task.messageIds)
        assertEquals("2026-11-17", task.today)
        assertEquals("Europe/Berlin", task.timeZone)
        assertNull(SparkleRules.task(many.copy(messageIds = emptyList()), ServiceAction.ADD_DATES, today, zone))
    }

    @Test fun planMyDayIsTodayExceptForABusyDay() {
        val overdue = f(AttentionRule.OVERDUE, AttentionFamily.TASKS, ServiceAction.PLAN_MY_DAY)
        assertEquals("2026-11-17", (SparkleRules.task(overdue, ServiceAction.PLAN_MY_DAY, today, zone) as AiSparkle.PlanMyDay).day)
        val at = 1_795_089_600L
        val busy = f(AttentionRule.BUSY_DAY, AttentionFamily.CALENDAR, ServiceAction.PLAN_MY_DAY, times = listOf(at))
        val day = (SparkleRules.task(busy, ServiceAction.PLAN_MY_DAY, today, zone) as AiSparkle.PlanMyDay).day
        assertEquals(java.time.Instant.ofEpochSecond(at).atZone(zone).toLocalDate().toString(), day)
    }

    @Test fun lookInMailNamesTheEventAndItsOccurrence() {
        val look = f(AttentionRule.NO_PLACE, AttentionFamily.CALENDAR, ServiceAction.LOOK_IN_MAIL, phone = listOf(PhoneAction.MoveEvent("e1", "r1")))
            .copy(eventIds = listOf("e1"))
        val task = SparkleRules.task(look, ServiceAction.LOOK_IN_MAIL, today, zone) as AiSparkle.LookInMail
        assertEquals("e1", task.eventId)
        assertEquals("r1", task.recurrenceId)
        assertNull(SparkleRules.task(look.copy(eventIds = emptyList()), ServiceAction.LOOK_IN_MAIL, today, zone))
    }

    @Test fun checkTheMailNeedsBothTheTaskAndTheMail() {
        val check = f(AttentionRule.MAIL_MAY_FINISH, AttentionFamily.TASKS, ServiceAction.CHECK_THE_MAIL).copy(taskIds = listOf("t1"), messageIds = listOf("m1"))
        val task = SparkleRules.task(check, ServiceAction.CHECK_THE_MAIL, today, zone) as AiSparkle.CheckTheMail
        assertEquals("t1" to "m1", task.taskId to task.messageId)
        assertNull(SparkleRules.task(check.copy(taskIds = emptyList()), ServiceAction.CHECK_THE_MAIL, today, zone))
        assertNull(SparkleRules.task(check.copy(messageIds = emptyList()), ServiceAction.CHECK_THE_MAIL, today, zone))
    }

    @Test fun replyIdeasTakesTheFirstMail() {
        val wait = f(AttentionRule.WAITING_FOR_YOU, AttentionFamily.MAIL, ServiceAction.REPLY_IDEAS).copy(messageIds = listOf("m9", "m8"))
        assertEquals("m9", (SparkleRules.task(wait, ServiceAction.REPLY_IDEAS, today, zone) as AiSparkle.ReplyIdeas).messageId)
    }

    // ---- the ask rule ----

    @Test fun theFirstUseAsksWhateverTheSetting() {
        assertEquals(SparkleRules.Ask.FIRST, SparkleRules.ask(false, false, false, preview(messages = 1)))
        assertEquals(SparkleRules.Ask.FIRST, SparkleRules.ask(false, false, true, preview(events = 2)))
    }

    @Test fun theChatsOwnYesIsAskedWithTheFirstUse() {
        assertEquals(SparkleRules.Ask.FIRST, SparkleRules.ask(true, true, false, preview(reads = true)))
        assertTrue(SparkleRules.needsChatYes(AiSparkle.LookInMail("e", null, "d", "z")))
        assertTrue(SparkleRules.needsChatYes(AiSparkle.CheckTheMail("t", "m", "d", "z")))
        assertFalse(SparkleRules.needsChatYes(AiSparkle.AddDates(listOf("m"), "d", "z")))
    }

    @Test fun askEachRunAsksOnlyForARunThatSendsMail() {
        assertEquals(SparkleRules.Ask.EACH, SparkleRules.ask(true, false, true, preview(messages = 2)))
        assertEquals(SparkleRules.Ask.EACH, SparkleRules.ask(true, false, true, preview(events = 1, reads = true)))
        // Plan my day sends calendar and task data only: it runs at once after the first agreement.
        assertEquals(SparkleRules.Ask.NONE, SparkleRules.ask(true, false, true, preview(events = 5, tasks = 3)))
    }

    @Test fun withTheSettingOffNothingIsAskedAfterTheAgreement() {
        assertEquals(SparkleRules.Ask.NONE, SparkleRules.ask(true, false, false, preview(messages = 3)))
    }

    // ---- the words ----

    @Test fun theDialogNamesMailsSendersEventsAndTasks() {
        val p = preview(messages = 2, senders = listOf("Merve Aydın <merve@example.com>", "Hetzner <billing@hetzner.com>"))
        assertEquals("2 mails from [Merve Aydın and Hetzner]", SparkleRules.scope(p, AiSparkle.AddDates(listOf("a", "b"), "d", "z"), FakeWords))
        val plan = preview(events = 5, tasks = 3)
        assertEquals("5 events and 3 tasks", SparkleRules.scope(plan, AiSparkle.PlanMyDay("d", "d", "z"), FakeWords))
        val look = preview(events = 1, reads = true)
        assertEquals("the event and more mail", SparkleRules.scope(look, AiSparkle.LookInMail("e", null, "d", "z"), FakeWords))
        assertEquals("the event and more mail, shortened", SparkleRules.scope(look.copy(isCut = true), AiSparkle.LookInMail("e", null, "d", "z"), FakeWords))
    }

    @Test fun sendersAreNamesWithoutAddressesAndShortened() {
        assertEquals("a@b.c", SparkleRules.senderNames(listOf("<a@b.c>"), FakeWords))
        assertEquals("A and B", SparkleRules.senderNames(listOf("A <a@x>", "\"B\" <b@x>", "A <a@x>"), FakeWords))
        assertEquals("A, B and 2 more", SparkleRules.senderNames(listOf("A <a@x>", "B <b@x>", "C <c@x>", "D <d@x>"), FakeWords))
    }

    private fun result(model: String = "gpt-5", messages: Int = 2, steps: Int = 0, usage: AiUsage? = null) = AiSparkleResult(
        text = "", proposals = emptyList(), ideas = emptyList(), found = null, verdict = null, changesAllowed = true, usage = usage, host = "h",
        model = model, messages = messages.toUInt(), characters = 100u, isCut = false, steps = steps.toUInt(),
    )

    @Test fun theCostNoteSaysWhatWasSentAndTheTokens() {
        val note = SparkleRules.costNote(
            AiSparkle.AddDates(listOf("a", "b"), "d", "z"), result(usage = AiUsage(2412u, 318u)), "OpenAI", "09:41", FakeWords, Locale.ENGLISH,
        )
        assertEquals("Sent 2 mails to OpenAI · gpt-5 at 09:41 · 2,412 in, 318 out", note)
    }

    @Test fun theCostNoteLeavesTheTokensOutWhenTheServiceSaidNothing() {
        val note = SparkleRules.costNote(AiSparkle.PlanMyDay("d", "d", "z"), result(), "OpenAI", "09:41", FakeWords, Locale.ENGLISH)
        assertEquals("Sent the day to OpenAI · gpt-5 at 09:41", note)
        val half = SparkleRules.costNote(AiSparkle.ReplyIdeas("m"), result(messages = 1, usage = AiUsage(null, 7u)), "X", "9:00", FakeWords, Locale.ENGLISH)
        assertEquals("Sent 1 mails to X · gpt-5 at 9:00 · 7 out", half)
    }

    @Test fun theCostNoteForALookupMentionsWhatItRead() {
        assertEquals(
            "Sent the event and lookups to S · m at 1:00",
            SparkleRules.costNote(AiSparkle.LookInMail("e", null, "d", "z"), result(model = "m", steps = 3), "S", "1:00", FakeWords, Locale.ENGLISH),
        )
    }

    private fun input(title: String, start: String, secs: Long, task: Boolean = false, allDay: Boolean = false) = EventInput(
        calendarId = "c", title = title, location = "", description = "", start = start, timeZone = null, durationSecs = secs.toULong(),
        allDay = allDay, isTask = task, repeat = null, alertMinutes = emptyList(),
    )

    @Test fun whenTextForEventsAndTasks() {
        assertEquals("Thu 19 Nov, 15:00 to 16:00", SparkleRules.whenText(input("a", "2026-11-19T15:00:00", 3600), Locale.UK, FakeWords))
        assertEquals("Due Mon 30 Nov", SparkleRules.whenText(input("t", "2026-11-30T00:00:00", 0, task = true), Locale.UK, FakeWords))
        assertEquals("Fri 20 Nov", SparkleRules.whenText(input("e", "2026-11-20T00:00:00", 86400, allDay = true), Locale.UK, FakeWords))
    }

    @Test fun aPlanLineIsTheTimeTheTitleAndTheLength() {
        assertEquals("10:00 Send invoice (15 min)", SparkleRules.planLine(input("Send invoice", "2026-11-17T10:00:00", 900), Locale.UK))
    }

    // ---- the cards ----

    private fun card(id: String, task: Boolean = false) = AiProposal(id, AiRisk.CHANGE, AiChange.AddEvent(input("T$id", "2026-11-19T10:00:00", 900, task = task), "Personal"))

    private fun run(vararg cards: AiProposal, extras: Boolean = false) = SparkleRun(
        finding = f(AttentionRule.DATE_IN_MAIL, AttentionFamily.MAIL, ServiceAction.ADD_DATES),
        task = AiSparkle.AddDates(listOf("m"), "d", "z"), runId = "r", phase = RunPhase.DONE,
        result = result().let { if (extras) it.copy(text = "Nothing found") else it }, open = cards.toList(),
    )

    @Test fun addingACardRemovesItAndRemembersThatSomethingWasAdded() {
        val r = run(card("a"), card("b")).adding("a").added(listOf("a"))
        assertEquals(listOf("b"), r.open.map { it.id })
        assertTrue(r.addedAny)
        assertFalse(r.isUsedUp)
    }

    @Test fun aRunWithEverythingAddedIsCompleted() {
        val r = run(card("a"), card("b")).added(listOf("a", "b"))
        assertTrue(r.isUsedUp)
        assertTrue(r.isCompleted)
    }

    @Test fun aRunWithEverythingDismissedIsUsedUpButNotCompleted() {
        val r = run(card("a")).dismissedCard("a")
        assertTrue(r.isUsedUp)
        assertFalse(r.isCompleted)
    }

    @Test fun aFailedCardStaysWithItsReason() {
        val r = run(card("a")).adding("a").failed(mapOf("a" to "no calendar"))
        assertEquals(listOf("a"), r.open.map { it.id })
        assertEquals("no calendar", r.errors["a"])
        assertTrue(r.busy.isEmpty())
        assertFalse(r.isUsedUp)
    }

    @Test fun thePlainLineStaysUntilItIsDismissed() {
        val r = run(extras = true)
        assertTrue(r.hasContent)
        assertTrue(r.dismissedExtras().isUsedUp)
    }

    @Test fun aRunThatIsNotDoneIsNeverUsedUp() {
        assertFalse(run().copy(phase = RunPhase.RUNNING).isUsedUp)
        assertFalse(run().copy(phase = RunPhase.FAILED).isUsedUp)
    }

    // ---- the batch ----

    @Test fun theBatchLineCountsEventsAndTasksAndNamesTheFirstReason() {
        val done = AiBatchApplied(
            applied = listOf("a", "b", "c").map { AiBatchItem(it, AiApplied(true, null)) },
            failed = listOf(AiBatchFailure("d", "calendar is read-only"), AiBatchFailure("e", "other")),
            undoId = "u",
        )
        val line = SparkleRules.batchLine(done, { it == "c" }, FakeWords)
        assertEquals("added 2 events 1 tasks", line.added)
        assertEquals("2 failed: calendar is read-only", line.failed)
    }

    @Test fun aBatchThatAllFailedHasNoAddedLine() {
        val done = AiBatchApplied(emptyList(), listOf(AiBatchFailure("d", "x")), null)
        val line = SparkleRules.batchLine(done, { false }, FakeWords)
        assertNull(line.added)
        assertEquals("1 failed: x", line.failed)
    }
}
