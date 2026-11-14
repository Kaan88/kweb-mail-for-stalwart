package ovh.kweb.mail.ui

/** The screens of the app. The sign-in screen for a signed-out app is not a route. */
sealed interface Route {
    /** The root. Which folder it shows is the state of its view model. */
    data object MailList : Route

    /**
     * One conversation. [mailboxId] is the folder it was opened from (what Archive, Delete and
     * Move act on), null when it was opened from a search. [messageId] is the message to show
     * first, if any.
     */
    data class Thread(val threadId: String, val mailboxId: String?, val messageId: String?) : Route

    /** The agenda: a root beside the mail list. */
    data object Calendar : Route

    /** One event or task. [recurrenceId] is the occurrence of a repeating event that was opened, if any. */
    data class Event(val eventId: String, val recurrenceId: String?) : Route

    /**
     * The form for a new entry ([eventId] null, [startHint] the day it starts on as "2026-11-12") or for
     * changing one.
     */
    data class EditEvent(val eventId: String?, val startHint: String?) : Route

    data object Search : Route

    data object DeliveryCheck : Route

    data object Settings : Route

    /** A message being written: the draft the core made, by id. */
    data class Compose(val draftId: String) : Route

    /** Signing in again to the same account (the token was refused). */
    data object SignInAgain : Route
}

/** The back stack as plain list operations. It never gets empty: the first entry is the root. */
object BackStack {
    val initial: List<Route> = listOf(Route.MailList)

    /** Pushes [route]; the same route on top twice (a double tap) counts once. */
    fun push(stack: List<Route>, route: Route): List<Route> = if (stack.last() == route) stack else stack + route

    /**
     * A tap on an entry of the bottom bar: the mail list is the bottom of the stack, the agenda sits
     * on it, so system back from the agenda goes to the mail list and then out.
     */
    fun switchTo(root: Route): List<Route> = if (root == Route.MailList) initial else initial + root

    fun pop(stack: List<Route>): List<Route> = if (stack.size > 1) stack.dropLast(1) else stack

    /** What a tap on the notification of a cached message shows: the message over the list. */
    fun forNotification(threadId: String, inboxId: String?, messageId: String): List<Route> =
        initial + Route.Thread(threadId, inboxId, messageId)

    /** What a tap on the reminder of an event shows: the event over the agenda over the list. */
    fun forReminder(eventId: String, recurrenceId: String?): List<Route> = switchTo(Route.Calendar) + Route.Event(eventId, recurrenceId)

    /**
     * Shows [target] (one of the lists above). While a form or a message being written is on top, the
     * target goes over the current stack instead of replacing it, so back returns to the form and what
     * was typed is not thrown away without the question the screen asks.
     */
    fun showOver(stack: List<Route>, target: List<Route>): List<Route> {
        val top = stack.last()
        if (top !is Route.EditEvent && top !is Route.Compose) return target
        return if (target.size > initial.size) push(stack, target.last()) else stack
    }

    /** After an entry was saved: the form and the event screens that showed the old values are gone, the agenda is on top. */
    fun afterEventSaved(stack: List<Route>): List<Route> =
        stack.dropLastWhile { it is Route.EditEvent || it is Route.Event }.ifEmpty { initial }

    /** What the Reply button of a notification shows: the new message over the list. */
    fun forCompose(draftId: String): List<Route> = initial + Route.Compose(draftId)

    /**
     * After a message was sent: the mail list, where Undo is offered, whatever the message was
     * written from (a thread, a search, an Undo that opened it again).
     */
    fun afterSend(): List<Route> = initial
}
