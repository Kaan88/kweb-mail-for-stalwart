package ovh.kweb.mail.ui

import android.content.res.Resources
import androidx.annotation.StringRes
import uniffi.kweb_core.Exception as CoreException

/** Something to tell the user: one of the app's own texts, or the core's words for what it refused. */
sealed interface UiMessage {
    data class Res(@StringRes val id: Int) : UiMessage

    /** The core's own message (a validation message), shown as it is. */
    data class Text(val text: String) : UiMessage

    fun resolve(resources: Resources): String = when (this) {
        is Res -> resources.getString(id)
        is Text -> text
    }

    companion object {
        /** What to say about [error]: a refusal by the core says why, anything else is [fallback]. */
        fun of(error: Throwable, @StringRes fallback: Int): UiMessage =
            if (error is CoreException.Invalid && error.msg.isNotBlank()) Text(error.msg) else Res(fallback)
    }
}
