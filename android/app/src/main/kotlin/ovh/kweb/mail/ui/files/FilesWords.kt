package ovh.kweb.mail.ui.files

import ovh.kweb.mail.R
import ovh.kweb.mail.ui.UiMessage
import uniffi.kweb_core.Exception as CoreException

object FilesWords {
    /** What to tell the user about [error]: the core's own words when it has some (they say why, and for how long), else a plain line. */
    fun message(error: Throwable): UiMessage = when {
        error is CoreException.Invalid && error.msg.isNotBlank() -> UiMessage.Text(error.msg)
        error is CoreException.Temporary && error.msg.isNotBlank() -> UiMessage.Text(error.msg)
        error is CoreException.SignInNeeded -> UiMessage.Res(R.string.files_sign_in_needed)
        else -> UiMessage.Res(R.string.files_failed)
    }
}
