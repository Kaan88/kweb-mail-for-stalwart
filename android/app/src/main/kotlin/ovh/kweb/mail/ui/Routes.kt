package ovh.kweb.mail.ui

/** The screens behind the first one. The sign-in screen for a signed-out app is not a route. */
enum class Route {
    Inbox,
    DeliveryCheck,

    /** Signing in again to the same account (the token was refused). */
    SignInAgain,
}

/** A request to show one message, made by a tap on its notification. [serial] tells repeats apart. */
data class FocusRequest(val messageId: String, val serial: Long)
