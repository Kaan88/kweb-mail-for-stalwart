package ovh.kweb.mail.ui

import uniffi.kweb_core.Address

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

    /** The contacts: the third root beside the mail list and the agenda. */
    data object Contacts : Route

    /** The file storage: the fourth root. Which folder it shows is the state of its view model. */
    data object Files : Route

    /** What other users share with this one: their accounts, each with the kinds it shares. Opened from the folder drawer. */
    data object Shared : Route

    /**
     * The shared folders of one account. Ids of mail, folders, events and contacts are only unique within an
     * account, so every shared route names it ([account] is the JMAP account id of the owner); [owner] is their
     * address, for the title.
     */
    data class SharedFolders(val account: String, val owner: String) : Route

    /** The messages of one shared folder. */
    data class SharedFolder(val account: String, val mailboxId: String) : Route

    /** One message of a shared folder, opened from [mailboxId] (whose rights say what may be done to it). */
    data class SharedMail(val account: String, val mailboxId: String, val messageId: String) : Route

    /** The agenda of the calendars one account shares. */
    data class SharedCalendar(val account: String, val owner: String) : Route

    /** One event of a shared calendar. */
    data class SharedEventView(val account: String, val eventId: String, val recurrenceId: String?) : Route

    /** The contacts of the address books one account shares. */
    data class SharedContacts(val account: String, val owner: String) : Route

    /** One shared contact. */
    data class SharedContactView(val account: String, val contactId: String) : Route

    /** One contact. */
    data class Contact(val contactId: String) : Route

    /** The form for a new contact ([contactId] null) or for changing one. */
    data class EditContact(val contactId: String?) : Route

    /** The away message, reached from Settings. */
    data object Away : Route

    /** The settings of the AI assistant, reached from Settings. */
    data object Assistant : Route

    data object Search : Route

    data object DeliveryCheck : Route

    data object Settings : Route

    /**
     * A message being written: the draft the core made, by id. [prefillTo] is a recipient the screen puts
     * in To when it opens a blank new draft (a tap on a contact's address); leaving with nothing else
     * changed throws the draft away.
     */
    data class Compose(val draftId: String, val prefillTo: Address? = null) : Route

    /** Signing in again to the same account (the token was refused). */
    data object SignInAgain : Route
}

/** The back stack as plain list operations. It never gets empty: the first entry is the root. */
object BackStack {
    val initial: List<Route> = listOf(Route.MailList)

    /** Pushes [route]; the same route on top twice (a double tap) counts once. */
    fun push(stack: List<Route>, route: Route): List<Route> = if (stack.last() == route) stack else stack + route

    /**
     * A tap on an entry of the bottom bar: the mail list is the bottom of the stack, the agenda and the
     * contacts sit on it, so system back from either goes to the mail list and then out.
     */
    fun switchTo(root: Route): List<Route> = if (root == Route.MailList) initial else initial + root

    /** The four screens the bottom bar leads to; only they show it. */
    fun isRoot(route: Route): Boolean = route == Route.MailList || route == Route.Calendar || route == Route.Contacts || route == Route.Files

    /** The share is gone: back to the shared screen (it reads itself again), or the root when it is not on the stack. */
    fun backToShared(stack: List<Route>): List<Route> {
        val index = stack.indexOfLast { it == Route.Shared }
        return if (index < 0) initial else stack.take(index + 1)
    }

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
        val isForm = top is Route.EditEvent || top is Route.EditContact || top is Route.Compose || top == Route.Away || top == Route.Assistant
        if (!isForm) return target
        return if (target.size > initial.size) push(stack, target.last()) else stack
    }

    /** After an entry was saved: the form and the event screens that showed the old values are gone, the agenda is on top. */
    fun afterEventSaved(stack: List<Route>): List<Route> =
        stack.dropLastWhile { it is Route.EditEvent || it is Route.Event }.ifEmpty { initial }

    /**
     * After a contact was saved: the form is gone. A changed contact shows again (it reads again when it comes
     * up), a new one leaves the list on top.
     */
    fun afterContactSaved(stack: List<Route>): List<Route> = stack.dropLastWhile { it is Route.EditContact }.ifEmpty { initial }

    /** What the Reply button of a notification shows: the new message over the list. */
    fun forCompose(draftId: String): List<Route> = initial + Route.Compose(draftId)

    /**
     * After a message was sent: the mail list, where Undo is offered, whatever the message was
     * written from (a thread, a search, an Undo that opened it again).
     */
    fun afterSend(): List<Route> = initial
}
