package ovh.kweb.mail.ui.assistant

import uniffi.kweb_core.Exception as CoreException

/**
 * What the models sheet says when the list could not be loaded: one plain sentence by kind, and the core's own text
 * apart as "Details". The core's texts are fixed words (never the key, never the service's answer).
 */
data class ModelFailure(val kind: Kind, val host: String?, val details: String?) {
    enum class Kind { KEY_REFUSED, UNREACHABLE, OTHER }

    companion object {
        private const val REFUSED = "the service refused the key"
        private const val UNREACHABLE = "could not reach the service at "

        fun of(error: Throwable): ModelFailure {
            val text = when (error) {
                is CoreException.Temporary -> error.msg
                is CoreException.Invalid -> error.msg
                else -> ""
            }.trim()
            return parse(text)
        }

        fun parse(text: String): ModelFailure = when {
            text.startsWith(REFUSED) -> ModelFailure(Kind.KEY_REFUSED, null, text)
            text.startsWith(UNREACHABLE) -> ModelFailure(Kind.UNREACHABLE, text.removePrefix(UNREACHABLE).trim().ifEmpty { null }, text)
            else -> ModelFailure(Kind.OTHER, null, text.ifEmpty { null })
        }
    }
}
