package ovh.kweb.mail.ui.mailextras

import ovh.kweb.mail.R
import uniffi.kweb_core.Unsubscribe

/** How the app would leave a list, and what it says before doing so. */
object UnsubscribeRules {
    enum class Way {
        /** A request goes to the list from the core (the list takes a one-click request). */
        REQUEST,

        /** A mail is sent from the user's address. */
        MAIL,

        /** The list's page opens in the browser: the only way left. */
        BROWSER,
    }

    /** The way to leave, or null when the message offers nothing the app can use. The core's own ways come before the browser. */
    fun way(info: Unsubscribe?): Way? = when {
        info == null -> null
        info.oneClick -> Way.REQUEST
        info.byMail -> Way.MAIL
        isSafeWebUrl(info.webUrl) -> Way.BROWSER
        else -> null
    }

    /** Only an https address goes to the browser. */
    fun isSafeWebUrl(url: String?): Boolean = url != null && url.length > "https://".length && url.startsWith("https://", ignoreCase = true) && url.none { it.isWhitespace() }

    /** The sentence under "Unsubscribe from [list]?": what will happen. */
    fun confirmText(way: Way): Int = when (way) {
        Way.REQUEST -> R.string.unsub_confirm_request
        Way.MAIL -> R.string.unsub_confirm_mail
        Way.BROWSER -> R.string.unsub_confirm_browser
    }

    /** The label of the button that does it. */
    fun confirmButton(way: Way): Int = if (way == Way.BROWSER) R.string.unsub_open_page else R.string.unsub_confirm_button
}
