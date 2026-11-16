package ovh.kweb.mail.ui.calendar

/**
 * What the form for a NEW entry starts with when it is opened from something the assistant found in a
 * message. The user sees the form and saves it himself; nothing is saved from here.
 * [start] is local time, "YYYY-MM-DDTHH:MM:SS".
 */
data class EventPrefill(
    val title: String,
    val start: String,
    val allDay: Boolean,
    val isTask: Boolean,
    /** Whole minutes of a timed entry; 0 keeps the blank form's length. */
    val durationMinutes: Int = 0,
)
