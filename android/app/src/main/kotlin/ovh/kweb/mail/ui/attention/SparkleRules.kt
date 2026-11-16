package ovh.kweb.mail.ui.attention

import java.text.NumberFormat
import java.time.Instant
import java.time.LocalDate
import java.time.LocalDateTime
import java.time.ZoneId
import java.time.format.DateTimeFormatter
import java.time.format.FormatStyle
import java.util.Locale
import uniffi.kweb_core.AiBatchApplied
import uniffi.kweb_core.AiChange
import uniffi.kweb_core.AiProposal
import uniffi.kweb_core.AiSparkle
import uniffi.kweb_core.AiSparklePreview
import uniffi.kweb_core.AiSparkleResult
import uniffi.kweb_core.EventInput
import uniffi.kweb_core.Finding
import uniffi.kweb_core.PhoneAction
import uniffi.kweb_core.ServiceAction

/** The fragments of text the rules put together, so that the rules stay plain Kotlin a unit test can run. */
interface SparkleWords {
    fun mails(count: Int, senders: String): String
    fun events(count: Int): String
    fun tasks(count: Int): String
    fun theEvent(): String
    fun moreMail(): String
    fun and(first: String, second: String): String
    fun shortened(scope: String): String
    fun sendersMore(names: String, more: Int): String
    fun sentMails(count: Int): String
    fun sentDay(): String
    fun sentEvent(lookups: Int): String
    fun sentTaskMail(): String
    fun cost(what: String, service: String, model: String, time: String): String
    fun tokens(input: String?, output: String?): String?
    fun withTokens(note: String, tokens: String): String
    fun added(events: Int, tasks: Int): String
    fun batchFailed(count: Int, reason: String): String
    fun due(text: String): String
    fun whenAt(day: String, time: String): String
    fun whenRange(day: String, from: String, to: String): String
}

/**
 * What the buttons with a sparkle do before and after a run: which request a finding makes, whether the user is asked,
 * what the dialog and the cost note say. Plain rules; the screens and the view model only carry them out.
 */
object SparkleRules {
    /** The most mails one "Add dates" run takes. */
    const val MOST_MAILS = 5

    /** The request a finding's service button makes, from the ids the finding carries; null when it carries none that fit. */
    fun task(finding: Finding, action: ServiceAction, today: LocalDate, zone: ZoneId): AiSparkle? {
        val now = today.toString()
        val tz = zone.id
        return when (action) {
            ServiceAction.ADD_DATES -> finding.messageIds.take(MOST_MAILS).takeIf { it.isNotEmpty() }?.let { AiSparkle.AddDates(it, now, tz) }
            ServiceAction.PLAN_MY_DAY -> AiSparkle.PlanMyDay(dayOf(finding, today, zone).toString(), now, tz)
            ServiceAction.LOOK_IN_MAIL -> finding.eventIds.firstOrNull()?.let { id -> AiSparkle.LookInMail(id, recurrenceOf(finding, id), now, tz) }
            ServiceAction.CHECK_THE_MAIL -> {
                val task = finding.taskIds.firstOrNull()
                val mail = finding.messageIds.firstOrNull()
                if (task != null && mail != null) AiSparkle.CheckTheMail(task, mail, now, tz) else null
            }
            ServiceAction.REPLY_IDEAS -> finding.messageIds.firstOrNull()?.let { AiSparkle.ReplyIdeas(it) }
        }
    }

    /** The day a plan is for: the day of a busy day, otherwise today. */
    fun dayOf(finding: Finding, today: LocalDate, zone: ZoneId): LocalDate =
        if (finding.rule == uniffi.kweb_core.AttentionRule.BUSY_DAY) {
            finding.times.firstOrNull()?.let { Instant.ofEpochSecond(it).atZone(zone).toLocalDate() } ?: today
        } else {
            today
        }

    private fun recurrenceOf(finding: Finding, eventId: String): String? = finding.phone.firstNotNullOfOrNull { a ->
        when (a) {
            is PhoneAction.MoveEvent -> a.recurrenceId.takeIf { a.eventId == eventId }
            is PhoneAction.FindSlot -> a.recurrenceId.takeIf { a.eventId == eventId }
            else -> null
        }
    }

    // ---- the ask ----

    /** Whether the user is asked before this run and with which words. */
    enum class Ask {
        /** Nothing: it runs at once. */
        NONE,

        /** The first use for this service and model: the agreement itself, with "you will not be asked again". */
        FIRST,

        /** The setting "Ask before each run": the same dialog without its last sentence. */
        EACH,
    }

    /** Whether the run sends mail (a mail's text, or lets the model read more) as opposed to calendar and task data only. */
    fun sendsMail(preview: AiSparklePreview): Boolean = preview.messages > 0u || preview.readsMail

    /**
     * The ask rule: the first use asks (the agreement for this service and model, and the chat's own for the two tasks that
     * read mail by themselves); after that "Ask before each run" asks only for a run that sends mail; Plan my day sends
     * calendar and task data only and runs at once; with the setting off nothing more is asked.
     */
    fun ask(consented: Boolean, chatYesMissing: Boolean, askEachRun: Boolean, preview: AiSparklePreview): Ask = when {
        !consented || chatYesMissing -> Ask.FIRST
        askEachRun && sendsMail(preview) -> Ask.EACH
        else -> Ask.NONE
    }

    /** The two tasks whose run needs the chat's own yes: the model reads or looks up by itself. */
    fun needsChatYes(task: AiSparkle): Boolean = task is AiSparkle.LookInMail || task is AiSparkle.CheckTheMail

    /** The names in "Name <address>" lines, without repeats, as the dialog lists them. */
    fun senderNames(senders: List<String>, words: SparkleWords): String {
        val names = senders.map { line ->
            val name = line.substringBefore('<').trim().trim('"').trim()
            name.ifEmpty { line.substringAfter('<', line).substringBefore('>').trim() }
        }.filter { it.isNotEmpty() }.distinct()
        return when {
            names.isEmpty() -> ""
            names.size == 1 -> names[0]
            names.size == 2 -> words.and(names[0], names[1])
            else -> words.sendersMore(names.take(2).joinToString(", "), names.size - 2)
        }
    }

    private fun join(parts: List<String>, words: SparkleWords): String = when (parts.size) {
        0 -> ""
        1 -> parts[0]
        2 -> words.and(parts[0], parts[1])
        else -> words.and(parts.dropLast(1).joinToString(", "), parts.last())
    }

    /** What goes out, as the dialog says it: the mails (with who they are from), the events, the tasks, "the mail it chooses to read". */
    fun scope(preview: AiSparklePreview, task: AiSparkle, words: SparkleWords): String {
        val parts = ArrayList<String>()
        if (preview.events > 0u) parts += if (task is AiSparkle.LookInMail) words.theEvent() else words.events(preview.events.toInt())
        if (preview.tasks > 0u) parts += words.tasks(preview.tasks.toInt())
        if (preview.messages > 0u) parts += words.mails(preview.messages.toInt(), senderNames(preview.senders, words))
        if (preview.readsMail) parts += words.moreMail()
        val text = join(parts, words)
        return if (preview.isCut) words.shortened(text) else text
    }

    // ---- the words under a result ----

    /** "Sent 2 mails to OpenAI · gpt-5 at 09:41 · 2,412 tokens in, 318 out"; the tokens are left out when the service reported none. */
    fun costNote(task: AiSparkle, result: AiSparkleResult, service: String, time: String, words: SparkleWords, locale: Locale): String {
        val what = when (task) {
            is AiSparkle.AddDates, is AiSparkle.ReplyIdeas -> words.sentMails(result.messages.toInt())
            is AiSparkle.PlanMyDay -> words.sentDay()
            is AiSparkle.LookInMail -> words.sentEvent(result.steps.toInt())
            is AiSparkle.CheckTheMail -> words.sentTaskMail()
        }
        val note = words.cost(what, service, result.model, time)
        val usage = result.usage ?: return note
        val format = NumberFormat.getIntegerInstance(locale)
        val tokens = words.tokens(usage.inputTokens?.let { format.format(it.toLong()) }, usage.outputTokens?.let { format.format(it.toLong()) })
        return if (tokens == null) note else words.withTokens(note, tokens)
    }

    /** "Thu 19 Nov, 15:00 to 16:00", "Thu 19 Nov" for an all-day entry, "Due Mon 30 Nov" or "Due Mon 30 Nov, 10:00" for a task. */
    fun whenText(input: EventInput, locale: Locale, words: SparkleWords): String {
        val start = runCatching { LocalDateTime.parse(input.start.trim()) }.getOrNull() ?: return ""
        val day = DateTimeFormatter.ofPattern("EEE d MMM", locale).format(start)
        val time = DateTimeFormatter.ofLocalizedTime(FormatStyle.SHORT).withLocale(locale)
        val timed = !input.allDay && start.toLocalTime() != java.time.LocalTime.MIDNIGHT
        if (input.isTask) return words.due(if (timed) words.whenAt(day, time.format(start)) else day)
        if (!timed) return day
        val seconds = input.durationSecs.toLong()
        if (seconds <= 0) return words.whenAt(day, time.format(start))
        val end = start.plusSeconds(seconds)
        return if (end.toLocalDate() == start.toLocalDate()) words.whenRange(day, time.format(start), time.format(end)) else words.whenAt(day, time.format(start))
    }

    /** The start time as a line of a plan: "10:00 Send invoice 0311 (15 min)". */
    fun planLine(input: EventInput, locale: Locale): String {
        val start = runCatching { LocalDateTime.parse(input.start.trim()) }.getOrNull() ?: return input.title
        val time = DateTimeFormatter.ofLocalizedTime(FormatStyle.SHORT).withLocale(locale).format(start)
        val minutes = (input.durationSecs / 60u).toInt()
        return if (minutes > 0) "$time ${input.title} ($minutes min)" else "$time ${input.title}"
    }

    // ---- cards ----

    /** The event or task a card would add; every card of a sparkle run is an addition. */
    fun inputOf(proposal: AiProposal): EventInput? = (proposal.change as? AiChange.AddEvent)?.input

    fun isTask(proposal: AiProposal): Boolean = inputOf(proposal)?.isTask == true

    fun calendarOf(proposal: AiProposal): String = (proposal.change as? AiChange.AddEvent)?.calendarName.orEmpty()

    /** The editor's form of a card. */
    fun prefillOf(input: EventInput): ovh.kweb.mail.ui.calendar.EventPrefill =
        ovh.kweb.mail.ui.calendar.EventPrefill(input.title, input.start, input.allDay, input.isTask, (input.durationSecs / 60u).toInt())

    /** What the line under an `ai_apply_all` says: how many events and tasks were added, and how many were not and why. */
    class BatchLine(val added: String?, val failed: String?)

    fun batchLine(done: AiBatchApplied, kindOf: (String) -> Boolean?, words: SparkleWords): BatchLine {
        val tasks = done.applied.count { kindOf(it.proposalId) == true }
        val events = done.applied.size - tasks
        val added = if (done.applied.isEmpty()) null else words.added(events, tasks)
        val failed = done.failed.takeIf { it.isNotEmpty() }?.let { words.batchFailed(it.size, it.first().message) }
        return BatchLine(added, failed)
    }
}
