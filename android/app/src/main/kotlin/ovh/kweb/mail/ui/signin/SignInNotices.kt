package ovh.kweb.mail.ui.signin

import kotlinx.coroutines.channels.Channel

/** What a sign-in has to tell the user after its own screen is gone. */
object SignInNotices {
    /** Signing in again gave a user the app already has: the entry was removed, the app says so. */
    val duplicateRemoved = Channel<Unit>(Channel.BUFFERED)
}
