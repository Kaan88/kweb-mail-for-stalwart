package ovh.kweb.mail.ui.filters

import java.time.ZoneId
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import uniffi.kweb_core.ExternalBlock
import uniffi.kweb_core.FilterCapabilities
import uniffi.kweb_core.FiltersState
import uniffi.kweb_core.Mailbox

internal fun caps(
    regex: Boolean = true,
    body: Boolean = true,
    mime: Boolean = true,
    period: Boolean = true,
    spamguard: Boolean = true,
    redirectLimit: UInt? = null,
) = FilterCapabilities(regex, body, mime, period, true, spamguard, true, redirectLimit, 0u, null)

internal fun coreState(
    rulesJson: String = "[]",
    active: Boolean = true,
    capabilities: FilterCapabilities = caps(),
    scriptName: String = "filters",
    supported: Boolean = true,
    opaque: Boolean = false,
    foreign: Boolean = false,
    rawText: String = "",
    vacationConflict: Boolean = false,
    migrationPending: Boolean = false,
    oldScriptPresent: Boolean = false,
    externalBlocks: List<ExternalBlock> = emptyList(),
) = FiltersState(
    supported = supported, scriptName = scriptName, active = active, opaque = opaque, foreign = foreign, rawText = rawText,
    rulesJson = rulesJson, externalBlocks = externalBlocks, capabilities = capabilities, vacationIncluded = false,
    vacationConflict = vacationConflict, displaced = false, differs = false, oldScriptPresent = oldScriptPresent,
    oldRulesJson = "[]", migrationPending = migrationPending, oldActive = false, vacationForward = false,
)

internal fun box(id: String, name: String, parent: String? = null, role: String? = null, order: UInt = 0u) =
    Mailbox(id, name, role, parent, order, 0u, 0u, false)

private val words = FilterWords(
    field = { f, h -> if (f == "header") "Header $h" else f.replaceFirstChar { it.uppercase() } },
    comparator = { it.replace('_', ' ') },
    action = { mapOf("move" to "Move to", "mark_read" to "Mark as read", "star" to "Star", "forward" to "Forward to", "discard" to "Discard")[it] ?: it },
    and = " and ", or = " or ", more = { "+$it" }, arrow = "→", anyMail = "All messages", noActions = "does nothing",
)

private val utc = ZoneId.of("UTC")

private const val RULE =
    """{"id":"r1","name":"Boss","enabled":true,"matchType":"all","conditions":[{"field":"from","comparator":"contains","value":"boss@x.org"}],"actions":[{"type":"move","value":"Work","mailboxId":"w"}],"stopProcessing":false}"""

private fun rule(json: String = RULE) = RuleModel.parseList("[$json]", utc).single()

class RuleModelTest {
    private val folders = setOf("w", "t")

    private fun problem(r: RuleDraft, c: FilterCapabilities = caps(), count: Int = 1) = RuleModel.check(r, c, folders, count, utc)

    @Test fun aRuleRoundTripsUnchanged() {
        val json = "[$RULE]"
        assertEquals(json, RuleModel.listToJson(RuleModel.parseList(json, utc), utc))
    }

    @Test fun unknownKeysAndTheirOrderSurviveAnEdit() {
        val json = """{"id":"r1","name":"A","enabled":true,"future":{"x":1},"matchType":"any","conditions":[{"field":"subject","comparator":"is","value":["a","b"],"extra":true}],"actions":[{"type":"star","note":"k"}],"stopProcessing":true}"""
        val r = rule(json)
        val out = Json.obj(Json.arr(Json.parse(RuleModel.listToJson(listOf(r.copy(name = "B")), utc)))!![0])!!
        assertEquals("B", out["name"])
        assertEquals(Json.parse("""{"x":1}"""), out["future"])
        val cond = Json.obj(Json.arr(out["conditions"])!![0])!!
        assertEquals(true, cond["extra"])
        assertEquals(listOf("a", "b"), cond["value"])
        assertEquals("k", Json.obj(Json.arr(out["actions"])!![0])!!["note"])
        assertEquals(listOf("id", "name", "enabled", "future", "matchType", "conditions", "actions", "stopProcessing"), out.keys.toList())
    }

    @Test fun severalValuesBecomeAListAndOneStaysAString() {
        val c = CondDraft("from", "contains", "a@x.org, b@x.org ,")
        assertEquals(listOf("a@x.org", "b@x.org"), RuleModel.valueOut(c))
        assertEquals("a@x.org", RuleModel.valueOut(c.copy(value = "a@x.org")))
        assertEquals("a,b", RuleModel.valueOut(c.copy(comparator = "regex", value = " a,b ")))
        assertEquals("", RuleModel.valueOut(c.copy(comparator = "has_any", value = "x")))
        assertEquals("", RuleModel.valueOut(c.copy(field = "all")))
    }

    @Test fun anOldStopActionBecomesTheStopBox() {
        val r = rule(RULE.replace("""],"stopProcessing":false""", """,{"type":"stop"}],"stopProcessing":false"""))
        assertTrue(r.stop)
        assertEquals(1, r.actions.size)
    }

    @Test fun aPeriodIsShownLocalAndWrittenUtc() {
        val ny = ZoneId.of("America/New_York")
        val json = RULE.replace(""","stopProcessing":false""", ""","stopProcessing":false,"activeFrom":"2026-12-24T18:00:00.000Z","activeUntil":"2026-12-26T18:00:00.000Z"""")
        val r = RuleModel.parseList("[$json]", ny).single()
        assertEquals("2026-12-24 13:00", r.from)
        assertTrue(r.hasPeriod)
        // Unchanged boundaries are written as they were found.
        assertEquals("[$json]", RuleModel.listToJson(listOf(r), ny))
        val changed = r.copy(from = "2026-12-24 14:30")
        val out = Json.obj(Json.arr(Json.parse(RuleModel.listToJson(listOf(changed), ny)))!![0])!!
        assertEquals("2026-12-24T19:30:00.000Z", out["activeFrom"])
        assertEquals("2026-12-26T18:00:00.000Z", out["activeUntil"])
        // Switched off: both go.
        val off = Json.obj(Json.arr(Json.parse(RuleModel.listToJson(listOf(r.copy(periodOn = false)), ny)))!![0])!!
        assertFalse(off.containsKey("activeFrom") || off.containsKey("activeUntil"))
    }

    @Test fun includeSpamIsWrittenOnlyWithAFolderAction() {
        val withSpam = rule().copy(includeSpam = true)
        assertTrue(RuleModel.toMap(withSpam, utc).containsKey("includeSpam"))
        val star = withSpam.copy(actions = listOf(ActDraft("star")))
        assertFalse(RuleModel.toMap(star, utc).containsKey("includeSpam"))
    }

    @Test fun actionsKeepOnlyWhatTheirKindNeeds() {
        val r = rule().copy(actions = listOf(ActDraft("forward", " a@b.org ", mailboxId = "w", keepCopy = true), ActDraft("discard", "x"), ActDraft("reject", "no")))
        val actions = Json.arr(RuleModel.toMap(r, utc)["actions"])!!.map { Json.obj(it)!! }
        assertEquals(mapOf("type" to "forward", "value" to "a@b.org", "keepCopy" to true), actions[0])
        assertEquals(mapOf("type" to "discard"), actions[1])
        assertEquals(mapOf("type" to "reject", "value" to "no"), actions[2])
    }

    @Test fun aGoodRulePassesAndEachFaultIsNamed() {
        val good = rule()
        assertNull(problem(good))
        assertEquals(RuleProblem.EmptyName, problem(good.copy(name = " ")))
        assertEquals(RuleProblem.NameTooLong, problem(good.copy(name = "x".repeat(201))))
        assertEquals(RuleProblem.NoConditions, problem(good.copy(conditions = emptyList())))
        assertEquals(RuleProblem.BlankValue(0), problem(good.copy(conditions = listOf(good.conditions[0].copy(value = " , ")))))
        assertEquals(RuleProblem.NoActions, problem(good.copy(actions = emptyList())))
        assertEquals(RuleProblem.TooMany, problem(good, count = 501))
    }

    @Test fun headerSizeAndRegexAreChecked() {
        val c = CondDraft("header", "contains", "x", headerName = "X Bad")
        val r = rule().copy(conditions = listOf(c))
        assertEquals(RuleProblem.BadHeader(0), problem(r))
        assertNull(problem(r.copy(conditions = listOf(c.copy(headerName = " X-List_Id ")))))
        val size = CondDraft("size", "greater_than", "5 MB")
        assertEquals(RuleProblem.BadSize(0), problem(r.copy(conditions = listOf(size))))
        assertNull(problem(r.copy(conditions = listOf(size.copy(value = "500K")))))
        val regex = CondDraft("subject", "regex", "(unclosed")
        assertEquals(RuleProblem.BadRegex(0), problem(r.copy(conditions = listOf(regex))))
        assertNull(problem(r.copy(conditions = listOf(regex.copy(value = "^a+b$")))))
        // all messages and has_any need no text.
        assertNull(problem(r.copy(conditions = listOf(CondDraft("all", "contains", "")))))
        assertNull(problem(r.copy(conditions = listOf(CondDraft("attachment", "has_any", "")))))
        assertEquals(RuleProblem.BlankValue(0), problem(r.copy(conditions = listOf(CondDraft("attachment", "has_type", "")))))
    }

    @Test fun whatTheServerLacksIsRefused() {
        val r = rule()
        val regex = r.copy(conditions = listOf(CondDraft("subject", "regex", "a")))
        assertEquals(RuleProblem.Unsupported(0), problem(regex, caps(regex = false)))
        val body = r.copy(conditions = listOf(CondDraft("body", "contains", "a")))
        assertEquals(RuleProblem.Unsupported(0), problem(body, caps(body = false)))
        val att = r.copy(conditions = listOf(CondDraft("attachment", "has_any", "")))
        assertEquals(RuleProblem.Unsupported(0), problem(att, caps(mime = false)))
    }

    @Test fun actionsAreChecked() {
        val r = rule()
        fun with(a: ActDraft) = problem(r.copy(actions = listOf(a)))
        assertEquals(RuleProblem.NoFolder(0), with(ActDraft("move")))
        assertEquals(RuleProblem.NoFolder(0), with(ActDraft("copy", "Gone", "gone")))
        assertNull(with(ActDraft("copy", "Work", "w")))
        assertEquals(RuleProblem.BadAddress(0), with(ActDraft("forward", "nope")))
        assertNull(with(ActDraft("forward", "me@example.org")))
        assertEquals(RuleProblem.NoRejectText(0), with(ActDraft("reject")))
        assertEquals(RuleProblem.NoLabel(0), with(ActDraft("add_label", " ")))
        assertNull(with(ActDraft("discard")))
        assertNull(with(ActDraft("keep")))
    }

    @Test fun thePeriodNeedsBothEndsInOrder() {
        val r = rule().copy(periodOn = true)
        assertEquals(RuleProblem.PeriodIncomplete, problem(r.copy(from = "2026-01-01 10:00")))
        assertEquals(RuleProblem.PeriodInvalid, problem(r.copy(from = "yesterday", until = "2026-01-01 10:00")))
        assertEquals(RuleProblem.PeriodReversed, problem(r.copy(from = "2026-01-02 10:00", until = "2026-01-01 10:00")))
        assertNull(problem(r.copy(from = "2026-01-01 10:00", until = "2026-01-02 10:00")))
    }

    @Test fun comparatorsFollowTheFieldAndTheServer() {
        val all = caps()
        assertEquals(listOf("greater_than", "less_than"), RuleModel.comparatorsFor("size", all))
        assertEquals(listOf("has_any", "has_type"), RuleModel.comparatorsFor("attachment", all))
        assertTrue("address_is" in RuleModel.comparatorsFor("from", all))
        assertFalse("address_is" in RuleModel.comparatorsFor("subject", all))
        assertTrue(RuleModel.comparatorsFor("all", all).isEmpty())
        assertFalse("regex" in RuleModel.comparatorsFor("subject", caps(regex = false)))
        // A rule that has one keeps it in the menu.
        assertTrue("regex" in RuleModel.comparatorsFor("subject", caps(regex = false), current = "regex"))
        assertFalse("body" in RuleModel.fieldsFor(caps(body = false)))
        assertFalse("attachment" in RuleModel.fieldsFor(caps(mime = false)))
        assertFalse("forward" in RuleModel.actionsFor(caps(redirectLimit = 0u)))
        assertTrue("forward" in RuleModel.actionsFor(caps(redirectLimit = 3u)))
        assertTrue("forward" in RuleModel.actionsFor(caps(redirectLimit = 0u), current = "forward"))
    }

    @Test fun changingTheFieldFixesTheComparator() {
        val c = CondDraft("from", "address_is", "a@b.org")
        val subject = RuleModel.withField(c, "subject", caps())
        assertEquals("contains", subject.comparator)
        assertEquals("a@b.org", subject.value)
        val size = RuleModel.withField(c, "size", caps())
        assertEquals("greater_than", size.comparator)
        assertEquals("", RuleModel.withField(c, "all", caps()).value)
        assertEquals("", RuleModel.withComparator(CondDraft("attachment", "has_type", "pdf"), "has_any").value)
    }

    @Test fun summaryShowsTwoConditionsAndTwoActionsThenCounts() {
        val r = rule().copy(
            conditions = listOf(CondDraft("from", "contains", "boss"), CondDraft("subject", "is", "hi"), CondDraft("header", "contains", "x", headerName = "X-A")),
            actions = listOf(ActDraft("move", "Work", "w"), ActDraft("mark_read"), ActDraft("star")),
        )
        assertEquals("From contains boss and Subject is hi +1 → Move to Work, Mark as read, +1", RuleModel.summary(r, words))
        val any = rule().copy(matchAll = false, conditions = listOf(CondDraft("from", "contains", "a"), CondDraft("to", "contains", "b")))
        assertEquals("From contains a or To contains b → Move to Work", RuleModel.summary(any, words))
        assertEquals("All messages → does nothing", RuleModel.summary(rule().copy(conditions = listOf(CondDraft("all", "contains", "")), actions = emptyList()), words))
        assertEquals("Header X-A contains x → Star", RuleModel.summary(rule().copy(conditions = listOf(CondDraft("header", "contains", "x", headerName = "X-A")), actions = listOf(ActDraft("star"))), words))
    }

    @Test fun foldersNestAndLeaveOutDraftsAndGiveThePath() {
        val boxes = listOf(box("in", "Inbox", role = "inbox"), box("d", "Drafts", role = "drafts"), box("dd", "Old", parent = "d"), box("w", "Work"), box("p", "Projects", parent = "w"))
        val tree = RuleModel.folderTree(boxes)
        assertEquals(listOf("Inbox", "Work", "Projects"), tree.map { it.name })
        assertEquals("Work/Projects", RuleModel.folderPath(tree, "p"))
        assertEquals(1, tree.first { it.id == "p" }.depth)
        assertNull(RuleModel.folderPath(tree, "d"))
    }

    @Test fun aRuleMadeFromAMessageStartsFromItsSender() {
        val seed = uniffi.kweb_core.FilterRule(
            "", "Ann", true, true,
            listOf(uniffi.kweb_core.FilterCondition(uniffi.kweb_core.FilterField.From, uniffi.kweb_core.FilterMatch.CONTAINS, "ann@x.org")),
            null, false, false, false, false, false,
        )
        val r = RuleModel.fromSeed(seed)
        assertEquals("Ann", r.name)
        assertEquals(CondDraft("from", "contains", "ann@x.org"), r.conditions[0])
        assertTrue(r.id.isNotEmpty())
        assertEquals(RuleProblem.NoFolder(0), problem(r))
        assertNotNull(RuleModel.listToJson(listOf(r), utc))
    }

    @Test fun addressesAreValidatedLikeTheCore() {
        assertTrue(RuleModel.isValidEmail("a.b+c@example.org"))
        assertFalse(RuleModel.isValidEmail("a@b@c"))
        assertFalse(RuleModel.isValidEmail("@x.org"))
        assertFalse(RuleModel.isValidEmail("a@-x.org"))
        assertFalse(RuleModel.isValidEmail("a b@x.org"))
    }
}

class JsonTest {
    @Test fun readsAndWritesWhatItReads() {
        val text = """{"a":[1,2.5,"x\né",true,null],"b":{"c":"\"q\""},"d":-3}"""
        val v = Json.parse(text)
        assertEquals(Json.parse(Json.write(v)), v)
        assertEquals(listOf("a", "b", "d"), Json.obj(v)!!.keys.toList())
        assertEquals(-3L, Json.obj(v)!!["d"])
    }

    @Test fun badTextIsRefused() {
        for (bad in listOf("{", "[1,]x", "{\"a\" 1}", "\"open", "tru", "[1] 2")) {
            try {
                Json.parse(bad)
                throw AssertionError("accepted $bad")
            } catch (_: Json.ParseError) {
            }
        }
    }
}

class BlockRulesTest {
    private val trash = FolderRow("t", "Trash", 0, "Trash")

    @Test fun theBlockRuleGoesFirstAndKeepsTheOthers() {
        val existing = """[{"id":"r1","name":"A","enabled":true,"future":1,"matchType":"all","conditions":[],"actions":[],"stopProcessing":false}]"""
        val out = BlockRules.withBlock(existing, "spam@x.org", trash)!!
        val list = Json.arr(Json.parse(out))!!
        assertEquals(2, list.size)
        val first = Json.obj(list[0])!!
        assertEquals("Blocked: spam@x.org", first["name"])
        assertEquals(true, first["stopProcessing"])
        val action = Json.obj(Json.arr(first["actions"])!![0])!!
        assertEquals(mapOf("type" to "move", "value" to "Trash", "mailboxId" to "t"), action)
        assertEquals(1L, Json.obj(list[1])!!["future"])
        // The editor reads the result back as rules it can show.
        assertEquals(2, RuleModel.parseList(out).size)
    }

    @Test fun aBlockedSenderIsNotBlockedTwice() {
        val once = BlockRules.withBlock("[]", "Spam@X.org", trash)!!
        assertNull(BlockRules.withBlock(once, "spam@x.org", trash))
    }

    @Test fun ownAddressesAreRecognised() {
        assertTrue(BlockRules.isOwn("Me@X.org ", listOf("me@x.org")))
        assertFalse(BlockRules.isOwn("joann@x.org", listOf("ann@x.org")))
    }
}
