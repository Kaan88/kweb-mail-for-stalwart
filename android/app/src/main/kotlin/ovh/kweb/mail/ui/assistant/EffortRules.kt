package ovh.kweb.mail.ui.assistant

import java.util.UUID
import uniffi.kweb_core.AiEffort
import uniffi.kweb_core.AiModel
import uniffi.kweb_core.AiModelList

/**
 * Which effort levels the settings screen offers, and what the model list says about a model. Plain Kotlin, no Android.
 * Every service follows one rule: only the levels the service itself listed for the model are offered (the core sends
 * nothing else), and a model it named no level for has no effort row at all.
 */
object EffortRules {
    /** Every level, lowest first: the order the offered ones are shown in. */
    val ALL: List<AiEffort> = listOf(
        AiEffort.DEFAULT, AiEffort.OFF, AiEffort.MINIMAL, AiEffort.LOW, AiEffort.MEDIUM, AiEffort.HIGH, AiEffort.X_HIGH, AiEffort.MAX,
    )

    /** The entry of [list] for [modelId] (spaces around the typed name do not matter), if the list is known and has it. */
    fun find(list: AiModelList?, modelId: String): AiModel? {
        val id = modelId.trim()
        return if (id.isEmpty()) null else list?.models?.firstOrNull { it.id == id }
    }

    /**
     * Default plus the model's own levels from the last loaded list. While no list is loaded yet: Default plus [current]
     * (the core shows a stored level only where the service listed it, so it is not lost before the list comes).
     */
    fun offered(modelId: String, list: AiModelList?, current: AiEffort = AiEffort.DEFAULT): List<AiEffort> {
        val own = if (list == null) listOf(current) else find(list, modelId)?.efforts.orEmpty()
        val levels = own.filter { it != AiEffort.DEFAULT }.toSet()
        return listOf(AiEffort.DEFAULT) + ALL.filter { it in levels }
    }

    /** The effort row is shown only when there is more than Default to choose from. */
    fun isShown(offered: List<AiEffort>): Boolean = offered.size > 1

    /** [effort] if it is among the offered levels, otherwise Default. Without a list it is kept. */
    fun normalised(effort: AiEffort, modelId: String, list: AiModelList?): AiEffort =
        if (effort in offered(modelId, list, effort)) effort else AiEffort.DEFAULT
}

/** What the model picker shows for one entry. */
object ModelRules {
    /** The id under the name, only when it differs from the name. */
    fun subtitle(model: AiModel): String? = model.id.takeIf { it != model.name && it.isNotBlank() }

    /** The title line: the name, or the id when the service gave no name. */
    fun title(model: AiModel): String = model.name.ifBlank { model.id }
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
