package ovh.kweb.mail.ui.assistant

import uniffi.kweb_core.AiActionMode
import uniffi.kweb_core.AiEffort
import uniffi.kweb_core.AiProvider
import uniffi.kweb_core.AiSettings

/** The rules of the AI assistant that need no Android: which entries show, the default address, the state line, the form. */
object AssistantRules {
    /** The kinds of task of the sheet, each with its own switch in the settings. */
    enum class Kind { SUMMARY, DRAFT, REWRITE, IDEAS, EXTRACT, EXPLAIN }

    /** Whether the chat can be used, and if not, what is in the way. */
    enum class ChatGate {
        READY,

        /** The assistant is off as a whole, or its settings are not known. */
        OFF,

        /** The assistant is on, the chat is switched off. */
        CHAT_OFF,

        /** The chat is on, but the user has not agreed to the service, or not to what the chat reads. */
        NOT_AGREED,
    }

    const val OPENAI_ADDRESS = "https://api.openai.com/v1"
    const val ANTHROPIC_ADDRESS = "https://api.anthropic.com/v1"

    /** The addresses the settings offer to fill in. A local server is plain http and needs no key. */
    enum class Preset(val address: String, val port: Int?, val local: Boolean) {
        OPENAI(OPENAI_ADDRESS, null, false),
        OLLAMA("http://localhost:11434/v1", 11434, true),
        LM_STUDIO("http://localhost:1234/v1", 1234, true),
        LLAMA_CPP("http://localhost:8080/v1", 8080, true),
    }

    private val localAddress = Regex("""^http://([^/:\s]+)(?::(\d+))?(/.*)?$""", RegexOption.IGNORE_CASE)

    /** Plain http: nothing sent is encrypted. The core only allows it for this device and private networks. */
    fun isCleartext(address: String): Boolean = address.trim().startsWith("http://", ignoreCase = true)

    /** The preset the address looks like: the hosted default exactly, a local one by its port (the host is the user's). */
    fun presetOf(address: String): Preset? {
        if (sameAddress(address, OPENAI_ADDRESS)) return Preset.OPENAI
        val port = localAddress.matchEntire(address.trim())?.groupValues?.get(2)?.toIntOrNull() ?: return null
        return Preset.entries.firstOrNull { it.local && it.port == port }
    }

    /**
     * The address after [preset] was picked: the hosted one as it is; a local one keeps the host the user already wrote
     * for a local server (so another port does not lose "192.168.1.20") and otherwise starts at localhost.
     */
    fun addressForPreset(current: String, preset: Preset): String {
        if (!preset.local) return preset.address
        val host = localAddress.matchEntire(current.trim())?.groupValues?.get(1)
        return if (host.isNullOrEmpty()) preset.address else "http://$host:${preset.port}/v1"
    }

    /** Whether the assistant offers [kind]: it is on as a whole and that kind is switched on. No settings read yet: no. */
    fun isAllowed(settings: AiSettings?, kind: Kind): Boolean = settings != null && settings.enabled && when (kind) {
        Kind.SUMMARY -> settings.allowSummaries
        Kind.DRAFT -> settings.allowDrafts
        Kind.REWRITE -> settings.allowRewrites
        Kind.IDEAS -> settings.allowIdeas
        Kind.EXTRACT -> settings.allowExtract
        Kind.EXPLAIN -> settings.allowExplain
    }

    /** The kinds whose entries are shown now. */
    fun visible(settings: AiSettings?): Set<Kind> = Kind.entries.filterTo(LinkedHashSet()) { isAllowed(settings, it) }

    /**
     * What stands between the user and the chat, in the order the core checks it: the assistant as a whole, the chat's
     * switch, then the two agreements (to the service, and to the chat choosing what it reads).
     */
    fun chatReady(settings: AiSettings?): ChatGate = when {
        settings == null || !settings.enabled -> ChatGate.OFF
        !settings.allowChat -> ChatGate.CHAT_OFF
        !settings.consented || !settings.chatConsented -> ChatGate.NOT_AGREED
        else -> ChatGate.READY
    }

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
        val allowIdeas: Boolean = false,
        val allowExtract: Boolean = false,
        val allowExplain: Boolean = false,
        /** The chat, in which the assistant itself chooses what it reads. */
        val allowChat: Boolean = false,
        /** What the chat may do about changes; it has no effect while the chat is off. */
        val actions: AiActionMode = AiActionMode.ASK_FIRST,
        /** The stored key is to be removed on Save. */
        val removeKey: Boolean = false,
    ) {
        /**
         * The settings to send: [stored] with the form's values. Consent is kept only while the service and the model
         * are the same as the stored ones (address compared without spaces and a trailing slash); saving never grants it.
         * The same holds for the chat's own agreement, which only the core's `aiChatConsent` gives.
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
                allowIdeas = allowIdeas,
                allowExtract = allowExtract,
                allowExplain = allowExplain,
                allowChat = allowChat,
                chatConsented = stored.chatConsented && stored.consented && sameService,
                actions = actions,
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
            allowIdeas = allowIdeas,
            allowExtract = allowExtract,
            allowExplain = allowExplain,
            allowChat = allowChat,
            actions = actions,
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
        allowIdeas = stored.allowIdeas,
        allowExtract = stored.allowExtract,
        allowExplain = stored.allowExplain,
        allowChat = stored.allowChat,
        actions = stored.actions,
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
