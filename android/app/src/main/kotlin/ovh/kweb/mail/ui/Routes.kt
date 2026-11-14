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

    data object Search : Route

    data object DeliveryCheck : Route

    /** Signing in again to the same account (the token was refused). */
    data object SignInAgain : Route
}

/** The back stack as plain list operations. It never gets empty: the first entry is the root. */
object BackStack {
    val initial: List<Route> = listOf(Route.MailList)

    /** Pushes [route]; the same route on top twice (a double tap) counts once. */
    fun push(stack: List<Route>, route: Route): List<Route> = if (stack.last() == route) stack else stack + route

    fun pop(stack: List<Route>): List<Route> = if (stack.size > 1) stack.dropLast(1) else stack

    /** What a tap on the notification of a cached message shows: the message over the list. */
    fun forNotification(threadId: String, inboxId: String?, messageId: String): List<Route> =
        initial + Route.Thread(threadId, inboxId, messageId)
}
