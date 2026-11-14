package ovh.kweb.mail.ui.accounts

import ovh.kweb.mail.ui.Route

/** What to do with something that arrived for an account (a notification tap, a Reply button, a reminder). */
enum class Incoming {
    /** The account is the one on screen: open it as always. */
    OPEN,

    /** Another account and a plain screen on top: switch, then open. */
    SWITCH_THEN_OPEN,

    /** Another account and a message being written on top: switch; the message saves itself as a draft in its own account, and the user is told. */
    SWITCH_SAVING_DRAFT,

    /** Another account and a form with typed input on top: do not switch. Offer it, so that nothing typed is dropped silently. */
    ASK,
}

/** Pure decisions of the account switch. */
object SwitchRules {
    fun onIncoming(top: Route, sameAccount: Boolean): Incoming = when {
        sameAccount -> Incoming.OPEN
        top is Route.Compose -> Incoming.SWITCH_SAVING_DRAFT
        isForm(top) -> Incoming.ASK
        else -> Incoming.SWITCH_THEN_OPEN
    }

    /** Screens where the user may have typed something that a switch of the account would throw away. */
    fun isForm(route: Route): Boolean = when (route) {
        is Route.EditEvent, is Route.EditContact, is Route.Filters, Route.AccountSecurity, Route.Away, Route.Senders, Route.Assistant,
        Route.AddAccount, Route.SignInAgain -> true
        is Route.Admin -> route.screen.holdsInput
        else -> false
    }
}

/** A switch of account offered in a snackbar ("This is for <label>", action "Switch"); [accept] does it and opens what came. */
class SwitchOffer(val label: String, val accept: () -> Unit)
