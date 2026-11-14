package ovh.kweb.mail.ui.assistant

import java.text.NumberFormat
import java.util.Locale
import uniffi.kweb_core.AiPreview
import uniffi.kweb_core.AiSettings

/** The fragments of text of the consent dialog, so that the rules stay plain Kotlin a unit test can run. */
interface AssistantWords {
    /** "12 messages, about 31,000 characters" ([characters] is formatted already). */
    fun scopeMessages(count: Int, characters: String): String

    /** "about 400 characters of your text". */
    fun scopeText(characters: String): String

    /** [scope] with the note that something was left out. */
    fun shortened(scope: String): String
}

/**
 * Consent is asked once for a service and a model: `consented` is only true for the provider, address and model it
 * was given for ([AssistantRules.Form.toSettings] drops it when any of them changes). The dialog says what is about to be sent, from the core's preview.
 */
object ConsentRules {
    /** The first task for this service and model asks; later ones do not. */
    fun isNeeded(settings: AiSettings): Boolean = !settings.consented

    /**
     * After a save: the core clears consent itself when address or model changed, and keeps it for a service agreed to
     * before whose values did not change. The dialog shows only for a service that is on and not agreed to.
     */
    fun isNeededAfterSave(returned: AiSettings): Boolean = returned.enabled && !returned.consented

    /** What "Decline" stores after a save: the values as returned, switched off. */
    fun declined(returned: AiSettings): AiSettings = returned.copy(enabled = false)

    /** The host (with its port, if any) of [baseUrl] as the dialog names it (the core's own label is not known before a preview); the text when it has no host. */
    fun hostOf(baseUrl: String): String {
        val text = baseUrl.trim()
        val uri = try {
            java.net.URI(text)
        } catch (_: java.net.URISyntaxException) {
            null
        }
        val host = uri?.host?.takeIf { it.isNotBlank() } ?: return text
        // As the core's label: host:port when the address has a port.
        return if (uri.port >= 0) "$host:${uri.port}" else host
    }

    /** What "Send" stores: the same provider, address and model, consent given. */
    fun granted(settings: AiSettings): AiSettings = settings.copy(consented = true)

    /** The size, rounded as the dialog says it: to two significant digits ("2,314" is "2,300"), to tens below a hundred. */
    fun roundedCharacters(characters: Int): Int {
        if (characters <= 0) return 0
        if (characters < 100) return ((characters + 5) / 10 * 10).coerceAtLeast(10)
        var step = 1
        var rest = characters
        while (rest >= 100) {
            rest /= 10
            step *= 10
        }
        return ((characters.toLong() + step / 2) / step * step).toInt()
    }

    /** "1 message, about 2,300 characters", "12 messages, about 31,000 characters, shortened to fit", "about 400 characters of your text". */
    fun scope(preview: AiPreview, words: AssistantWords, locale: Locale): String {
        val characters = NumberFormat.getIntegerInstance(locale).format(roundedCharacters(preview.characters.toInt()))
        val messages = preview.messages.toInt()
        val base = if (messages == 0) words.scopeText(characters) else words.scopeMessages(messages, characters)
        return if (preview.isCut) words.shortened(base) else base
    }
}
