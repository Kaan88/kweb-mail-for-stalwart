package ovh.kweb.mail.ui.assistant

import uniffi.kweb_core.AiEffort
import uniffi.kweb_core.AiProvider
import uniffi.kweb_core.AiSettings

/** The rules of the AI assistant that need no Android: which entries show, the default address, the state line, the form. */
object AssistantRules {
    /** The three kinds of task, each with its own switch in the settings. */
    enum class Kind { SUMMARY, DRAFT, REWRITE }

    const val OPENAI_ADDRESS = "https://api.openai.com/v1"
    const val ANTHROPIC_ADDRESS = "https://api.anthropic.com/v1"

    /** Whether the assistant offers [kind]: it is on as a whole and that kind is switched on. No settings read yet: no. */
    fun isAllowed(settings: AiSettings?, kind: Kind): Boolean = settings != null && settings.enabled && when (kind) {
        Kind.SUMMARY -> settings.allowSummaries
        Kind.DRAFT -> settings.allowDrafts
        Kind.REWRITE -> settings.allowRewrites
    }

    /** The kinds whose entries are shown now. */
    fun visible(settings: AiSettings?): Set<Kind> = Kind.entries.filterTo(LinkedHashSet()) { isAllowed(settings, it) }

    fun defaultAddress(provider: AiProvider): String = when (provider) {
        AiProvider.OPEN_AI_COMPATIBLE -> OPENAI_ADDRESS
        AiProvider.ANTHROPIC -> ANTHROPIC_ADDRESS
    }

    /**
     * The address after the user chose [to] while [from] was chosen: the default of [to] when the field is empty
     * or still holds the default of the other service, otherwise what the user wrote.
     */
    fun addressAfterChoosing(current: String, from: AiProvider, to: AiProvider): String {
        if (from == to) return current
        val bare = current.trim().trimEnd('/')
        return if (bare.isEmpty() || bare == defaultAddress(from)) defaultAddress(to) else current
    }

    private fun sameAddress(a: String, b: String): Boolean = a.trim().trimEnd('/') == b.trim().trimEnd('/')

    /** The quiet line of the Settings row. */
    sealed interface StateLine {
        data object Off : StateLine

        /** On, but no model named yet. */
        data object On : StateLine

        data class Model(val name: String) : StateLine
    }

    /** What the Settings row says; null while the settings are not known (the row then says nothing). */
    fun stateLine(settings: AiSettings?): StateLine? = when {
        settings == null -> null
        !settings.enabled -> StateLine.Off
        settings.model.isBlank() -> StateLine.On
        else -> StateLine.Model(settings.model.trim())
    }

    /** What the settings screen edits. The key is not part of it: it is typed, sent on Save and forgotten. */
    data class Form(
        val enabled: Boolean,
        val provider: AiProvider,
        val baseUrl: String,
        val model: String,
        val effort: AiEffort = AiEffort.DEFAULT,
        val allowSummaries: Boolean,
        val allowDrafts: Boolean,
        val allowRewrites: Boolean,
        /** The stored key is to be removed on Save. */
        val removeKey: Boolean = false,
    ) {
        /**
         * The settings to send: [stored] with the form's values. Consent is kept only while the service and the model
         * are the same as the stored ones (address compared without spaces and a trailing slash); saving never grants it.
         */
        fun toSettings(stored: AiSettings): AiSettings {
            val sameService = provider == stored.provider && sameAddress(baseUrl, stored.baseUrl) && model.trim() == stored.model.trim()
            return stored.copy(
                enabled = enabled,
                provider = provider,
                baseUrl = baseUrl.trim(),
                model = model.trim(),
                effort = effort,
                consented = stored.consented && sameService,
                allowSummaries = allowSummaries,
                allowDrafts = allowDrafts,
                allowRewrites = allowRewrites,
            )
        }

        fun withProvider(to: AiProvider): Form = copy(provider = to, baseUrl = addressAfterChoosing(baseUrl, provider, to))

        /**
         * The form after the user picked another service and the core gave that service's stored settings ([service]):
         * address, model and effort are the service's own (unsaved input for the other one is dropped, the typed key is
         * dropped by the caller); the switches that belong to no service (on or off, what it may do) stay as shown, and
         * a pending key removal does not carry over.
         */
        fun switchedTo(service: AiSettings): Form = formOf(service).copy(
            enabled = enabled,
            allowSummaries = allowSummaries,
            allowDrafts = allowDrafts,
            allowRewrites = allowRewrites,
            removeKey = false,
        )
    }

    /**
     * The form for [stored]. An empty address is prefilled with the service's default, as when the service is chosen.
     */
    fun formOf(stored: AiSettings): Form = Form(
        enabled = stored.enabled,
        provider = stored.provider,
        baseUrl = stored.baseUrl.ifBlank { defaultAddress(stored.provider) },
        model = stored.model,
        effort = stored.effort,
        allowSummaries = stored.allowSummaries,
        allowDrafts = stored.allowDrafts,
        allowRewrites = stored.allowRewrites,
    )

    /** Anything to lose by leaving: the form differs from [initial], or a key was typed. */
    fun isChanged(form: Form, initial: Form, keyTyped: Boolean): Boolean =
        keyTyped || form.copy(baseUrl = form.baseUrl.trim(), model = form.model.trim()) != initial.copy(baseUrl = initial.baseUrl.trim(), model = initial.model.trim())

    /**
     * What to give the core as `key`: a typed key replaces the stored one, a removal sends an empty string, otherwise
     * null keeps what is stored.
     */
    fun keyToSend(typed: String, remove: Boolean): String? = when {
        typed.isNotBlank() -> typed.trim()
        remove -> ""
        else -> null
    }
}
