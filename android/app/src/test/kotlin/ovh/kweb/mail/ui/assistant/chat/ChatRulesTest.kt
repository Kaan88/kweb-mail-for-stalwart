package ovh.kweb.mail.ui.assistant.chat

import java.time.LocalDateTime
import java.time.format.DateTimeFormatter
import java.time.format.FormatStyle
import java.util.Locale
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertSame
import org.junit.Assert.assertTrue
import org.junit.Test
import ovh.kweb.mail.R
import ovh.kweb.mail.ui.UiMessage
import ovh.kweb.mail.ui.assistant.AssistantRules
import ovh.kweb.mail.ui.assistant.AssistantRules.ChatGate as Gate
import ovh.kweb.mail.ui.assistant.chat.ChatRules.ChangeKind
import ovh.kweb.mail.ui.assistant.chat.ChatRules.EventField
import ovh.kweb.mail.ui.assistant.chat.ChatRules.StepKind
import uniffi.kweb_core.AiActionMode
import uniffi.kweb_core.AiApplied
import uniffi.kweb_core.AiAutoResult
import uniffi.kweb_core.AiAbout
import uniffi.kweb_core.AiChange
import uniffi.kweb_core.AiChatEnd
import uniffi.kweb_core.AiChatTurn
import uniffi.kweb_core.AiEffort
import uniffi.kweb_core.AiLink
import uniffi.kweb_core.AiMessageRef
import uniffi.kweb_core.AiProposal
import uniffi.kweb_core.AiProvider
import uniffi.kweb_core.AiRisk
import uniffi.kweb_core.AiSettings
import uniffi.kweb_core.AiStep
import uniffi.kweb_core.AiToolKind
import uniffi.kweb_core.EventInput
import uniffi.kweb_core.Exception as CoreException

class ChatRulesTest {
    /** English words, as the resources have them. */
    private object Words : ChatWords {
        private fun messages(count: Int) = if (count == 1) "1 message" else "$count messages"

        override fun step(kind: StepKind) = when (kind) {
            StepKind.SEARCH -> "Searching mail"
            StepKind.READ_MESSAGE -> "Reading a message"
            StepKind.SUGGEST -> "Preparing a suggestion"
            StepKind.OTHER -> "Working"
            else -> kind.name
        }

        override fun stepWithDetail(line: String, detail: String) = "$line: $detail"

        override fun title(kind: ChangeKind, count: Int, target: String) = when (kind) {
            ChangeKind.ARCHIVE -> "Archive ${messages(count)}?"
            ChangeKind.MOVE -> "Move ${messages(count)} to $target?"
            ChangeKind.MARK_READ -> "Mark ${messages(count)} as read?"
            ChangeKind.MARK_UNREAD -> "Mark ${messages(count)} as unread?"
            ChangeKind.STAR -> "Star ${messages(count)}?"
            ChangeKind.UNSTAR -> "Remove the star from ${messages(count)}?"
            ChangeKind.TAG -> "Tag ${messages(count)} with $target?"
            ChangeKind.UNTAG -> "Remove $target from ${messages(count)}?"
            ChangeKind.ADD_EVENT -> "Add this event?"
            ChangeKind.ADD_TASK -> "Add this task?"
        }

        override fun done(kind: ChangeKind, count: Int, target: String) = when (kind) {
            ChangeKind.ARCHIVE -> "Archived ${messages(count)}"
            ChangeKind.MOVE -> "Moved ${messages(count)} to $target"
            ChangeKind.ADD_EVENT -> "Added the event"
            else -> "$kind $count $target"
        }

        override fun warning(count: Int) = "This changes ${messages(count)} at once."

        override fun allDay(day: String) = "$day, all day"

        override fun inZone(time: String, zone: String) = "$time ($zone)"

        override fun reminder(minutes: Int) = "$minutes minutes before"

        override fun repeats() = "Yes"
    }

    private fun settings(enabled: Boolean = true, consented: Boolean = true, allowChat: Boolean = true, chatConsented: Boolean = true) = AiSettings(
        enabled, AiProvider.OPEN_AI_COMPATIBLE, "https://api.openai.com/v1", "gpt-x", AiEffort.DEFAULT, false, consented, true, true, true,
        allowIdeas = false, allowExtract = false, allowExplain = false, allowChat = allowChat, chatConsented = chatConsented, actions = AiActionMode.ASK_FIRST,
    )

    private fun ref(id: String) = AiMessageRef(id, "t-$id", "Subject $id", "Ada <ada@example.org>", "Inbox")

    private fun refs(count: Int) = (1..count).map { ref("m$it") }

    private fun proposal(id: String, change: AiChange, risk: AiRisk = AiRisk.CHANGE) = AiProposal(id, risk, change)

    private fun turn(
        text: String = "Done.",
        proposals: List<AiProposal> = emptyList(),
        links: List<AiLink> = emptyList(),
        steps: UInt = 1u,
        end: AiChatEnd = AiChatEnd.ANSWERED,
        toolsOff: Boolean = false,
        automatic: List<AiAutoResult> = emptyList(),
    ) = AiChatTurn(text, proposals, links, steps, end, toolsOff, automatic)

    private fun step(tool: AiToolKind, detail: String = "") = AiStep(tool, AiRisk.READ, detail)

    private fun event(
        start: String = "2026-11-12T10:00:00",
        zone: String? = "Europe/Berlin",
        duration: ULong = 3600u,
        allDay: Boolean = false,
        isTask: Boolean = false,
        location: String = "",
        description: String = "",
    ) = EventInput("cal1", "Dentist", location, description, start, zone, duration, allDay, isTask, null, emptyList())

    private val ready = ChatState(gate = Gate.READY)

    private fun running(runId: String = "r1", text: String = "What is new?") = ChatRules.sent(ready, text, runId)

    // ---- the gate ----

    @Test fun theGateFollowsTheCoresOrder() {
        assertEquals(Gate.READY, AssistantRules.chatReady(settings()))
        assertEquals(Gate.OFF, AssistantRules.chatReady(settings(enabled = false, allowChat = false, chatConsented = false)))
        assertEquals(Gate.CHAT_OFF, AssistantRules.chatReady(settings(allowChat = false, chatConsented = false)))
        assertEquals(Gate.NOT_AGREED, AssistantRules.chatReady(settings(chatConsented = false)))
        // The chat's yes needs the first yes: without it the chat is not agreed either.
        assertEquals(Gate.NOT_AGREED, AssistantRules.chatReady(settings(consented = false)))
    }

    @Test fun theListOffersTheChatOnlyWhenAssistantAndChatAreOn() {
        assertFalse(ChatRules.entryShown(null))
        assertFalse(ChatRules.entryShown(settings(enabled = false)))
        assertFalse(ChatRules.entryShown(settings(allowChat = false)))
        assertTrue(ChatRules.entryShown(settings()))
        // Not agreed yet: the entry shows and the chat screen asks for the rest.
        assertTrue(ChatRules.entryShown(settings(chatConsented = false)))
    }

    @Test fun sendNeedsAReadyGateAnIdleChatAndText() {
        assertTrue(ChatRules.canSend(Gate.READY, running = false, input = "hello"))
        assertFalse(ChatRules.canSend(Gate.READY, running = false, input = "  \n"))
        assertFalse(ChatRules.canSend(Gate.READY, running = true, input = "hello"))
        assertFalse(ChatRules.canSend(null, running = false, input = "hello"))
        for (gate in listOf(Gate.OFF, Gate.CHAT_OFF, Gate.NOT_AGREED)) assertFalse(ChatRules.canSend(gate, running = false, input = "hello"))
    }

    // ---- one send ----

    @Test fun sendingAddsTheMessageAndStartsTheRun() {
        val before = ready.copy(items = listOf(ChatItem.Theirs("Earlier.")), notice = UiMessage.Res(R.string.chat_failed))
        val s = ChatRules.sent(before, "What is new?", "r1")
        assertEquals(listOf(ChatItem.Theirs("Earlier."), ChatItem.Mine("What is new?")), s.items)
        assertTrue(s.running)
        assertEquals("r1", s.runId)
        assertEquals(1, s.runStart)
        assertNull(s.notice)
    }

    @Test fun piecesGrowOneAnswer() {
        var s = running()
        s = ChatRules.piece(s, "r1", "Two ")
        s = ChatRules.piece(s, "r1", "new messages.")
        assertEquals(listOf(ChatItem.Mine("What is new?"), ChatItem.Theirs("Two new messages.", streaming = true)), s.items)
    }

    @Test fun latePiecesOfAnOldRunAreDropped() {
        // A run was stopped and the next one began: what the old one still delivers changes nothing.
        var s = ChatRules.stopped(ChatRules.piece(running("old"), "old", "Half"))
        s = ChatRules.sent(s, "Again", "new")
        val before = s
        assertSame(before, ChatRules.piece(s, "old", " an answer"))
        assertSame(before, ChatRules.step(s, "old", step(AiToolKind.SEARCH_MAIL)))
        assertSame(before, ChatRules.finished(s, "old", turn()))
        assertSame(before, ChatRules.failed(s, "old", UiMessage.Res(R.string.chat_failed)))
        // And nothing arrives when nothing runs.
        val idle = ChatRules.stopped(s)
        assertSame(idle, ChatRules.piece(idle, "new", "x"))
        assertSame(idle, ChatRules.step(idle, "new", step(AiToolKind.SEARCH_MAIL)))
    }

    @Test fun aStepClearsTheInterimTextAndJoinsTheStepsBeforeIt() {
        var s = running()
        s = ChatRules.piece(s, "r1", "Let me look.")
        s = ChatRules.step(s, "r1", step(AiToolKind.SEARCH_MAIL, "invoice"))
        assertEquals(listOf(ChatItem.Mine("What is new?"), ChatItem.Steps(listOf(step(AiToolKind.SEARCH_MAIL, "invoice")))), s.items)
        // Text between two tools is interim too, and the second tool joins the group of the first.
        s = ChatRules.piece(s, "r1", "Found it, reading.")
        s = ChatRules.step(s, "r1", step(AiToolKind.READ_MESSAGE, "Invoice 42"))
        assertEquals(2, s.items.size)
        assertEquals(listOf(step(AiToolKind.SEARCH_MAIL, "invoice"), step(AiToolKind.READ_MESSAGE, "Invoice 42")), (s.items[1] as ChatItem.Steps).lines)
    }

    @Test fun theTurnReplacesWhatWasHeardAndBringsChipsCardsAndTheEnd() {
        var s = running()
        s = ChatRules.step(s, "r1", step(AiToolKind.SEARCH_MAIL))
        s = ChatRules.piece(s, "r1", "Two new mess")
        val link = AiLink.Message(ref("m1"))
        val archive = proposal("p1", AiChange.Archive(refs(3)))
        s = ChatRules.finished(s, "r1", turn("Two new messages.", listOf(archive), listOf(link), steps = 30u, end = AiChatEnd.STEP_LIMIT))
        assertEquals(
            listOf(
                ChatItem.Mine("What is new?"),
                ChatItem.Steps(listOf(step(AiToolKind.SEARCH_MAIL))),
                ChatItem.Theirs("Two new messages."),
                ChatItem.Chip(link),
                ChatItem.Card(archive, CardState.Pending),
                ChatItem.Notice(ChatNote.StepLimit(30)),
            ),
            s.items,
        )
        assertFalse(s.running)
        assertNull(s.runId)
    }

    @Test fun theEndIsNamed() {
        fun last(turn: AiChatTurn) = ChatRules.finished(running(), "r1", turn).items.last()
        assertEquals(ChatItem.Theirs("Done."), last(turn()))
        assertEquals(ChatItem.Notice(ChatNote.SizeLimit), last(turn(end = AiChatEnd.SIZE_LIMIT)))
        assertEquals(ChatItem.Notice(ChatNote.StepLimit(30)), last(turn(text = "", steps = 30u, end = AiChatEnd.STEP_LIMIT)))
        // An empty answer with nothing else is said, not left as silence.
        assertEquals(ChatItem.Notice(ChatNote.NoAnswer), last(turn(text = " ")))
        // A card alone is an answer.
        val card = proposal("p1", AiChange.Archive(refs(1)))
        assertEquals(ChatItem.Card(card, CardState.Pending), last(turn(text = "", proposals = listOf(card))))
    }

    @Test fun thatTheModelCannotLookThingsUpIsSaidOnce() {
        var s = ChatRules.finished(running(), "r1", turn(toolsOff = true))
        assertTrue(s.toolsOff)
        assertEquals(ChatItem.Notice(ChatNote.ToolsOff), s.items.last())
        s = ChatRules.finished(ChatRules.sent(s, "And?", "r2"), "r2", turn(toolsOff = true))
        assertTrue(s.toolsOff)
        assertEquals(1, s.items.count { it == ChatItem.Notice(ChatNote.ToolsOff) })
    }

    @Test fun aStopKeepsWhatCameAndMarksIt() {
        var s = ChatRules.piece(running(), "r1", "Half an")
        s = ChatRules.stopped(s)
        assertEquals(ChatItem.Theirs("Half an", streaming = false, stopped = true), s.items.last())
        assertFalse(s.running)
        assertNull(s.runId)
        // Nothing heard yet: the stop is still shown.
        assertEquals(ChatItem.Theirs("", stopped = true), ChatRules.stopped(running()).items.last())
        // A stop when nothing runs does nothing.
        assertSame(ready, ChatRules.stopped(ready))
    }

    @Test fun aFailedSendTakesItsItemsOutAgain() {
        val before = ready.copy(items = listOf(ChatItem.Mine("One"), ChatItem.Theirs("Answer one.")))
        var s = ChatRules.sent(before, "Two", "r2")
        s = ChatRules.step(s, "r2", step(AiToolKind.SEARCH_MAIL))
        s = ChatRules.piece(s, "r2", "Half")
        s = ChatRules.failed(s, "r2", UiMessage.Res(R.string.chat_busy))
        assertEquals(before.items, s.items)
        assertFalse(s.running)
        assertEquals(UiMessage.Res(R.string.chat_busy), s.notice)
    }

    @Test fun aNewChatKeepsNothingButTheGate() {
        var s = ChatRules.finished(running(), "r1", turn(toolsOff = true)).copy(host = "api.openai.com", notice = UiMessage.Res(R.string.chat_failed))
        s = ChatRules.sent(s, "More", "r2")
        val fresh = ChatRules.cleared(s, restarted = false)
        assertEquals(ChatState(gate = Gate.READY, host = "api.openai.com"), fresh)
        assertEquals(listOf(ChatItem.Notice(ChatNote.Restarted)), ChatRules.cleared(s, restarted = true).items)
    }

    // ---- errors ----

    @Test fun theCoresRefusalsAreToldApart() {
        assertEquals(ChatRules.Failure.CANCELLED, ChatRules.failure(CoreException.Invalid("cancelled")))
        assertEquals(ChatRules.Failure.BUSY, ChatRules.failure(CoreException.Invalid("the assistant is already working on something")))
        assertEquals(ChatRules.Failure.OVER, ChatRules.failure(CoreException.Invalid("this conversation is over")))
        assertEquals(ChatRules.Failure.OTHER, ChatRules.failure(CoreException.Invalid("write something first")))
        // Only a refusal counts: the same words in another kind of error are not one of these.
        assertEquals(ChatRules.Failure.OTHER, ChatRules.failure(CoreException.Temporary("cancelled")))
        assertEquals(ChatRules.Failure.OTHER, ChatRules.failure(IllegalStateException("cancelled")))
    }

    @Test fun errorsBecomeMessages() {
        assertEquals(UiMessage.Res(R.string.chat_busy), ChatRules.message(CoreException.Invalid("the assistant is already working on something")))
        // The core's own words for a refusal and for trouble on the way are shown as they are.
        assertEquals(UiMessage.Text("that message is too long"), ChatRules.message(CoreException.Invalid("that message is too long")))
        assertEquals(UiMessage.Text("the service did not answer"), ChatRules.message(CoreException.Temporary("the service did not answer")))
        assertEquals(UiMessage.Res(R.string.assistant_sign_in_needed), ChatRules.message(CoreException.SignInNeeded("x")))
        // Anything else is the fallback: nothing of the error itself is shown.
        assertEquals(UiMessage.Res(R.string.chat_failed), ChatRules.message(IllegalStateException("secret")))
        assertEquals(UiMessage.Res(R.string.chat_undo_failed), ChatRules.message(IllegalStateException("secret"), R.string.chat_undo_failed))
    }

    // ---- the card ----

    private val archive = AiChange.Archive(refs(3))
    private val addEvent = AiChange.AddEvent(event(), "Private")

    @Test fun onlyAWaitingCardCanBeApprovedOrDeclined() {
        assertEquals(CardState.Applying, ChatRules.approve(CardState.Pending))
        assertEquals(CardState.Declined, ChatRules.decline(CardState.Pending))
        val closed = listOf(
            CardState.Applying,
            CardState.Applied(canUndo = true),
            CardState.Undoing,
            CardState.Undone,
            CardState.Declined,
            CardState.Failed(UiMessage.Res(R.string.chat_apply_failed), canUndo = true),
        )
        for (state in closed) {
            assertNull("$state", ChatRules.approve(state))
            assertNull("$state", ChatRules.decline(state))
        }
    }

    @Test fun anAppliedCardOffersUndoWhenTheCoreSaysSo() {
        assertEquals(CardState.Applied(canUndo = true), ChatRules.applied(AiApplied(true, null)))
        assertEquals(CardState.Applied(canUndo = false), ChatRules.applied(AiApplied(false, "e1")))
        assertEquals(CardState.Undoing, ChatRules.undo(CardState.Applied(canUndo = true)))
        assertNull(ChatRules.undo(CardState.Applied(canUndo = false)))
        for (state in listOf(CardState.Pending, CardState.Applying, CardState.Undoing, CardState.Undone, CardState.Declined)) assertNull("$state", ChatRules.undo(state))
    }

    @Test fun aFailedMailChangeIsClosedAndKeepsItsUndo() {
        val failed = ChatRules.applyFailed(archive, CoreException.Temporary("the server did not answer"))
        assertEquals(CardState.Failed(UiMessage.Text("the server did not answer"), canUndo = true, canRetry = false), failed)
        assertNull(ChatRules.approve(failed))
        assertNull(ChatRules.decline(failed))
        assertEquals(CardState.Undoing, ChatRules.undo(failed))
    }

    @Test fun aFailedEventSaveStaysOpenForAnotherTry() {
        val failed = ChatRules.applyFailed(addEvent, CoreException.Temporary("offline"))
        assertEquals(CardState.Failed(UiMessage.Text("offline"), canUndo = false, canRetry = true), failed)
        assertEquals(CardState.Applying, ChatRules.approve(failed))
        assertEquals(CardState.Declined, ChatRules.decline(failed))
        assertNull(ChatRules.undo(failed))
    }

    @Test fun aRefusedSuggestionIsClosedWithNothingToUndo() {
        for (change in listOf(archive, addEvent)) {
            val failed = ChatRules.applyFailed(change, CoreException.Invalid("this suggestion is no longer there"))
            assertEquals(CardState.Failed(UiMessage.Text("this suggestion is no longer there"), canUndo = false, canRetry = false), failed)
            assertNull(ChatRules.approve(failed))
            assertNull(ChatRules.undo(failed))
        }
    }

    @Test fun aFailedUndoCanBeTriedAgainUnlessItWasRefused() {
        val again = ChatRules.undoFailed(CoreException.Temporary("offline"))
        assertEquals(CardState.Failed(UiMessage.Text("offline"), canUndo = true), again)
        assertEquals(CardState.Undoing, ChatRules.undo(again))
        val gone = ChatRules.undoFailed(CoreException.Invalid("this can no longer be undone"))
        assertEquals(CardState.Failed(UiMessage.Text("this can no longer be undone"), canUndo = false), gone)
        assertNull(ChatRules.undo(gone))
        assertEquals(CardState.Failed(UiMessage.Res(R.string.chat_undo_failed), canUndo = true), ChatRules.undoFailed(IllegalStateException("x")))
    }

    @Test fun theFullCardShowsOnlyWhileItWaitsOrIsBeingDone() {
        assertTrue(ChatRules.isOpen(CardState.Pending))
        assertTrue(ChatRules.isOpen(CardState.Applying))
        for (state in listOf(CardState.Applied(true), CardState.Undoing, CardState.Undone, CardState.Declined, CardState.Failed(UiMessage.Res(R.string.chat_apply_failed), false))) {
            assertFalse("$state", ChatRules.isOpen(state))
        }
    }

    @Test fun aCardIsFoundAndChangedByItsId() {
        val one = proposal("p1", archive)
        val two = proposal("p2", addEvent)
        val s = ChatRules.finished(running(), "r1", turn(proposals = listOf(one, two)))
        val next = ChatRules.withCard(s, "p2", CardState.Applying)
        assertEquals(CardState.Pending, ChatRules.card(next, "p1")?.state)
        assertEquals(CardState.Applying, ChatRules.card(next, "p2")?.state)
        // A card that is gone (a new chat began) changes nothing.
        assertNull(ChatRules.card(next, "p3"))
        assertEquals(next, ChatRules.withCard(next, "p3", CardState.Declined))
    }

    @Test fun titlesAndPlurals() {
        assertEquals("Archive 3 messages?", ChatRules.title(archive, Words))
        assertEquals("Archive 1 message?", ChatRules.title(AiChange.Archive(refs(1)), Words))
        assertEquals("Move 2 messages to Bills?", ChatRules.title(AiChange.Move(refs(2), "mb1", "Bills"), Words))
        assertEquals("Mark 4 messages as read?", ChatRules.title(AiChange.MarkRead(refs(4), true), Words))
        assertEquals("Mark 4 messages as unread?", ChatRules.title(AiChange.MarkRead(refs(4), false), Words))
        assertEquals("Star 2 messages?", ChatRules.title(AiChange.Star(refs(2), true), Words))
        assertEquals("Remove the star from 1 message?", ChatRules.title(AiChange.Star(refs(1), false), Words))
        assertEquals("Tag 1 message with Travel?", ChatRules.title(AiChange.Tag(refs(1), "l1", "Travel", true), Words))
        assertEquals("Remove Travel from 2 messages?", ChatRules.title(AiChange.Tag(refs(2), "l1", "Travel", false), Words))
        assertEquals("Add this event?", ChatRules.title(addEvent, Words))
        assertEquals("Add this task?", ChatRules.title(AiChange.AddEvent(event(isTask = true), "Private"), Words))
    }

    @Test fun whatWasDoneIsSaidInThePast() {
        assertEquals("Archived 3 messages", ChatRules.doneLine(archive, Words))
        assertEquals("Moved 1 message to Bills", ChatRules.doneLine(AiChange.Move(refs(1), "mb1", "Bills"), Words))
        assertEquals("Added the event", ChatRules.doneLine(addEvent, Words))
    }

    @Test fun everyMessageOfAChangeIsListed() {
        val many = refs(50)
        for (change in listOf(AiChange.Archive(many), AiChange.Move(many, "mb1", "Bills"), AiChange.MarkRead(many, true), AiChange.Star(many, true), AiChange.Tag(many, "l1", "Travel", true))) {
            assertEquals(many, ChatRules.messages(change))
        }
        assertTrue(ChatRules.messages(addEvent).isEmpty())
        assertFalse(ChatRules.scrollsInside(5))
        assertTrue(ChatRules.scrollsInside(6))
    }

    @Test fun onlyAConfirmChangeWarns() {
        assertNull(ChatRules.warning(proposal("p1", archive, AiRisk.CHANGE), Words))
        assertEquals("This changes 12 messages at once.", ChatRules.warning(proposal("p2", AiChange.Archive(refs(12)), AiRisk.CONFIRM), Words))
    }

    // ---- the fields of an event ----

    private val locale = Locale.UK
    private val day = DateTimeFormatter.ofLocalizedDate(FormatStyle.FULL).withLocale(locale)
    private val time = DateTimeFormatter.ofLocalizedTime(FormatStyle.SHORT).withLocale(locale)
    private val start = LocalDateTime.parse("2026-11-12T10:00:00")

    @Test fun anEventSaysDayStartAndEnd() {
        assertEquals("${day.format(start)}, ${time.format(start)} – ${time.format(start.plusHours(1))}", ChatRules.eventWhen(event(), locale, "Europe/Berlin", Words))
        // Over midnight the end names its day.
        val late = start.plusHours(20)
        assertEquals(
            "${day.format(start)}, ${time.format(start)} – ${day.format(late)}, ${time.format(late)}",
            ChatRules.eventWhen(event(duration = 72_000u), locale, "Europe/Berlin", Words),
        )
        // No length: the start alone.
        assertEquals("${day.format(start)}, ${time.format(start)}", ChatRules.eventWhen(event(duration = 0u), locale, "Europe/Berlin", Words))
    }

    @Test fun aZoneOtherThanThePhonesIsNamed() {
        val text = ChatRules.eventWhen(event(zone = "America/New_York"), locale, "Europe/Berlin", Words)
        assertTrue(text, text.endsWith(" (America/New_York)"))
        assertFalse(ChatRules.eventWhen(event(zone = null), locale, "Europe/Berlin", Words).contains("("))
    }

    @Test fun allDayEntriesNameTheirDays() {
        val midnight = LocalDateTime.parse("2026-11-12T00:00:00")
        assertEquals("${day.format(midnight)}, all day", ChatRules.eventWhen(event("2026-11-12T00:00:00", duration = 86_400u, allDay = true), locale, "Europe/Berlin", Words))
        assertEquals(
            "${day.format(midnight)} – ${day.format(midnight.plusDays(2))}, all day",
            ChatRules.eventWhen(event("2026-11-12T00:00:00", duration = 259_200u, allDay = true), locale, "Europe/Berlin", Words),
        )
    }

    @Test fun aTaskSaysWhenItIsDueAndAStartThatCannotBeReadIsShownAsItIs() {
        assertEquals("${day.format(start)}, ${time.format(start)}", ChatRules.eventWhen(event(duration = 0u, isTask = true), locale, "Europe/Berlin", Words))
        assertEquals("soon", ChatRules.eventWhen(event(start = "soon"), locale, "Europe/Berlin", Words))
    }

    @Test fun everyFieldOfTheEventIsALine() {
        val full = event(location = "Main Street 1", description = "Bring the card.")
        val lines = ChatRules.eventLines(full, "Private", locale, "Europe/Berlin", Words)
        assertEquals(listOf(EventField.TITLE, EventField.WHEN, EventField.CALENDAR, EventField.PLACE, EventField.DESCRIPTION), lines.map { it.field })
        assertEquals(listOf("Dentist", ChatRules.eventWhen(full, locale, "Europe/Berlin", Words), "Private", "Main Street 1", "Bring the card."), lines.map { it.value })
        // Empty fields have no line; a task is due.
        assertEquals(
            listOf(EventField.TITLE, EventField.DUE, EventField.CALENDAR),
            ChatRules.eventLines(event(isTask = true, duration = 0u), "Private", locale, "Europe/Berlin", Words).map { it.field },
        )
        // The card is the record: reminders, if the record has any, are shown.
        val alerts = ChatRules.eventLines(event().copy(alertMinutes = listOf(15u, 60u)), "Private", locale, "Europe/Berlin", Words)
        assertEquals(ChatRules.EventLine(EventField.REMINDERS, "15 minutes before, 60 minutes before"), alerts.last())
    }

    // ---- steps ----

    @Test fun everyToolHasAStepLine() {
        assertEquals(StepKind.SEARCH, ChatRules.stepKind(AiToolKind.SEARCH_MAIL))
        assertEquals(StepKind.EVENTS, ChatRules.stepKind(AiToolKind.LIST_EVENTS))
        assertEquals(StepKind.DRAFT, ChatRules.stepKind(AiToolKind.DRAFT_REPLY))
        for (tool in listOf(AiToolKind.ARCHIVE, AiToolKind.MOVE, AiToolKind.MARK, AiToolKind.TAG, AiToolKind.ADD_EVENT)) assertEquals(StepKind.SUGGEST, ChatRules.stepKind(tool))
        // Every tool the core has today has words of its own.
        for (tool in AiToolKind.entries) assertFalse("$tool", ChatRules.stepKind(tool) == StepKind.OTHER)
    }

    @Test fun aStepLineCarriesItsDetailOnOneLine() {
        assertEquals("Searching mail: invoice", ChatRules.stepLine(step(AiToolKind.SEARCH_MAIL, "invoice"), Words))
        assertEquals("Reading a message", ChatRules.stepLine(step(AiToolKind.READ_MESSAGE, "  "), Words))
        assertEquals("Searching mail: two lines", ChatRules.stepLine(step(AiToolKind.SEARCH_MAIL, "two\nlines"), Words))
    }

    // ---- layout ----

    @Test fun chipsThatFollowEachOtherAreOneGroup() {
        val chip = ChatItem.Chip(AiLink.Message(ref("m1")))
        val items = listOf(ChatItem.Mine("q"), chip, chip, chip, ChatItem.Theirs("a"), chip)
        assertEquals(0 to 1, ChatRules.chipRun(items, 0))
        assertEquals(0 to 3, ChatRules.chipRun(items, 1))
        assertEquals(1 to 3, ChatRules.chipRun(items, 2))
        assertEquals(2 to 3, ChatRules.chipRun(items, 3))
        assertEquals(0 to 1, ChatRules.chipRun(items, 4))
        assertEquals(0 to 1, ChatRules.chipRun(items, 5))
    }

    @Test
    fun a_card_the_core_applied_by_itself_is_done_with_its_undo_and_one_it_could_not_is_failed_or_waiting() {
        val one = proposal("c:1", AiChange.Archive(refs(1)))
        val two = proposal("c:2", AiChange.MarkRead(refs(1), true))
        val three = proposal("c:3", AiChange.Star(refs(1), true))
        val four = proposal("c:4", AiChange.Archive(refs(2)))
        val results = listOf(
            AiAutoResult("c:1", applied = true, canUndo = true, leftOpen = false, message = ""),
            AiAutoResult("c:2", applied = false, canUndo = false, leftOpen = false, message = "one of these messages is no longer there"),
            AiAutoResult("c:3", applied = false, canUndo = false, leftOpen = true, message = "waits"),
        )
        val after = ChatRules.finished(running(), "r1", turn("Done.", proposals = listOf(one, two, three, four), automatic = results))
        val cards = after.items.filterIsInstance<ChatItem.Card>().map { it.state }
        assertEquals(CardState.Applied(canUndo = true, automatically = true), cards[0])
        assertEquals(CardState.Failed(UiMessage.Text("one of these messages is no longer there"), canUndo = true), cards[1])
        // Left for a tap, and one the core did not touch at all, both wait as in ask mode.
        assertEquals(CardState.Pending, cards[2])
        assertEquals(CardState.Pending, cards[3])
        assertNotNull(ChatRules.undo(cards[0]))
    }

    @Test
    fun the_rest_is_offered_on_a_waiting_card_only_while_automatic_changes_are_off() {
        val plain = ChatItem.Card(proposal("c:1", AiChange.Archive(refs(1))), CardState.Pending)
        assertTrue(ChatRules.canAcceptRest(plain, auto = false))
        assertFalse(ChatRules.canAcceptRest(plain, auto = true))
        assertFalse(ChatRules.canAcceptRest(plain.copy(state = CardState.Applying), auto = false))
        // A change the card warns about (many messages) always asks by itself.
        val warned = ChatItem.Card(proposal("c:2", AiChange.Archive(refs(11)), AiRisk.CONFIRM), CardState.Pending)
        assertFalse(ChatRules.canAcceptRest(warned, auto = false))
    }

    @Test
    fun the_conversation_chip_can_be_removed_until_the_first_message_and_a_new_chat_clears_it_and_the_switch() {
        val about = AiAbout("t1", "Lunch", 2u, false)
        val chat = ready.copy(about = about, auto = true)
        assertTrue(ChatRules.aboutRemovable(chat))
        assertFalse(ChatRules.aboutRemovable(ready))
        assertFalse(ChatRules.aboutRemovable(ChatRules.sent(chat, "hi", "r1")))
        val fresh = ChatRules.cleared(chat, restarted = false)
        assertNull(fresh.about)
        assertFalse(fresh.auto)
    }
}
