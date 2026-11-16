package ovh.kweb.mail.ui.filters

// The rule model and its JSON follow the format of Bulwark webmail's filter generator
// (bulwarkmail/webmail, AGPL-3.0 with an app store permission; lib/sieve, components/filters), so that
// either client can edit the same script. The core writes the script; this file is the editor's side:
// a draft of one rule, its conversion to and from the JSON the core exchanges, the checks and the
// one-line summary. Free of Android so that it can be tested.

import java.time.LocalDateTime
import java.time.OffsetDateTime
import java.time.ZoneId
import java.time.ZoneOffset
import java.time.format.DateTimeFormatter
import java.time.format.DateTimeParseException
import java.util.UUID
import uniffi.kweb_core.FilterCapabilities
import uniffi.kweb_core.FilterField
import uniffi.kweb_core.FilterRule
import uniffi.kweb_core.Mailbox

/** One condition as the editor holds it; [orig] is the object it was read from (its other keys stay). */
data class CondDraft(
    val field: String,
    val comparator: String,
    /** The text as typed: several values are separated by commas (not for a regular expression). */
    val value: String,
    val headerName: String = "",
    val orig: Map<String, Any?> = emptyMap(),
)

data class ActDraft(
    val type: String,
    /** A folder's path, an address, a label or a text, by [type]. */
    val value: String = "",
    val mailboxId: String? = null,
    val keepCopy: Boolean = false,
    val orig: Map<String, Any?> = emptyMap(),
)

/**
 * One rule. [from] and [until] are the active period in local time ("yyyy-MM-dd HH:mm", empty for none);
 * [origFrom] and [origUntil] are what the rule had (so an unchanged period is written back as it was).
 */
data class RuleDraft(
    val id: String,
    val name: String,
    val enabled: Boolean,
    val matchAll: Boolean,
    val conditions: List<CondDraft>,
    val actions: List<ActDraft>,
    val stop: Boolean,
    val includeSpam: Boolean,
    val periodOn: Boolean = false,
    val from: String = "",
    val until: String = "",
    val origFrom: String? = null,
    val origUntil: String? = null,
    val orig: Map<String, Any?> = emptyMap(),
) {
    val hasPeriod: Boolean get() = origFrom != null || origUntil != null
}

/** A fault the editor sees before the core is asked (the core refuses the same things). Indexes are 0-based. */
sealed interface RuleProblem {
    data object EmptyName : RuleProblem
    data object NameTooLong : RuleProblem
    data object TooMany : RuleProblem
    data object NoConditions : RuleProblem
    data class BlankValue(val index: Int) : RuleProblem
    data class BadHeader(val index: Int) : RuleProblem
    data class BadSize(val index: Int) : RuleProblem
    data class BadRegex(val index: Int) : RuleProblem
    data class Unsupported(val index: Int) : RuleProblem
    data object NoActions : RuleProblem
    data class NoFolder(val index: Int) : RuleProblem
    data class BadAddress(val index: Int) : RuleProblem
    data class NoRejectText(val index: Int) : RuleProblem
    data class NoLabel(val index: Int) : RuleProblem
    data object PeriodIncomplete : RuleProblem
    data object PeriodInvalid : RuleProblem
    data object PeriodReversed : RuleProblem
}

/** One folder in the picker: [depth] is how far it is indented, [path] is what the script names. */
data class FolderRow(val id: String, val name: String, val depth: Int, val path: String)

/** The words of a summary, from the app's strings (the tests give their own). */
class FilterWords(
    val field: (field: String, headerName: String) -> String,
    val comparator: (String) -> String,
    val action: (String) -> String,
    val and: String,
    val or: String,
    val more: (Int) -> String,
    val arrow: String,
    val anyMail: String,
    val noActions: String,
)

object RuleModel {
    const val MAX_RULES = 500
    const val MAX_NAME = 200
    const val LOCAL_FORMAT = "yyyy-MM-dd HH:mm"
    private val localFormatter = DateTimeFormatter.ofPattern(LOCAL_FORMAT)
    private val utcFormatter = DateTimeFormatter.ofPattern("yyyy-MM-dd'T'HH:mm:ss.SSS'Z'")
    private val HEADER_NAME = Regex("[A-Za-z0-9_-]+")
    private val SIZE = Regex("[0-9]+[KMGkmg]?")
    private const val LOCAL_EMAIL_CHARS = ".!#$%&'*+/=?^_`{|}~-"

    val FIELDS = listOf("from", "to", "cc", "subject", "header", "size", "body", "attachment", "all")
    val ACTIONS = listOf("move", "copy", "forward", "mark_read", "star", "add_label", "discard", "reject", "keep")
    private val TEXT_COMPARATORS = listOf("contains", "not_contains", "is", "not_is", "starts_with", "ends_with", "matches", "regex")
    private val ADDRESS_COMPARATORS = TEXT_COMPARATORS + listOf("address_is", "domain_is")

    // ---- what the server allows ----

    fun fieldsFor(caps: FilterCapabilities, current: String? = null): List<String> =
        FIELDS.filter { it == current || (it != "body" || caps.body) && (it != "attachment" || caps.mime) }

    fun comparatorsFor(field: String, caps: FilterCapabilities, current: String? = null): List<String> {
        val all = when (field) {
            "from", "to", "cc" -> ADDRESS_COMPARATORS
            "subject", "header", "body" -> TEXT_COMPARATORS
            "size" -> listOf("greater_than", "less_than")
            "attachment" -> listOf("has_any", "has_type")
            else -> emptyList()
        }
        return all.filter { it == current || it != "regex" || caps.regex }
    }

    fun defaultComparator(field: String): String = when (field) {
        "size" -> "greater_than"
        "attachment" -> "has_any"
        else -> "contains"
    }

    /** Forwarding is offered unless the server's limit is 0 (a rule that forwards already keeps the type). */
    fun actionsFor(caps: FilterCapabilities, current: String? = null): List<String> =
        ACTIONS.filter { it == current || it != "forward" || caps.redirectLimit != 0u }

    fun needsValue(c: CondDraft) = c.field != "all" && c.comparator != "has_any"

    /** The condition after its field was changed: a comparator that does not fit is replaced. */
    fun withField(c: CondDraft, field: String, caps: FilterCapabilities): CondDraft {
        val ok = comparatorsFor(field, caps)
        val comparator = if (c.comparator in ok) c.comparator else defaultComparator(field)
        val keepsValue = field != "all" && comparator != "has_any"
        return c.copy(field = field, comparator = comparator, value = if (keepsValue) c.value else "")
    }

    fun withComparator(c: CondDraft, comparator: String) =
        c.copy(comparator = comparator, value = if (comparator == "has_any") "" else c.value)

    fun withActionType(a: ActDraft, type: String) =
        if (type == a.type) a else ActDraft(type = type, orig = a.orig.filterKeys { it !in setOf("value", "mailboxId", "keepCopy") })

    fun usesFolder(a: ActDraft) = a.type == "move" || a.type == "copy"

    fun spamOptionApplies(rule: RuleDraft, caps: FilterCapabilities) = caps.spamguard && rule.actions.any { usesFolder(it) }

    // ---- new rules ----

    fun blankCondition() = CondDraft("from", "contains", "")

    fun newRule(): RuleDraft = RuleDraft(
        id = UUID.randomUUID().toString(),
        name = "",
        enabled = true,
        matchAll = true,
        conditions = listOf(blankCondition()),
        actions = listOf(ActDraft("move")),
        stop = false,
        includeSpam = false,
    )

    /** A rule from the core's seed for "Filter mail like this" (the sender, or the mailing list). */
    fun fromSeed(seed: FilterRule): RuleDraft {
        val conditions = seed.conditions.map { c ->
            val value = c.value
            when (val f = c.field) {
                FilterField.From -> CondDraft("from", "contains", value)
                FilterField.Recipient -> CondDraft("to", "contains", value)
                FilterField.Subject -> CondDraft("subject", "contains", value)
                is FilterField.Header -> CondDraft("header", "contains", value, headerName = f.name)
            }
        }
        return newRule().copy(name = seed.name.take(MAX_NAME), conditions = conditions.ifEmpty { listOf(blankCondition()) })
    }

    // ---- JSON ----

    fun parseList(json: String, zone: ZoneId = ZoneId.systemDefault()): List<RuleDraft> =
        Json.arr(Json.parse(json)).orEmpty().mapNotNull { Json.obj(it)?.let { o -> fromMap(o, zone) } }

    fun listToJson(rules: List<RuleDraft>, zone: ZoneId = ZoneId.systemDefault()): String =
        Json.write(rules.map { toMap(it, zone) })

    private fun str(m: Map<String, Any?>, k: String): String? = m[k] as? String

    fun valueText(v: Any?): String = when (v) {
        is String -> v
        is List<*> -> v.filterIsInstance<String>().joinToString(", ")
        else -> ""
    }

    fun fromMap(m: Map<String, Any?>, zone: ZoneId = ZoneId.systemDefault()): RuleDraft? {
        val id = str(m, "id") ?: return null
        var stop = m["stopProcessing"] == true
        val conditions = Json.arr(m["conditions"]).orEmpty().mapNotNull { Json.obj(it) }.map { c ->
            CondDraft(str(c, "field").orEmpty(), str(c, "comparator").orEmpty(), valueText(c["value"]), str(c, "headerName").orEmpty(), c)
        }
        val actions = ArrayList<ActDraft>()
        for (a in Json.arr(m["actions"]).orEmpty().mapNotNull { Json.obj(it) }) {
            val type = str(a, "type").orEmpty()
            // An old `stop` action is the "Stop processing" box now.
            if (type == "stop") { stop = true; continue }
            actions += ActDraft(type, str(a, "value").orEmpty(), str(a, "mailboxId"), a["keepCopy"] == true, a)
        }
        val from = str(m, "activeFrom")
        val until = str(m, "activeUntil")
        return RuleDraft(
            id = id,
            name = str(m, "name").orEmpty(),
            enabled = m["enabled"] != false,
            matchAll = str(m, "matchType") != "any",
            conditions = conditions,
            actions = actions,
            stop = stop,
            includeSpam = m["includeSpam"] == true,
            periodOn = from != null || until != null,
            from = from?.let { toLocalText(it, zone) }.orEmpty(),
            until = until?.let { toLocalText(it, zone) }.orEmpty(),
            origFrom = from,
            origUntil = until,
            orig = m,
        )
    }

    /** The values of a condition as the core wants them: one stays a string, several are a list. */
    fun valueOut(c: CondDraft): Any {
        if (!needsValue(c)) return ""
        if (c.comparator == "regex" || c.field == "size") return c.value.trim()
        val parts = c.value.split(',').map { it.trim() }.filter { it.isNotEmpty() }
        return if (parts.size <= 1) parts.firstOrNull() ?: "" else parts
    }

    fun toMap(r: RuleDraft, zone: ZoneId = ZoneId.systemDefault()): Map<String, Any?> {
        val out = LinkedHashMap<String, Any?>(r.orig)
        out["id"] = r.id
        out["name"] = r.name
        out["enabled"] = r.enabled
        out["matchType"] = if (r.matchAll) "all" else "any"
        out["conditions"] = r.conditions.map { c ->
            val o = LinkedHashMap<String, Any?>(c.orig)
            o["field"] = c.field
            o["comparator"] = c.comparator
            o["value"] = valueOut(c)
            if (c.field == "header") o["headerName"] = c.headerName.trim() else o.remove("headerName")
            o
        }
        out["actions"] = r.actions.map { a ->
            val o = LinkedHashMap<String, Any?>(a.orig)
            o["type"] = a.type
            when (a.type) {
                "move", "copy" -> {
                    o["value"] = a.value
                    if (a.mailboxId != null) o["mailboxId"] = a.mailboxId else o.remove("mailboxId")
                    o.remove("keepCopy")
                }
                "forward" -> {
                    o["value"] = a.value.trim()
                    o.remove("mailboxId")
                    if (a.keepCopy) o["keepCopy"] = true else o.remove("keepCopy")
                }
                "reject", "add_label" -> {
                    o["value"] = a.value
                    o.remove("mailboxId")
                    o.remove("keepCopy")
                }
                else -> {
                    o.remove("value")
                    o.remove("mailboxId")
                    o.remove("keepCopy")
                }
            }
            o
        }
        out["stopProcessing"] = r.stop
        if (r.includeSpam && r.actions.any { usesFolder(it) }) out["includeSpam"] = true else out.remove("includeSpam")
        periodOut(out, "activeFrom", r.periodOn, r.from, r.origFrom, zone)
        periodOut(out, "activeUntil", r.periodOn, r.until, r.origUntil, zone)
        return out
    }

    private fun periodOut(out: MutableMap<String, Any?>, key: String, on: Boolean, local: String, orig: String?, zone: ZoneId) {
        val text = local.trim()
        if (!on || text.isEmpty()) {
            out.remove(key)
            return
        }
        // An unchanged boundary is written back as it was found.
        if (orig != null && toLocalText(orig, zone) == text) {
            out[key] = orig
            return
        }
        parseLocal(text, zone)?.let { out[key] = it }
    }

    // ---- the active period ----

    fun toLocalText(utc: String, zone: ZoneId): String? = try {
        OffsetDateTime.parse(utc).atZoneSameInstant(zone).toLocalDateTime().format(localFormatter)
    } catch (_: DateTimeParseException) {
        null
    }

    /** Local text to the UTC moment the script uses ("2026-10-06T08:00:00.000Z"), or null when it is not a date and time. */
    fun parseLocal(text: String, zone: ZoneId): String? = try {
        LocalDateTime.parse(text.trim(), localFormatter).atZone(zone).withZoneSameInstant(ZoneOffset.UTC).format(utcFormatter)
    } catch (_: DateTimeParseException) {
        null
    }

    // ---- checks ----

    /** Bulwark's `isValidEmail`. */
    fun isValidEmail(email: String): Boolean {
        if (email.isEmpty() || email.length > 254) return false
        if (email.any { it == '\r' || it == '\n' || it == '\u0000' || it == '<' || it == '>' }) return false
        val at = email.indexOf('@')
        if (at <= 0) return false
        val local = email.substring(0, at)
        val domain = email.substring(at + 1)
        if ('@' in domain) return false
        if (!local.all { it.code < 128 && it.isLetterOrDigit() || LOCAL_EMAIL_CHARS.contains(it) }) return false
        return domain.split('.').all { l ->
            l.isNotEmpty() && l.length <= 63 && l.all { it.code < 128 && (it.isLetterOrDigit() || it == '-') } && !l.startsWith('-') && !l.endsWith('-')
        }
    }

    /** The first fault of [rule], or null. [folderIds] are the user's own folders; [ruleCount] the rules after the save. */
    fun check(rule: RuleDraft, caps: FilterCapabilities, folderIds: Set<String>, ruleCount: Int, zone: ZoneId = ZoneId.systemDefault()): RuleProblem? {
        if (ruleCount > MAX_RULES) return RuleProblem.TooMany
        if (rule.name.isBlank()) return RuleProblem.EmptyName
        if (rule.name.length > MAX_NAME) return RuleProblem.NameTooLong
        if (rule.conditions.isEmpty()) return RuleProblem.NoConditions
        rule.conditions.forEachIndexed { i, c ->
            if (c.field != "all" && c.comparator !in comparatorsFor(c.field, caps, c.comparator)) return RuleProblem.Unsupported(i)
            if (c.comparator == "regex" && !caps.regex) return RuleProblem.Unsupported(i)
            if (c.field == "body" && !caps.body) return RuleProblem.Unsupported(i)
            if (c.field == "attachment" && !caps.mime) return RuleProblem.Unsupported(i)
            if (c.field == "header" && !HEADER_NAME.matches(c.headerName.trim())) return RuleProblem.BadHeader(i)
            if (!needsValue(c)) return@forEachIndexed
            val v = valueOut(c)
            if (v is String && v.isBlank()) return RuleProblem.BlankValue(i)
            if (c.field == "size" && !SIZE.matches(v as? String ?: "")) return RuleProblem.BadSize(i)
            if (c.comparator == "regex") {
                try { Regex(v as String) } catch (_: Exception) { return RuleProblem.BadRegex(i) }
            }
        }
        if (rule.actions.isEmpty()) return RuleProblem.NoActions
        rule.actions.forEachIndexed { i, a ->
            when (a.type) {
                "move", "copy" -> if (a.value.isBlank() || a.mailboxId == null || a.mailboxId !in folderIds) return RuleProblem.NoFolder(i)
                "forward" -> if (!isValidEmail(a.value.trim())) return RuleProblem.BadAddress(i)
                "reject" -> if (a.value.isBlank()) return RuleProblem.NoRejectText(i)
                "add_label" -> if (a.value.isBlank()) return RuleProblem.NoLabel(i)
            }
        }
        if (rule.periodOn) {
            val f = rule.from.trim()
            val u = rule.until.trim()
            if (f.isEmpty() || u.isEmpty()) return RuleProblem.PeriodIncomplete
            val pf = parseLocal(f, zone) ?: return RuleProblem.PeriodInvalid
            val pu = parseLocal(u, zone) ?: return RuleProblem.PeriodInvalid
            if (pf > pu) return RuleProblem.PeriodReversed
        }
        return null
    }

    // ---- the list ----

    /** The list with [rule] saved into it: replaced where its id is, or added at the end when it is new. */
    fun withRule(rules: List<RuleDraft>, rule: RuleDraft): List<RuleDraft> {
        val at = rules.indexOfFirst { it.id == rule.id }
        return if (at < 0) rules + rule else rules.mapIndexed { i, r -> if (i == at) rule else r }
    }

    // ---- the summary ----

    /** "From contains boss and Subject is x +1 → Move to Work, Mark as read". */
    fun summary(rule: RuleDraft, words: FilterWords): String {
        val joiner = if (rule.matchAll) words.and else words.or
        val shown = rule.conditions.take(2).joinToString(joiner) { condition(it, words) }.ifEmpty { words.anyMail }
        val extra = rule.conditions.size - 2
        val conditions = if (extra > 0) "$shown ${words.more(extra)}" else shown
        val acts = rule.actions.take(2).map { action(it, words) }
        val actionExtra = rule.actions.size - 2
        val actionText = when {
            acts.isEmpty() -> words.noActions
            actionExtra > 0 -> acts.joinToString(", ") + ", " + words.more(actionExtra)
            else -> acts.joinToString(", ")
        }
        return "$conditions ${words.arrow} $actionText"
    }

    private fun condition(c: CondDraft, words: FilterWords): String {
        if (c.field == "all") return words.anyMail
        val head = "${words.field(c.field, c.headerName.trim())} ${words.comparator(c.comparator)}"
        return if (needsValue(c)) "$head ${c.value.trim()}" else head
    }

    private fun action(a: ActDraft, words: FilterWords): String {
        val name = words.action(a.type)
        return when (a.type) {
            "move", "copy", "forward", "add_label" -> if (a.value.isBlank()) name else "$name ${a.value.trim()}"
            else -> name
        }
    }

    // ---- folders ----

    /**
     * The folders to pick from as a tree: children under their parent, in the server's order then by name; the Drafts
     * folder and what is inside it are left out. The path is the names from the top joined by "/" (as the script names it).
     */
    fun folderTree(mailboxes: List<Mailbox>): List<FolderRow> {
        val drafts = HashSet<String>()
        mailboxes.filter { it.role == "drafts" }.forEach { drafts += it.id }
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
        fun walk(parent: String?, depth: Int, prefix: String) {
            for (m in byParent[parent].orEmpty().sortedWith(order)) {
                if (!seen.add(m.id)) continue
                val path = if (prefix.isEmpty()) m.name else "$prefix/${m.name}"
                out += FolderRow(m.id, m.name, depth, path)
                walk(m.id, depth + 1, path)
            }
        }
        walk(null, 0, "")
        for (m in usable.sortedWith(order)) if (m.id !in seen) { seen += m.id; out += FolderRow(m.id, m.name, 0, m.name) }
        return out
    }

    /** The path of the folder [id] in [tree], or null when it is not one of the user's. */
    fun folderPath(tree: List<FolderRow>, id: String): String? = tree.firstOrNull { it.id == id }?.path
}
