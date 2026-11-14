package ovh.kweb.mail.ui.assistant

import java.util.UUID
import uniffi.kweb_core.AiEffort
import uniffi.kweb_core.AiModel
import uniffi.kweb_core.AiModelList
import uniffi.kweb_core.AiProvider

/** Which effort levels the settings screen offers, and what the model list says about a model. Plain Kotlin, no Android. */
object EffortRules {
    /** Every level, lowest first: what is offered when the model is unknown or does not name its levels. */
    val ALL: List<AiEffort> = listOf(AiEffort.DEFAULT, AiEffort.LOW, AiEffort.MEDIUM, AiEffort.HIGH, AiEffort.X_HIGH, AiEffort.MAX)

    /** The effort row is shown for the Anthropic service only. */
    fun isShown(provider: AiProvider): Boolean = provider == AiProvider.ANTHROPIC

    /** The entry of [list] for [modelId] (spaces around the typed name do not matter), if the list is known and has it. */
    fun find(list: AiModelList?, modelId: String): AiModel? {
        val id = modelId.trim()
        return if (id.isEmpty()) null else list?.models?.firstOrNull { it.id == id }
    }

    /** Default plus the model's own levels when the last loaded list names them, otherwise all of them. */
    fun offered(provider: AiProvider, modelId: String, list: AiModelList?): List<AiEffort> {
        if (!isShown(provider)) return listOf(AiEffort.DEFAULT)
        val own = find(list, modelId)?.efforts.orEmpty().filter { it != AiEffort.DEFAULT }
        return if (own.isEmpty()) ALL else listOf(AiEffort.DEFAULT) + own.distinct()
    }

    /** [effort] if it is among the offered levels, otherwise Default. */
    fun normalised(effort: AiEffort, provider: AiProvider, modelId: String, list: AiModelList?): AiEffort =
        if (effort in offered(provider, modelId, list)) effort else AiEffort.DEFAULT
}

/** What the model picker shows for one entry. */
object ModelRules {
    /** The id under the name, only when it differs from the name. */
    fun subtitle(model: AiModel): String? = model.id.takeIf { it != model.name && it.isNotBlank() }

    /** The title line: the name, or the id when the service gave no name. */
    fun title(model: AiModel): String = model.name.ifBlank { model.id }

    /** The one-line note for a built-in list (the service was not asked, or has no key yet). */
    fun isBuiltIn(list: AiModelList): Boolean = !list.fromService
}

/** Run ids and when a task has to be cancelled. */
object RunRules {
    /** A new name for a run: the core tells a Stop for this run from one for the next. */
    fun newRunId(): String = UUID.randomUUID().toString()

    /**
     * Whether leaving or closing must cancel a task in this phase: while it runs, and also while it is still being
     * prepared (the core remembers a cancel that comes before the run itself).
     */
    fun cancelsWhenClosed(phase: AssistPhase?): Boolean = phase == AssistPhase.Running || phase == AssistPhase.Checking
}
