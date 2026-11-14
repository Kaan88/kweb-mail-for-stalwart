package ovh.kweb.mail.ui.calendar

import android.content.res.Resources
import androidx.annotation.StringRes
import ovh.kweb.mail.R
import uniffi.kweb_core.Exception as CoreException

/** Something to tell the user: one of the app's own texts, or the core's words for what it refused. */
sealed interface CalendarMessage {
    data class Res(@StringRes val id: Int) : CalendarMessage

    /** The core's own message (a validation message), shown as it is. */
    data class Text(val text: String) : CalendarMessage

    fun resolve(resources: Resources): String = when (this) {
        is Res -> resources.getString(id)
        is Text -> text
    }

    companion object {
        /** What to say about [error]: a refusal by the core says why, anything else is [fallback]. */
        fun of(error: Throwable, @StringRes fallback: Int): CalendarMessage =
            if (error is CoreException.Invalid && error.msg.isNotBlank()) Text(error.msg) else Res(fallback)

        @StringRes
        fun problem(problem: Problem): Int = when (problem) {
            Problem.TITLE_BLANK -> R.string.cal_problem_title
            Problem.NO_CALENDAR -> R.string.cal_problem_calendar
            Problem.END_BEFORE_START -> R.string.cal_problem_end
            Problem.UNTIL_MISSING -> R.string.cal_problem_until_missing
            Problem.UNTIL_BEFORE_START -> R.string.cal_problem_until
            Problem.COUNT_INVALID -> R.string.cal_problem_count
            Problem.NO_WEEKDAY -> R.string.cal_problem_weekday
        }
    }
}
