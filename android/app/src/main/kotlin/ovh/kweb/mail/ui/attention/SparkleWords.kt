package ovh.kweb.mail.ui.attention

import android.content.res.Resources
import ovh.kweb.mail.R

/** [SparkleWords] from the app's string resources. */
class ResourceSparkleWords(private val r: Resources) : SparkleWords {
    override fun mails(count: Int, senders: String): String {
        val n = r.getQuantityString(R.plurals.att_run_n_mails, count, count)
        return if (senders.isEmpty()) n else r.getString(R.string.att_run_scope_from, n, senders)
    }

    override fun events(count: Int): String = r.getQuantityString(R.plurals.att_run_n_events, count, count)

    override fun tasks(count: Int): String = r.getQuantityString(R.plurals.att_run_n_tasks, count, count)

    override fun theEvent(): String = r.getString(R.string.att_run_scope_the_event)

    override fun moreMail(): String = r.getString(R.string.att_run_scope_more_mail)

    override fun and(first: String, second: String): String = r.getString(R.string.att_run_scope_and, first, second)

    override fun shortened(scope: String): String = r.getString(R.string.att_run_scope_shortened, scope)

    override fun sendersMore(names: String, more: Int): String = r.getString(R.string.att_run_sender_more, names, more)

    override fun sentMails(count: Int): String = r.getQuantityString(R.plurals.att_run_sent_mails, count, count)

    override fun sentDay(): String = r.getString(R.string.att_run_sent_day)

    override fun sentEvent(lookups: Int): String = r.getString(if (lookups > 0) R.string.att_run_sent_event_lookups else R.string.att_run_sent_event)

    override fun sentTaskMail(): String = r.getString(R.string.att_run_sent_task_mail)

    override fun cost(what: String, service: String, model: String, time: String): String = r.getString(R.string.att_run_cost, what, service, model, time)

    override fun tokens(input: String?, output: String?): String? = when {
        input != null && output != null -> r.getString(R.string.att_run_tokens_both, input, output)
        input != null -> r.getString(R.string.att_run_tokens_in, input)
        output != null -> r.getString(R.string.att_run_tokens_out, output)
        else -> null
    }

    override fun withTokens(note: String, tokens: String): String = r.getString(R.string.att_run_cost_tokens, note, tokens)

    override fun added(events: Int, tasks: Int): String = when {
        events > 0 && tasks > 0 -> r.getString(R.string.att_run_added_mixed, events(events), tasks(tasks))
        tasks > 0 -> r.getQuantityString(R.plurals.att_run_added_tasks, tasks, tasks)
        else -> r.getQuantityString(R.plurals.att_run_added_events, events, events)
    }

    override fun batchFailed(count: Int, reason: String): String = r.getString(R.string.att_run_batch_failed_n, count, reason)

    override fun due(text: String): String = r.getString(R.string.att_run_due, text)

    override fun whenAt(day: String, time: String): String = r.getString(R.string.att_run_when_at, day, time)

    override fun whenRange(day: String, from: String, to: String): String = r.getString(R.string.att_run_when_range, day, from, to)
}
