package ovh.kweb.mail.ui.attention

import uniffi.kweb_core.AiProposal
import uniffi.kweb_core.AiSparkle
import uniffi.kweb_core.AiSparkleResult
import uniffi.kweb_core.Finding

enum class RunPhase { RUNNING, DONE, FAILED }

/**
 * One run of a button with a sparkle, kept under the finding it came from. The cards the core returned are in [open]
 * until they are added or dismissed; the transitions below are plain functions so that a test can run them.
 */
data class SparkleRun(
    val finding: Finding,
    val task: AiSparkle,
    val runId: String,
    val phase: RunPhase,
    /** The service as the user knows it ("OpenAI"), for the cost note. */
    val service: String = "",
    /** The setting "Show the cost note" as it was when the run started. */
    val showCost: Boolean = true,
    /** Unix seconds when the answer came. */
    val at: Long = 0,
    val result: AiSparkleResult? = null,
    /** The cards that still wait for Add or Dismiss. */
    val open: List<AiProposal> = emptyList(),
    /** Cards being added right now. */
    val busy: Set<String> = emptySet(),
    /** A card that could not be added, with the core's reason. */
    val errors: Map<String, String> = emptyMap(),
    /** At least one card was added: when the run is used up the finding itself is done. */
    val addedAny: Boolean = false,
    /** The found place, the verdict, the ideas and the plain line are still shown. */
    val extrasShown: Boolean = true,
    /** Why a failed run failed, in words for the user. */
    val error: String? = null,
) {
    val id: String get() = finding.id

    /** The result has something besides cards: a place found, a verdict, ideas or a plain line. */
    val hasExtras: Boolean
        get() = result?.let { it.found != null || it.verdict != null || it.ideas.isNotEmpty() || it.text.isNotBlank() } == true

    /** Something is still on screen for the user to act on or read. */
    val hasContent: Boolean get() = phase != RunPhase.DONE || open.isNotEmpty() || (extrasShown && hasExtras)

    /** Everything was added or dismissed. */
    val isUsedUp: Boolean get() = phase == RunPhase.DONE && !hasContent

    /** Used up with at least one addition: the finding is done and stays away. */
    val isCompleted: Boolean get() = isUsedUp && addedAny

    fun adding(id: String): SparkleRun = copy(busy = busy + id, errors = errors - id)

    fun added(ids: Collection<String>): SparkleRun =
        copy(open = open.filterNot { it.id in ids }, busy = busy - ids.toSet(), errors = errors - ids.toSet(), addedAny = addedAny || ids.isNotEmpty())

    fun failed(reasons: Map<String, String>): SparkleRun = copy(busy = busy - reasons.keys, errors = errors + reasons)

    fun dismissedCard(id: String): SparkleRun = copy(open = open.filterNot { it.id == id }, busy = busy - id, errors = errors - id)

    fun dismissedExtras(): SparkleRun = copy(extrasShown = false)

    /** The cards of the run that are events (not tasks), in order. */
    val events: List<AiProposal> get() = open.filterNot { SparkleRules.isTask(it) }

    /** The ids of every card that can be added in one go (none being added already). */
    val addable: List<AiProposal> get() = open.filter { it.id !in busy }
}
