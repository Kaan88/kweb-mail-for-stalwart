package ovh.kweb.mail.ui.filters

import android.content.res.Resources
import androidx.annotation.StringRes
import ovh.kweb.mail.R

/** The words for the ids of fields, comparators and actions. An id the app does not know is shown as it is. */
internal object FilterLabels {
    private val FIELDS = mapOf(
        "from" to R.string.filters_field_from, "to" to R.string.filters_field_to, "cc" to R.string.filters_field_cc,
        "subject" to R.string.filters_field_subject, "header" to R.string.filters_field_header, "size" to R.string.filters_field_size,
        "body" to R.string.filters_field_body, "attachment" to R.string.filters_field_attachment, "all" to R.string.filters_field_all,
    )
    private val COMPARATORS = mapOf(
        "contains" to R.string.filters_cmp_contains, "not_contains" to R.string.filters_cmp_not_contains,
        "is" to R.string.filters_cmp_is, "not_is" to R.string.filters_cmp_not_is,
        "starts_with" to R.string.filters_cmp_starts_with, "ends_with" to R.string.filters_cmp_ends_with,
        "matches" to R.string.filters_cmp_matches, "regex" to R.string.filters_cmp_regex,
        "address_is" to R.string.filters_cmp_address_is, "domain_is" to R.string.filters_cmp_domain_is,
        "greater_than" to R.string.filters_cmp_greater_than, "less_than" to R.string.filters_cmp_less_than,
        "has_any" to R.string.filters_cmp_has_any, "has_type" to R.string.filters_cmp_has_type,
    )
    private val ACTIONS = mapOf(
        "move" to R.string.filters_act_move, "copy" to R.string.filters_act_copy, "forward" to R.string.filters_act_forward,
        "mark_read" to R.string.filters_act_mark_read, "star" to R.string.filters_act_star, "add_label" to R.string.filters_act_add_label,
        "discard" to R.string.filters_act_discard, "reject" to R.string.filters_act_reject, "keep" to R.string.filters_act_keep,
    )
    private val SUMMARY_ACTIONS = mapOf(
        "move" to R.string.filters_sum_move, "copy" to R.string.filters_sum_copy, "forward" to R.string.filters_sum_forward,
        "mark_read" to R.string.filters_sum_mark_read, "star" to R.string.filters_sum_star, "add_label" to R.string.filters_sum_add_label,
        "discard" to R.string.filters_sum_discard, "reject" to R.string.filters_sum_reject, "keep" to R.string.filters_sum_keep,
    )

    private fun word(res: Resources, map: Map<String, Int>, id: String): String = map[id]?.let(res::getString) ?: id

    fun field(res: Resources, id: String) = word(res, FIELDS, id)
    fun comparator(res: Resources, id: String) = word(res, COMPARATORS, id)
    fun action(res: Resources, id: String) = word(res, ACTIONS, id)

    @StringRes
    fun actionRes(id: String): Int? = ACTIONS[id]

    fun words(res: Resources) = FilterWords(
        field = { f, header ->
            if (f == "header" && header.isNotBlank()) res.getString(R.string.filters_field_header_named, header) else field(res, f)
        },
        comparator = { comparator(res, it) },
        action = { word(res, SUMMARY_ACTIONS, it) },
        and = res.getString(R.string.filters_sum_and),
        or = res.getString(R.string.filters_sum_or),
        more = { res.getString(R.string.filters_sum_more, it) },
        arrow = res.getString(R.string.filters_sum_arrow),
        anyMail = res.getString(R.string.filters_sum_any_mail),
        noActions = res.getString(R.string.filters_sum_no_actions),
    )

    fun problem(res: Resources, p: RuleProblem): String = when (p) {
        RuleProblem.EmptyName -> res.getString(R.string.filters_problem_name)
        RuleProblem.NameTooLong -> res.getString(R.string.filters_problem_name_long)
        RuleProblem.TooMany -> res.getString(R.string.filters_problem_too_many)
        RuleProblem.NoConditions -> res.getString(R.string.filters_problem_no_condition)
        is RuleProblem.BlankValue -> res.getString(R.string.filters_problem_blank, p.index + 1)
        is RuleProblem.BadHeader -> res.getString(R.string.filters_problem_header, p.index + 1)
        is RuleProblem.BadSize -> res.getString(R.string.filters_problem_size, p.index + 1)
        is RuleProblem.BadRegex -> res.getString(R.string.filters_problem_regex, p.index + 1)
        is RuleProblem.Unsupported -> res.getString(R.string.filters_problem_unsupported, p.index + 1)
        RuleProblem.NoActions -> res.getString(R.string.filters_problem_no_action)
        is RuleProblem.NoFolder -> res.getString(R.string.filters_problem_folder, p.index + 1)
        is RuleProblem.BadAddress -> res.getString(R.string.filters_problem_address, p.index + 1)
        is RuleProblem.NoRejectText -> res.getString(R.string.filters_problem_reject, p.index + 1)
        is RuleProblem.NoLabel -> res.getString(R.string.filters_problem_label, p.index + 1)
        RuleProblem.PeriodIncomplete -> res.getString(R.string.filters_problem_period_incomplete)
        RuleProblem.PeriodInvalid -> res.getString(R.string.filters_problem_period_invalid)
        RuleProblem.PeriodReversed -> res.getString(R.string.filters_problem_period_reversed)
    }
}
