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

    /** The hosted services the settings offer by name; each fills in its address. [OTHER] shows the address field. */
    enum class Service(val address: String?) {
        OPENAI(OPENAI_ADDRESS),
        OPENROUTER("https://openrouter.ai/api/v1"),
        GEMINI("https://generativelanguage.googleapis.com/v1beta/openai"),
        MISTRAL("https://api.mistral.ai/v1"),
        DEEPSEEK("https://api.deepseek.com/v1"),
        GROQ("https://api.groq.com/openai/v1"),
        OTHER(null),
    }

    /** Plain http: nothing sent is encrypted. The core only allows it for this device and private networks. */
    fun isCleartext(address: String): Boolean = address.trim().startsWith("http://", ignoreCase = true)

    /** The service the address belongs to: a named one when it is exactly that address, otherwise [Service.OTHER]. */
    fun serviceOf(address: String): Service =
        Service.entries.firstOrNull { it.address != null && sameAddress(address, it.address) } ?: Service.OTHER

    /**
     * The service the dropdown shows. [pickedOther]: the user chose "Other address" and is typing, so the field stays
     * even when what was typed happens to be the address of a named service.
     */
    fun shownService(address: String, pickedOther: Boolean): Service = if (pickedOther) Service.OTHER else serviceOf(address)

    /**
     * The address after [service] was picked: a named service's own address; "Other address" starts empty when the
     * address was a named service's (so it is not mistaken for what the user typed) and keeps what is there otherwise.
     */
    fun addressForService(current: String, service: Service): String = when {
        service.address != null -> service.address
        serviceOf(current) != Service.OTHER -> ""
        else -> current
    }

    /** The host of [address] (no scheme, port or path), for showing; empty when there is none. */
    fun hostOf(address: String): String {
        val authority = hostPattern.find(address.trim())?.groupValues?.get(1) ?: return ""
        return if (authority.startsWith("[")) authority.substringBefore(']').trimStart('[') else authority.substringBeforeLast(':', authority)
    }

    private val hostPattern = Regex("""^[a-z][a-z0-9+.-]*://([^/?#\s]+)""", RegexOption.IGNORE_CASE)

    /**
     * Whether the address is this device or a private network: the rule of the core (127/8, 10/8, 172.16/12,
     * 192.168/16, ::1, fc00::/7, fe80::/10, `localhost`, `.localhost` and `.local` names), where a model needs no key.
     */
    fun isPrivateAddress(address: String): Boolean {
        if (hostPattern.find(address.trim()) == null) return false
        val host = hostOf(address)
        val bare = host.trimEnd('.').lowercase()
        if (bare.isEmpty()) return false
        if (host.contains(':')) return privateIpv6(bare)
        if (ipv4.matches(bare)) return privateIpv4(bare.split('.').map { it.toInt() })
        return bare == "localhost" || bare.endsWith(".localhost") || bare.endsWith(".local")
    }

    private val ipv4 = Regex("""\d{1,3}(\.\d{1,3}){3}""")

    private fun privateIpv4(o: List<Int>): Boolean =
        o.all { it in 0..255 } && (o[0] == 127 || o[0] == 10 || (o[0] == 172 && o[1] in 16..31) || (o[0] == 192 && o[1] == 168))

    private fun privateIpv6(text: String): Boolean {
        // A literal only: it never asks a name server.
        if (!text.all { it.isDigit() || it in 'a'..'f' || it == ':' || it == '.' }) return false
        val bytes = try {
            java.net.InetAddress.getByName(text).address
        } catch (_: java.net.UnknownHostException) {
            return false
        }
        val first = bytes[0].toInt() and 0xff
        val second = bytes[1].toInt() and 0xff
        return when (bytes.size) {
            4 -> privateIpv4(bytes.map { it.toInt() and 0xff }) // ::ffff:a.b.c.d
            else -> text == "::1" || bytes.dropLast(1).all { it.toInt() == 0 } && bytes.last().toInt() == 1 ||
                first and 0xfe == 0xfc || (first == 0xfe && second and 0xc0 == 0x80)
        }
    }

    /** What the line under the main switch says. */
    sealed interface SwitchLine {
        /** Off, and no model chosen yet. */
        data object NotSetUp : SwitchLine

        data object Off : SwitchLine

        /** On, no model named yet. */
        data object On : SwitchLine

        /** On: the service (a named one, "Anthropic", or the host of another address) and the model. */
        data class Using(val provider: AiProvider, val service: Service, val host: String, val model: String) : SwitchLine
    }

    fun switchLine(form: Form): SwitchLine {
        val model = form.model.trim()
        return when {
            !form.enabled && model.isEmpty() -> SwitchLine.NotSetUp
            !form.enabled -> SwitchLine.Off
            model.isEmpty() -> SwitchLine.On
            else -> SwitchLine.Using(form.provider, serviceOf(form.baseUrl), hostOf(form.baseUrl), model)
        }
    }

    /** The address and model a chat found to take no tools. */
    data class ToolsFact(val baseUrl: String, val model: String)

    fun toolsFact(settings: AiSettings): ToolsFact? =
        if (settings.toolsUnsupported) ToolsFact(settings.baseUrl.trim().trimEnd('/'), settings.model.trim()) else null

    /** Whether the note "no tool support" shows under the model: for that address and model only, so a change ends it. */
    fun showToolsNote(fact: ToolsFact?, form: Form): Boolean =
        fact != null && form.provider == AiProvider.OPEN_AI_COMPATIBLE && sameAddress(form.baseUrl, fact.baseUrl) && form.model.trim() == fact.model

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

    /** The longest "Your instructions" may be (the core refuses more). */
    const val INSTRUCTIONS_MAX = 8000

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
        /** "Your instructions": the user's own text, sent after the app's own with every request. Per account, not per service. */
        val instructions: String = "",
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
                instructions = instructions,
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
            instructions = instructions,
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
        instructions = stored.instructions,
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
