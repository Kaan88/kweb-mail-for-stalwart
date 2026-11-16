package ovh.kweb.mail.ui.assistant.chat

import java.time.LocalDateTime
import java.time.format.DateTimeFormatter
import java.time.format.DateTimeParseException
import java.time.format.FormatStyle
import java.util.Locale
import ovh.kweb.mail.R
import ovh.kweb.mail.ui.UiMessage
import ovh.kweb.mail.ui.assistant.AssistantMessages
import ovh.kweb.mail.ui.assistant.AssistantRules.ChatGate
import uniffi.kweb_core.AiAbout
import uniffi.kweb_core.AiApplied
import uniffi.kweb_core.AiAutoResult
import uniffi.kweb_core.AiChange
import uniffi.kweb_core.AiChatEnd
import uniffi.kweb_core.AiChatTurn
import uniffi.kweb_core.AiLink
import uniffi.kweb_core.AiMessageRef
import uniffi.kweb_core.AiProposal
import uniffi.kweb_core.AiRisk
import uniffi.kweb_core.AiSettings
import uniffi.kweb_core.AiStep
import uniffi.kweb_core.AiToolKind
import uniffi.kweb_core.EventInput
import uniffi.kweb_core.Exception as CoreException

/** Where an approval card stands. Nothing is done before [Applying], and that only follows the user's tap. */
sealed interface CardState {
    /** Waiting for the user: Approve or Don't. */
    data object Pending : CardState

    data object Applying : CardState

    /** [automatically]: the core did it because the user switched automatic changes on, not on a tap. */
    data class Applied(val canUndo: Boolean, val automatically: Boolean = false) : CardState

    data object Undoing : CardState

    data object Undone : CardState

    data object Declined : CardState

    /**
     * Doing it (or taking it back) failed. [canUndo]: something may have been done and can be taken back.
     * [canRetry]: the suggestion is still open in the core (an event that could not be saved).
     */
    data class Failed(val message: UiMessage, val canUndo: Boolean, val canRetry: Boolean = false) : CardState
}

/** A quiet line in the conversation that is not the assistant's text. */
sealed interface ChatNote {
    data class StepLimit(val steps: Int) : ChatNote

    data object SizeLimit : ChatNote

    /** The service takes no tools: the chat answers from the conversation only. */
    data object ToolsOff : ChatNote

    /** The core had dropped the conversation (the service or model changed): this is a new one. */
    data object Restarted : ChatNote

    /** The answer was empty and nothing came with it. */
    data object NoAnswer : ChatNote

    /** An earlier chat held with another service or model than the ones set now: the next message goes to the current ones. */
    data class HeldWith(val service: String, val model: String) : ChatNote
}

sealed interface ChatItem {
    data class Mine(val text: String) : ChatItem

    /** The assistant's text. [streaming]: still being written. [stopped]: the user stopped it here. */
    data class Theirs(val text: String, val streaming: Boolean = false, val stopped: Boolean = false) : ChatItem

    /** The tools used in a row, drawn as one group. */
    data class Steps(val lines: List<AiStep>) : ChatItem

    data class Card(val proposal: AiProposal, val state: CardState) : ChatItem

    /** Something the user may open by tapping; nothing opens by itself. */
    data class Chip(val link: AiLink) : ChatItem

    data class Notice(val note: ChatNote) : ChatItem
}

/**
 * What the chat screen shows. What the user is typing is not here: it lives in the view model as Compose state (a
 * text field fed through a flow loses the cursor).
 */
data class ChatState(
    /** Whether the chat may be used; null until the settings were read. */
    val gate: ChatGate? = null,
    /** The settings could not be read. */
    val settingsFailed: Boolean = false,
    /** The service the chat sends to, as the empty state names it. */
    val host: String = "",
    val items: List<ChatItem> = emptyList(),
    val running: Boolean = false,
    /** The run a Stop names; pieces for any other run are dropped. */
    val runId: String? = null,
    /** Where the items of the running send begin (its own message), to take them out again when it fails. */
    val runStart: Int = 0,
    val notice: UiMessage? = null,
    val toolsOff: Boolean = false,
    /** A reply draft is being made from a chip. */
    val draftBusy: Boolean = false,
    /** The conversation this chat was opened about (the chip "About: ..."), until it is removed or a new chat begins. */
    val about: AiAbout? = null,
    /** The settings let the chat suggest changes ("Ask first"); with them off there is nothing to make automatic. */
    val actionsOn: Boolean = true,
    /** Automatic changes are on for this chat (the user switched them on; the core holds the real flag). */
    val auto: Boolean = false,
)

/** Every decision of the chat screen. Plain Kotlin: nothing here needs Android or talks to the core. */
object ChatRules {
    // The core's own sentences this screen tells apart (core/src/assistant/mod.rs, core/src/ffi/ai_chat.rs).
    const val CANCELLED = "cancelled"
    const val BUSY = "the assistant is already working on something"
    const val OVER = "this conversation is over"

    /** More messages than this in one card and the list scrolls inside the card. */
    const val CARD_ROWS = 5

    // ---- the gate ----

    /** The mail list offers the chat when the assistant and the chat are on (the chat screen asks for the rest). */
    fun entryShown(settings: AiSettings?): Boolean = settings != null && settings.enabled && settings.allowChat

    /** Send is offered for a text that is not blank, when the chat may be used and nothing is running. */
    fun canSend(gate: ChatGate?, running: Boolean, input: String): Boolean = gate == ChatGate.READY && !running && input.isNotBlank()

    // ---- one send ----

    /** The user's message goes into the list and the run [runId] begins. */
    fun sent(state: ChatState, text: String, runId: String): ChatState = state.copy(
        items = state.items + ChatItem.Mine(text),
        running = true,
        runId = runId,
        runStart = state.items.size,
        notice = null,
    )

    private fun isCurrent(state: ChatState, runId: String): Boolean = state.running && state.runId == runId

    private fun withoutInterim(items: List<ChatItem>): List<ChatItem> {
        val last = items.lastOrNull()
        return if (last is ChatItem.Theirs && last.streaming) items.dropLast(1) else items
    }

    /** The next piece of text of [runId]. A piece of an older run, or one after a Stop, is dropped. */
    fun piece(state: ChatState, runId: String, piece: String): ChatState {
        if (!isCurrent(state, runId) || piece.isEmpty()) return state
        val last = state.items.lastOrNull()
        val items = if (last is ChatItem.Theirs && last.streaming) {
            state.items.dropLast(1) + last.copy(text = last.text + piece)
        } else {
            state.items + ChatItem.Theirs(piece, streaming = true)
        }
        return state.copy(items = items)
    }

    /** A tool is about to be used: the text heard so far was interim and goes; the step joins the steps before it. */
    fun step(state: ChatState, runId: String, step: AiStep): ChatState {
        if (!isCurrent(state, runId)) return state
        val items = withoutInterim(state.items)
        val last = items.lastOrNull()
        return state.copy(
            items = if (last is ChatItem.Steps) items.dropLast(1) + ChatItem.Steps(last.lines + step) else items + ChatItem.Steps(listOf(step)),
        )
    }

    /**
     * The send is complete: the turn's text replaces what was heard, then come the things to open, the suggestions
     * (each waiting for the user) and a line that says why it ended, if not with an answer.
     */
    fun finished(state: ChatState, runId: String, turn: AiChatTurn): ChatState {
        if (!isCurrent(state, runId)) return state
        val items = withoutInterim(state.items).toMutableList()
        if (turn.text.isNotBlank()) items += ChatItem.Theirs(turn.text)
        turn.links.forEach { items += ChatItem.Chip(it) }
        val automatic = turn.automatic.associateBy { it.proposalId }
        turn.proposals.forEach { items += ChatItem.Card(it, cardAfterSend(automatic[it.id])) }
        when (turn.end) {
            AiChatEnd.ANSWERED -> if (turn.text.isBlank() && turn.links.isEmpty() && turn.proposals.isEmpty()) items += ChatItem.Notice(ChatNote.NoAnswer)
            AiChatEnd.STEP_LIMIT -> items += ChatItem.Notice(ChatNote.StepLimit(turn.steps.toInt()))
            AiChatEnd.SIZE_LIMIT -> items += ChatItem.Notice(ChatNote.SizeLimit)
        }
        // Said once, when it is first seen.
        if (turn.toolsOff && !state.toolsOff) items += ChatItem.Notice(ChatNote.ToolsOff)
        return state.copy(items = items, running = false, runId = null, toolsOff = state.toolsOff || turn.toolsOff)
    }

    /**
     * Where a new card stands: waiting, unless the core applied it by itself (done, with an Undo where there is one) or
     * tried to and could not (closed, failed) or left it for a tap (still waiting).
     */
    fun cardAfterSend(result: AiAutoResult?): CardState = when {
        result == null -> CardState.Pending
        result.applied -> CardState.Applied(result.canUndo, automatically = true)
        result.leftOpen -> CardState.Pending
        else -> CardState.Failed(UiMessage.Text(result.message), canUndo = true)
    }

    /**
     * Whether a waiting card offers "Accept, and accept the rest in this chat". It mirrors the core's list of changes
     * that may become automatic (the core decides, and checks again): not a card that warns about many messages, and
     * not while automatic changes are already on.
     */
    fun canAcceptRest(card: ChatItem.Card, auto: Boolean): Boolean =
        !auto && card.state == CardState.Pending && card.proposal.risk == AiRisk.CHANGE

    /** The chip can be taken away until the first message went out: after it the text has been sent. */
    fun aboutRemovable(state: ChatState): Boolean = state.about != null && state.items.none { it is ChatItem.Mine }

    /** The user stopped the run: what came so far stays, marked as stopped, and whatever still arrives is dropped. */
    fun stopped(state: ChatState): ChatState {
        if (!state.running) return state
        val last = state.items.lastOrNull()
        val items = if (last is ChatItem.Theirs && last.streaming) {
            state.items.dropLast(1) + last.copy(streaming = false, stopped = true)
        } else {
            state.items + ChatItem.Theirs("", stopped = true)
        }
        return state.copy(items = items, running = false, runId = null)
    }

    /**
     * The send failed. The core left the conversation as it was before, so the items of this run (the user's message
     * too) go and [message] says why; the caller puts the text back into the input. A failure of an older or a
     * stopped run changes nothing.
     */
    fun failed(state: ChatState, runId: String, message: UiMessage): ChatState {
        if (!isCurrent(state, runId)) return state
        return state.copy(items = state.items.take(state.runStart), running = false, runId = null, notice = message)
    }

    /** A new chat: nothing of the old one stays. [restarted]: the core had dropped the old one, and the list says so. */
    fun cleared(state: ChatState, restarted: Boolean): ChatState = state.copy(
        items = if (restarted) listOf(ChatItem.Notice(ChatNote.Restarted)) else emptyList(),
        running = false,
        runId = null,
        runStart = 0,
        notice = null,
        toolsOff = false,
        draftBusy = false,
        about = null,
        auto = false,
    )

    /** An earlier chat opened again: its words are the list, with a note when it was held with another service or model. */
    fun resumed(state: ChatState, opened: uniffi.kweb_core.AiChatOpened): ChatState = state.copy(
        items = opened.messages.map { if (it.fromUser) ChatItem.Mine(it.text) else ChatItem.Theirs(it.text) } +
            (if (opened.serviceChanged) listOf(ChatItem.Notice(ChatNote.HeldWith(opened.service, opened.model))) else emptyList()),
        running = false,
        runId = null,
        runStart = 0,
        notice = null,
        toolsOff = false,
        draftBusy = false,
        about = null,
        auto = false,
    )

    // ---- errors ----

    enum class Failure { CANCELLED, BUSY, OVER, OTHER }

    /** Which of the core's refusals [error] is. */
    fun failure(error: Throwable): Failure {
        val words = (error as? CoreException.Invalid)?.msg ?: return Failure.OTHER
        return when (words) {
            CANCELLED -> Failure.CANCELLED
            BUSY -> Failure.BUSY
            OVER -> Failure.OVER
            else -> Failure.OTHER
        }
    }

    /** What to tell the user about [error]: the core's single slot is taken, or what the assistant's other screens say. */
    fun message(error: Throwable, fallback: Int = R.string.chat_failed): UiMessage =
        if (failure(error) == Failure.BUSY) UiMessage.Res(R.string.chat_busy) else AssistantMessages.of(error, fallback)

    // ---- the approval card ----

    /** "Approve" (or "Try again" on a suggestion that is still open): the next state, or null when the tap does nothing. */
    fun approve(state: CardState): CardState? = when {
        state == CardState.Pending -> CardState.Applying
        state is CardState.Failed && state.canRetry -> CardState.Applying
        else -> null
    }

    /** "Don't": only a waiting card (or one still open after a failure) can be declined. */
    fun decline(state: CardState): CardState? = when {
        state == CardState.Pending -> CardState.Declined
        state is CardState.Failed && state.canRetry -> CardState.Declined
        else -> null
    }

    fun applied(result: AiApplied): CardState = CardState.Applied(result.canUndo)

    /**
     * Applying failed. A refusal (Invalid) happens before anything is done and closes the card. Otherwise a mail change
     * is closed by the core, which keeps an Undo for the part that was done; an event that could not be saved stays
     * open for another try.
     */
    fun applyFailed(change: AiChange, error: Throwable): CardState {
        val refused = error is CoreException.Invalid
        val event = change is AiChange.AddEvent
        return CardState.Failed(message(error, R.string.chat_apply_failed), canUndo = !refused && !event, canRetry = !refused && event)
    }

    /** "Undo": only where something was done that can be taken back. */
    fun undo(state: CardState): CardState? = when {
        state is CardState.Applied && state.canUndo -> CardState.Undoing
        state is CardState.Failed && state.canUndo -> CardState.Undoing
        else -> null
    }

    /** Taking it back failed. A refusal means there is nothing left to undo; anything else may be tried again. */
    fun undoFailed(error: Throwable): CardState = CardState.Failed(message(error, R.string.chat_undo_failed), canUndo = error !is CoreException.Invalid)

    /** Whether the card is still the full card with its list and buttons (afterwards it is one row). */
    fun isOpen(state: CardState): Boolean = state == CardState.Pending || state == CardState.Applying

    /** The items with the card of [proposalId] in [next]; unchanged when it is not there (a new chat began). */
    fun withCard(state: ChatState, proposalId: String, next: CardState): ChatState =
        state.copy(items = state.items.map { if (it is ChatItem.Card && it.proposal.id == proposalId) it.copy(state = next) else it })

    fun card(state: ChatState, proposalId: String): ChatItem.Card? =
        state.items.firstOrNull { it is ChatItem.Card && it.proposal.id == proposalId } as? ChatItem.Card

    // ---- what a card says ----

    enum class ChangeKind { ARCHIVE, MOVE, MARK_READ, MARK_UNREAD, STAR, UNSTAR, TAG, UNTAG, ADD_EVENT, ADD_TASK }

    fun kind(change: AiChange): ChangeKind = when (change) {
        is AiChange.Archive -> ChangeKind.ARCHIVE
        is AiChange.Move -> ChangeKind.MOVE
        is AiChange.MarkRead -> if (change.read) ChangeKind.MARK_READ else ChangeKind.MARK_UNREAD
        is AiChange.Star -> if (change.on) ChangeKind.STAR else ChangeKind.UNSTAR
        is AiChange.Tag -> if (change.on) ChangeKind.TAG else ChangeKind.UNTAG
        is AiChange.AddEvent -> if (change.input.isTask) ChangeKind.ADD_TASK else ChangeKind.ADD_EVENT
    }

    /** Every message the change touches, in the core's order; none for an event. */
    fun messages(change: AiChange): List<AiMessageRef> = when (change) {
        is AiChange.Archive -> change.messages
        is AiChange.Move -> change.messages
        is AiChange.MarkRead -> change.messages
        is AiChange.Star -> change.messages
        is AiChange.Tag -> change.messages
        is AiChange.AddEvent -> emptyList()
    }

    /** The folder or label the change names, empty when it names none. */
    private fun target(change: AiChange): String = when (change) {
        is AiChange.Move -> change.mailboxName
        is AiChange.Tag -> change.labelName
        else -> ""
    }

    /** "Archive 3 messages?", "Move 2 messages to Bills?", "Add this event?". */
    fun title(change: AiChange, words: ChatWords): String = words.title(kind(change), messages(change).size, target(change))

    /** "Archived 3 messages": the row a card becomes once it was done. */
    fun doneLine(change: AiChange, words: ChatWords): String = words.done(kind(change), messages(change).size, target(change))

    /** The warning line of a change the core classed as Confirm (many messages at once); none otherwise. */
    fun warning(proposal: AiProposal, words: ChatWords): String? =
        if (proposal.risk == AiRisk.CONFIRM) words.warning(messages(proposal.change).size) else null

    /** Whether the message list scrolls inside the card. */
    fun scrollsInside(count: Int): Boolean = count > CARD_ROWS

    // ---- the fields of an event ----

    enum class EventField { TITLE, WHEN, DUE, CALENDAR, PLACE, DESCRIPTION, REMINDERS, REPEATS }

    data class EventLine(val field: EventField, val value: String)

    /**
     * Every field of the entry a card would add, in the order it is read; a field that is empty has no line. The core
     * adds neither reminders nor a repeat rule, but if the record carries one it is shown: the card is the record.
     */
    fun eventLines(input: EventInput, calendarName: String, locale: Locale, phoneZone: String, words: ChatWords): List<EventLine> = buildList {
        add(EventLine(EventField.TITLE, input.title))
        add(EventLine(if (input.isTask) EventField.DUE else EventField.WHEN, eventWhen(input, locale, phoneZone, words)))
        add(EventLine(EventField.CALENDAR, calendarName))
        if (input.location.isNotBlank()) add(EventLine(EventField.PLACE, input.location))
        if (input.description.isNotBlank()) add(EventLine(EventField.DESCRIPTION, input.description))
        if (input.alertMinutes.isNotEmpty()) add(EventLine(EventField.REMINDERS, input.alertMinutes.joinToString { words.reminder(it.toInt()) }))
        if (input.repeat != null) add(EventLine(EventField.REPEATS, words.repeats()))
    }

    /**
     * Day and time of the entry: the day alone ("…, all day") or the days from first to last for an all-day entry, the
     * day and the time for a task, the day with start and end otherwise; a zone other than the phone's is named. A
     * start that cannot be read is shown as it is.
     */
    fun eventWhen(input: EventInput, locale: Locale, phoneZone: String, words: ChatWords): String {
        val start = try {
            LocalDateTime.parse(input.start)
        } catch (_: DateTimeParseException) {
            return input.start
        }
        val day = DateTimeFormatter.ofLocalizedDate(FormatStyle.FULL).withLocale(locale)
        val time = DateTimeFormatter.ofLocalizedTime(FormatStyle.SHORT).withLocale(locale)
        if (input.allDay) {
            val days = (input.durationSecs.toLong() / SECONDS_PER_DAY).coerceAtLeast(1)
            val first = day.format(start)
            return words.allDay(if (days > 1 && !input.isTask) "$first – ${day.format(start.plusDays(days - 1))}" else first)
        }
        val begin = "${day.format(start)}, ${time.format(start)}"
        val length = input.durationSecs.toLong()
        val text = if (input.isTask || length <= 0) {
            begin
        } else {
            val end = start.plusSeconds(length)
            if (end.toLocalDate() == start.toLocalDate()) "$begin – ${time.format(end)}" else "$begin – ${day.format(end)}, ${time.format(end)}"
        }
        val zone = input.timeZone
        return if (zone != null && zone != phoneZone) words.inZone(text, zone) else text
    }

    private const val SECONDS_PER_DAY = 86_400L

    // ---- steps ----

    enum class StepKind { SEARCH, LIST, READ_MESSAGE, READ_THREAD, FOLDERS, LABELS, EVENTS, EVENT, OPEN, DRAFT, SUGGEST, OTHER }

    /** What kind of line a tool gets. A tool this screen has no words for (one the core gains later) is "Working". */
    @Suppress("REDUNDANT_ELSE_IN_WHEN")
    fun stepKind(tool: AiToolKind): StepKind = when (tool) {
        AiToolKind.SEARCH_MAIL -> StepKind.SEARCH
        AiToolKind.LIST_MESSAGES, AiToolKind.LIST_TASKS -> StepKind.LIST
        AiToolKind.READ_MESSAGE -> StepKind.READ_MESSAGE
        AiToolKind.READ_THREAD -> StepKind.READ_THREAD
        AiToolKind.LIST_FOLDERS -> StepKind.FOLDERS
        AiToolKind.LIST_LABELS -> StepKind.LABELS
        AiToolKind.LIST_EVENTS -> StepKind.EVENTS
        AiToolKind.READ_EVENT -> StepKind.EVENT
        AiToolKind.OPEN_MESSAGE -> StepKind.OPEN
        AiToolKind.DRAFT_REPLY -> StepKind.DRAFT
        AiToolKind.ARCHIVE, AiToolKind.MOVE, AiToolKind.MARK, AiToolKind.TAG, AiToolKind.ADD_EVENT, AiToolKind.ADD_TASK -> StepKind.SUGGEST
        else -> StepKind.OTHER
    }

    /** "Searching mail: invoice", or the words alone when the step has no detail. The detail is one line. */
    fun stepLine(step: AiStep, words: ChatWords): String {
        val line = words.step(stepKind(step.tool))
        val detail = step.detail.replace('\n', ' ').trim()
        return if (detail.isEmpty()) line else words.stepWithDetail(line, detail)
    }

    // ---- layout ----

    /**
     * The place of the item at [index] among the things to open that follow each other (they are one group): its
     * index in the run and the run's length. An item that is no chip is alone.
     */
    fun chipRun(items: List<ChatItem>, index: Int): Pair<Int, Int> {
        if (items.getOrNull(index) !is ChatItem.Chip) return 0 to 1
        var first = index
        while (first > 0 && items[first - 1] is ChatItem.Chip) first--
        var last = index
        while (last < items.lastIndex && items[last + 1] is ChatItem.Chip) last++
        return (index - first) to (last - first + 1)
    }
}
