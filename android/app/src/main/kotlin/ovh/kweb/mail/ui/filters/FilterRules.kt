package ovh.kweb.mail.ui.filters

import uniffi.kweb_core.FilterCondition
import uniffi.kweb_core.FilterField
import uniffi.kweb_core.FilterMatch
import uniffi.kweb_core.FilterRule
import uniffi.kweb_core.Filters
import uniffi.kweb_core.Mailbox

/** Which kind of line a condition looks at, without the header's name. */
enum class FieldKind { FROM, RECIPIENT, SUBJECT, HEADER }

/** A fault the editor can see before the core is asked; the core refuses the same things. */
sealed interface RuleProblem {
    data object NoCondition : RuleProblem

    /** The condition at [index] (0-based) has no text. */
    data class BlankValue(val index: Int) : RuleProblem

    /** The text of the condition at [index] has more than one line. */
    data class ManyLines(val index: Int) : RuleProblem

    /** The header name of the condition at [index] is empty or not a header name. */
    data class BadHeader(val index: Int) : RuleProblem

    data object DeleteAndMove : RuleProblem

    data object NoAction : RuleProblem

    /** The folder to move to is not among the user's folders any more. */
    data object FolderGone : RuleProblem

    data object TooMany : RuleProblem
}

/** What a banner above the rules says. */
sealed interface Banner {
    data object AwayOn : Banner

    data class OtherScript(val name: String) : Banner

    data object Stale : Banner

    data object Edited : Banner
}

/** What switching the filters on has to ask first, because it switches something else off. */
sealed interface SwitchOff {
    data object Away : SwitchOff

    data class Other(val name: String) : SwitchOff
}

/** The words of a rule's summary, from the app's strings (the tests give their own). */
class FilterWords(
    val from: String,
    val recipient: String,
    val subject: String,
    val header: (String) -> String,
    val contains: String,
    val isExactly: String,
    val beginsWith: String,
    val endsWith: String,
    val and: String,
    val or: String,
    val moveTo: (String) -> String,
    val moveToUnknown: String,
    val markRead: String,
    val star: String,
    val trash: String,
    val stop: String,
    val noConditions: String,
    val noActions: String,
    val arrow: String,
    val separator: String,
)

/** One folder in the picker: [depth] is how far it is indented. */
data class FolderRow(val id: String, val name: String, val depth: Int)

/** The logic of the filter screens, free of Android so that it can be tested. */
object FilterRules {
    const val MAX_RULES = 200
    private val HEADER_NAME = Regex("[A-Za-z0-9-]{1,64}")

    fun kind(field: FilterField): FieldKind = when (field) {
        FilterField.From -> FieldKind.FROM
        FilterField.Recipient -> FieldKind.RECIPIENT
        FilterField.Subject -> FieldKind.SUBJECT
        is FilterField.Header -> FieldKind.HEADER
    }

    /** The field of [kind]; a header keeps [headerName] (what the user typed before, if anything). */
    fun field(kind: FieldKind, headerName: String = ""): FilterField = when (kind) {
        FieldKind.FROM -> FilterField.From
        FieldKind.RECIPIENT -> FilterField.Recipient
        FieldKind.SUBJECT -> FilterField.Subject
        FieldKind.HEADER -> FilterField.Header(headerName)
    }

    fun headerName(field: FilterField): String = (field as? FilterField.Header)?.name.orEmpty()

    fun blankCondition() = FilterCondition(FilterField.From, FilterMatch.CONTAINS, "")

    /** A rule to start from for a new entry: one empty condition, no action, on. */
    fun newRule() = FilterRule(
        id = "",
        name = "",
        enabled = true,
        matchAll = true,
        conditions = listOf(blankCondition()),
        moveTo = null,
        markRead = false,
        star = false,
        delete = false,
        stop = false,
        folderMissing = false,
    )

    fun hasAction(rule: FilterRule) = rule.moveTo != null || rule.markRead || rule.star || rule.delete || rule.stop

    /** Turning delete on switches the folder off (the mail goes to the Trash); turning it off leaves the folder empty. */
    fun withDelete(rule: FilterRule, delete: Boolean) =
        rule.copy(delete = delete, moveTo = if (delete) null else rule.moveTo, folderMissing = rule.folderMissing && !delete)

    fun withFolder(rule: FilterRule, folderId: String?) = rule.copy(moveTo = folderId, folderMissing = false, delete = rule.delete && folderId == null)

    fun withCondition(rule: FilterRule, index: Int, change: (FilterCondition) -> FilterCondition): FilterRule =
        rule.copy(conditions = rule.conditions.mapIndexed { i, c -> if (i == index) change(c) else c })

    fun addCondition(rule: FilterRule): FilterRule = rule.copy(conditions = rule.conditions + blankCondition())

    /** Removes a condition; the last one stays. */
    fun removeCondition(rule: FilterRule, index: Int): FilterRule =
        if (rule.conditions.size <= 1 || index !in rule.conditions.indices) rule else rule.copy(conditions = rule.conditions.filterIndexed { i, _ -> i != index })

    /** The first fault of [rule], or null. [folderIds] are the user's folders; [ruleCount] the rules in the list after the save. */
    fun check(rule: FilterRule, folderIds: Set<String>, ruleCount: Int): RuleProblem? {
        if (ruleCount > MAX_RULES) return RuleProblem.TooMany
        if (rule.conditions.isEmpty()) return RuleProblem.NoCondition
        rule.conditions.forEachIndexed { i, c ->
            if (c.value.isBlank()) return RuleProblem.BlankValue(i)
            if (c.value.any { it == '\n' || it == '\r' }) return RuleProblem.ManyLines(i)
            val field = c.field
            if (field is FilterField.Header && !HEADER_NAME.matches(field.name)) return RuleProblem.BadHeader(i)
        }
        if (rule.delete && rule.moveTo != null) return RuleProblem.DeleteAndMove
        if (!hasAction(rule)) return RuleProblem.NoAction
        val folder = rule.moveTo
        if (folder != null && (folder !in folderIds || rule.folderMissing)) return RuleProblem.FolderGone
        return null
    }

    /** The list with [rule] saved into it: replaced where its id is, or added at the top when it is new. */
    fun withRule(rules: List<FilterRule>, rule: FilterRule): List<FilterRule> {
        val at = if (rule.id.isEmpty()) -1 else rules.indexOfFirst { it.id == rule.id }
        return if (at < 0) listOf(rule) + rules else rules.mapIndexed { i, r -> if (i == at) rule else r }
    }

    fun withoutRule(rules: List<FilterRule>, id: String): List<FilterRule> = rules.filter { it.id != id }

    /** Moves the rule at [index] by [delta] (-1 up, 1 down); the same list when it cannot move. */
    fun move(rules: List<FilterRule>, index: Int, delta: Int): List<FilterRule> {
        val to = index + delta
        if (index !in rules.indices || to !in rules.indices || delta == 0) return rules
        val out = rules.toMutableList()
        out[index] = rules[to]
        out[to] = rules[index]
        return out
    }

    fun withEnabled(rules: List<FilterRule>, index: Int, enabled: Boolean): List<FilterRule> =
        rules.mapIndexed { i, r -> if (i == index) r.copy(enabled = enabled) else r }

    /** The banners for [filters], most important first; each only when it applies. */
    fun banners(filters: Filters): List<Banner> = buildList {
        if (filters.isEdited) add(Banner.Edited) else if (filters.isStale) add(Banner.Stale)
        if (filters.awayIsOn) add(Banner.AwayOn)
        filters.otherActive?.let { add(Banner.OtherScript(it)) }
    }

    /** What switching on has to ask about first; empty when nothing else is switched off. */
    fun askBeforeSwitchingOn(filters: Filters): List<SwitchOff> = buildList {
        if (filters.awayIsOn) add(SwitchOff.Away)
        filters.otherActive?.let { add(SwitchOff.Other(it)) }
    }

    /** A rule in words: "From contains boss → Move to Work, mark read". [folderName] gives a folder's name, or null when it is gone. */
    fun summary(rule: FilterRule, folderName: (String) -> String?, words: FilterWords): String {
        val joiner = if (rule.matchAll) words.and else words.or
        val conditions = rule.conditions.joinToString(joiner) { condition(it, words) }.ifEmpty { words.noConditions }
        val actions = buildList {
            rule.moveTo?.let { add(folderName(it)?.let(words.moveTo) ?: words.moveToUnknown) }
            if (rule.delete) add(words.trash)
            if (rule.markRead) add(words.markRead)
            if (rule.star) add(words.star)
            if (rule.stop) add(words.stop)
        }
        val text = if (actions.isEmpty()) words.noActions else actions.joinToString(words.separator).replaceFirstChar { it.uppercase() }
        return "$conditions ${words.arrow} $text"
    }

    private fun condition(c: FilterCondition, words: FilterWords): String {
        val field = when (val f = c.field) {
            FilterField.From -> words.from
            FilterField.Recipient -> words.recipient
            FilterField.Subject -> words.subject
            is FilterField.Header -> words.header(f.name)
        }
        val how = when (c.how) {
            FilterMatch.CONTAINS -> words.contains
            FilterMatch.IS -> words.isExactly
            FilterMatch.BEGINS_WITH -> words.beginsWith
            FilterMatch.ENDS_WITH -> words.endsWith
        }
        return "$field $how ${c.value}"
    }

    /**
     * The folders to pick from as a tree: children under their parent, in the server's order then by name;
     * the Drafts folder and what is inside it are left out. A folder whose parent is missing counts as top level.
     */
    fun folderTree(mailboxes: List<Mailbox>): List<FolderRow> {
        val drafts = HashSet<String>()
        mailboxes.filter { it.role == "drafts" }.forEach { drafts += it.id }
        // A folder inside Drafts goes too.
        var grew = true
        while (grew) {
            grew = false
            for (m in mailboxes) if (m.id !in drafts && m.parentId in drafts) { drafts += m.id; grew = true }
        }
        val usable = mailboxes.filter { it.id !in drafts }
        val ids = usable.mapTo(HashSet()) { it.id }
        val order = compareBy<Mailbox>({ it.sortOrder }, { it.name.lowercase() })
        val byParent = usable.groupBy { if (it.parentId in ids && it.parentId != it.id) it.parentId else null }
        val out = ArrayList<FolderRow>()
        val seen = HashSet<String>()
        fun walk(parent: String?, depth: Int) {
            for (m in byParent[parent].orEmpty().sortedWith(order)) {
                if (!seen.add(m.id)) continue
                out += FolderRow(m.id, m.name, depth)
                walk(m.id, depth + 1)
            }
        }
        walk(null, 0)
        // A loop of parents (never expected) would hide its folders; show them at the top.
        for (m in usable.sortedWith(order)) if (m.id !in seen) { seen += m.id; out += FolderRow(m.id, m.name, 0) }
        return out
    }

    /** The name of a folder, or null when it is not one of the user's. */
    fun folderName(mailboxes: List<Mailbox>, id: String): String? = mailboxes.firstOrNull { it.id == id }?.name
}
