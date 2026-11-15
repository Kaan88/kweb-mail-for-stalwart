package ovh.kweb.mail.ui.assistant

import java.time.LocalDate
import java.time.LocalDateTime
import java.time.format.DateTimeFormatter
import java.time.format.DateTimeParseException
import java.time.format.FormatStyle
import java.util.Locale
import ovh.kweb.mail.ui.assistant.AssistantRules.Kind
import ovh.kweb.mail.ui.calendar.EventPrefill
import uniffi.kweb_core.AiFinding

/**
 * What the sheet does with reply ideas and with the to-dos and dates found in mail. All of it is text the model wrote
 * about mail: it is shown and, on a tap, put into a form or a draft the user still has to save or send himself.
 */
object InsightRules {
    /** A title of the form holds one line of at most this many characters. */
    const val TITLE_CHARS = 200

    private val START: DateTimeFormatter = DateTimeFormatter.ofPattern("uuuu-MM-dd'T'HH:mm:ss")

    // ---- reply ideas ----

    /** What a tap on a reply idea does. */
    enum class IdeaTap {
        /** The assistant writes a reply draft that says what the idea says. */
        WRITE_DRAFT,

        /** Reply drafts are switched off: a reply is started with the idea itself as its text. */
        USE_AS_TEXT,
    }

    fun ideaTap(draftsAllowed: Boolean): IdeaTap = if (draftsAllowed) IdeaTap.WRITE_DRAFT else IdeaTap.USE_AS_TEXT

    /** The ideas as rows: blank ones dropped, each on one line. */
    fun ideaRows(ideas: List<String>): List<String> = ideas.map(::oneLine).filter { it.isNotEmpty() }

    /**
     * Whether the sheet shows the answer as plain text. Ideas and findings arrive in a form made for reading by the
     * app, so their text is shown only when a finished ideas answer held no idea the app could read.
     */
    fun showsAnswer(kind: Kind, done: Boolean, ideas: Int): Boolean = when (kind) {
        Kind.IDEAS -> done && ideas == 0
        Kind.EXTRACT -> false
        Kind.SUMMARY, Kind.DRAFT, Kind.REWRITE, Kind.EXPLAIN -> true
    }

    // ---- to-dos and dates ----

    /** One finding as a row: what it is, when it is (dates only), and the finding a tap acts on. */
    data class Row(val title: String, val detail: String?, val finding: AiFinding)

    /** The two groups of the sheet, each in the order the findings came. */
    data class Rows(val todos: List<Row>, val dates: List<Row>) {
        val isEmpty: Boolean get() = todos.isEmpty() && dates.isEmpty()
    }

    /** The findings as rows. One without a text is dropped; a date that cannot be read is shown without its time. */
    fun rows(findings: List<AiFinding>, locale: Locale): Rows {
        val todos = ArrayList<Row>()
        val dates = ArrayList<Row>()
        for (finding in findings) {
            when (finding) {
                is AiFinding.Todo -> oneLine(finding.text).takeIf { it.isNotEmpty() }?.let { todos += Row(it, null, finding) }
                is AiFinding.Date -> oneLine(finding.title).takeIf { it.isNotEmpty() }?.let {
                    dates += Row(it, whenText(finding.start, finding.allDay, locale), finding)
                }
            }
        }
        return Rows(todos, dates)
    }

    /** "4 Mar 2027" for a whole day, "4 Mar 2027, 14:30" otherwise, as [locale] writes them; null when [start] is no time. */
    fun whenText(start: String, allDay: Boolean, locale: Locale): String? {
        val at = parse(start) ?: return null
        val day = DateTimeFormatter.ofLocalizedDate(FormatStyle.MEDIUM).withLocale(locale).format(at)
        if (allDay) return day
        return day + ", " + DateTimeFormatter.ofLocalizedTime(FormatStyle.SHORT).withLocale(locale).format(at)
    }

    /**
     * What the form for a new calendar entry starts with for [finding]. A to-do becomes a task for [today], the whole
     * day; a date becomes an event at its time. A date whose time cannot be read falls on [today], the whole day, so
     * the user sets it in the form.
     */
    fun prefill(finding: AiFinding, today: LocalDate): EventPrefill = when (finding) {
        is AiFinding.Todo -> EventPrefill(title(finding.text), START.format(today.atStartOfDay()), allDay = true, isTask = true)
        is AiFinding.Date -> {
            val at = parse(finding.start)
            when {
                at == null -> EventPrefill(title(finding.title), START.format(today.atStartOfDay()), allDay = true, isTask = false)
                finding.allDay -> EventPrefill(title(finding.title), START.format(at.toLocalDate().atStartOfDay()), allDay = true, isTask = false)
                else -> EventPrefill(title(finding.title), START.format(at.withNano(0)), allDay = false, isTask = false)
            }
        }
    }

    private fun parse(start: String): LocalDateTime? = try {
        LocalDateTime.parse(start.trim())
    } catch (_: DateTimeParseException) {
        null
    }

    private fun oneLine(text: String): String = text.split(Regex("\\s+")).filter { it.isNotEmpty() }.joinToString(" ")

    private fun title(text: String): String = oneLine(text).take(TITLE_CHARS).trimEnd()
}
