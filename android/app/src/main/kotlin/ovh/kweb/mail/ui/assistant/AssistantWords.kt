package ovh.kweb.mail.ui.assistant

import android.content.res.Resources
import ovh.kweb.mail.R
import ovh.kweb.mail.ui.UiMessage
import uniffi.kweb_core.Exception as CoreException

/** The consent wording from the string resources. */
class ResourceAssistantWords(private val resources: Resources) : AssistantWords {
    override fun scopeMessages(count: Int, characters: String): String =
        resources.getQuantityString(R.plurals.assistant_scope_messages, count, count, characters)

    override fun scopeText(characters: String): String = resources.getString(R.string.assistant_scope_text, characters)

    override fun shortened(scope: String): String = resources.getString(R.string.assistant_scope_shortened, scope)
}

/** What to tell the user about a failure of the assistant or its settings. */
object AssistantMessages {
    /**
     * The core words what it refused (Invalid) or what went wrong on the way to the service (Temporary: "a message the
     * user can read"); a refused sign-in has its own text; anything else is [fallback].
     */
    fun of(error: Throwable, fallback: Int): UiMessage = when {
        error is CoreException.Temporary && error.msg.isNotBlank() -> UiMessage.Text(error.msg)
        error is CoreException.SignInNeeded -> UiMessage.Res(R.string.assistant_sign_in_needed)
        else -> UiMessage.of(error, fallback)
    }
}
