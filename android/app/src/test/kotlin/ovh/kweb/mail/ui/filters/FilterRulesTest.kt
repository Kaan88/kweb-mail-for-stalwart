package ovh.kweb.mail.ui.filters

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertSame
import org.junit.Assert.assertTrue
import org.junit.Test
import uniffi.kweb_core.FilterCondition
import uniffi.kweb_core.FilterField
import uniffi.kweb_core.FilterMatch
import uniffi.kweb_core.FilterRule
import uniffi.kweb_core.Filters
import uniffi.kweb_core.Mailbox

internal fun rule(
    id: String = "r1",
    name: String = "Boss",
    conditions: List<FilterCondition> = listOf(FilterCondition(FilterField.From, FilterMatch.CONTAINS, "boss")),
    moveTo: String? = null,
    markRead: Boolean = false,
    star: Boolean = false,
    delete: Boolean = false,
    stop: Boolean = false,
    enabled: Boolean = true,
    matchAll: Boolean = true,
    folderMissing: Boolean = false,
) = FilterRule(id, name, enabled, matchAll, conditions, moveTo, markRead, star, delete, stop, folderMissing)

internal fun filters(
    rules: List<FilterRule> = emptyList(),
    active: Boolean = false,
    stale: Boolean = false,
    edited: Boolean = false,
    away: Boolean = false,
    other: String? = null,
    text: String = "# script",
) = Filters(rules, active, stale, edited, text, away, other, other?.let { "text" })

internal fun box(id: String, name: String, parent: String? = null, role: String? = null, order: UInt = 0u) =
    Mailbox(id, name, role, parent, order, 0u, 0u, false)

private val words = FilterWords(
    from = "From", recipient = "To or Cc", subject = "Subject", header = { "$it header" },
    contains = "contains", isExactly = "is", beginsWith = "begins with", endsWith = "ends with",
    and = " and ", or = " or ", moveTo = { "Move to $it" }, moveToUnknown = "Move to a missing folder",
    markRead = "mark read", star = "star", trash = "Move to Trash", stop = "stop",
    noConditions = "Any mail", noActions = "does nothing", arrow = "→", separator = ", ",
)

class FilterRulesTest {
    private val folders = setOf("work", "box")
    private fun problem(r: FilterRule, count: Int = 1) = FilterRules.check(r, folders, count)

    @Test fun summaryNamesConditionAndActions() {
        val r = rule(moveTo = "work", markRead = true)
        assertEquals("From contains boss → Move to Work, mark read", FilterRules.summary(r, { if (it == "work") "Work" else null }, words))
    }

    @Test fun summaryJoinsConditionsByMode() {
        val cs = listOf(
            FilterCondition(FilterField.Subject, FilterMatch.BEGINS_WITH, "Invoice"),
            FilterCondition(FilterField.Header(" List-Id".trim()), FilterMatch.IS, "x"),
        )
        assertEquals("Subject begins with Invoice and List-Id header is x → Star", FilterRules.summary(rule(conditions = cs, star = true), { null }, words))
        assertEquals("Subject begins with Invoice or List-Id header is x → Star", FilterRules.summary(rule(conditions = cs, star = true, matchAll = false), { null }, words))
    }

    @Test fun summaryTakesSeparatorAndArrowFromTheWords() {
        val w = FilterWords(
            from = "From", recipient = "To", subject = "S", header = { it }, contains = "has", isExactly = "is", beginsWith = "b", endsWith = "e",
            and = "&", or = "|", moveTo = { it }, moveToUnknown = "?", markRead = "read", star = "star", trash = "t", stop = "s",
            noConditions = "-", noActions = "-", arrow = "=>", separator = "; ",
        )
        assertEquals("From has boss => Read; star", FilterRules.summary(rule(markRead = true, star = true), { null }, w))
    }

    @Test fun summaryCapitalisesFirstActionAndSaysDeleteStopAndMissingFolder() {
        assertEquals("From contains boss → Mark read, star, stop", FilterRules.summary(rule(markRead = true, star = true, stop = true), { null }, words))
        assertEquals("From contains boss → Move to Trash", FilterRules.summary(rule(delete = true), { null }, words))
        assertEquals("From contains boss → Move to a missing folder", FilterRules.summary(rule(moveTo = "gone"), { null }, words))
        assertEquals("From contains boss → does nothing", FilterRules.summary(rule(), { null }, words))
    }

    @Test fun checkAcceptsAGoodRule() {
        assertNull(problem(rule(moveTo = "work")))
        assertNull(problem(rule(stop = true)))
    }

    @Test fun checkFindsEachFault() {
        assertEquals(RuleProblem.NoCondition, problem(rule(conditions = emptyList(), star = true)))
        assertEquals(RuleProblem.BlankValue(1), problem(rule(conditions = listOf(FilterCondition(FilterField.From, FilterMatch.IS, "a"), FilterCondition(FilterField.Subject, FilterMatch.IS, "  ")), star = true)))
        assertEquals(RuleProblem.ManyLines(0), problem(rule(conditions = listOf(FilterCondition(FilterField.From, FilterMatch.IS, "a\nb")), star = true)))
        assertEquals(RuleProblem.BadHeader(0), problem(rule(conditions = listOf(FilterCondition(FilterField.Header("Bad Name"), FilterMatch.IS, "a")), star = true)))
        assertEquals(RuleProblem.BadHeader(0), problem(rule(conditions = listOf(FilterCondition(FilterField.Header(""), FilterMatch.IS, "a")), star = true)))
        assertEquals(RuleProblem.DeleteAndMove, problem(rule(delete = true, moveTo = "work")))
        assertEquals(RuleProblem.NoAction, problem(rule()))
        assertEquals(RuleProblem.FolderGone, problem(rule(moveTo = "gone")))
        assertEquals(RuleProblem.FolderGone, problem(rule(moveTo = "work", folderMissing = true)))
        assertEquals(RuleProblem.TooMany, problem(rule(star = true), count = 201))
        assertNull(problem(rule(star = true), count = 200))
    }

    @Test fun deleteSwitchesTheFolderOff() {
        val r = FilterRules.withDelete(rule(moveTo = "work", folderMissing = true), true)
        assertTrue(r.delete)
        assertNull(r.moveTo)
        assertEquals(false, r.folderMissing)
        assertNull(problem(r))
        assertEquals("work", FilterRules.withFolder(FilterRules.withDelete(r, false), "work").moveTo)
    }

    @Test fun conditionsAreAddedAndRemovedButOneStays() {
        var r = FilterRules.newRule()
        assertEquals(1, r.conditions.size)
        r = FilterRules.addCondition(r)
        assertEquals(2, r.conditions.size)
        r = FilterRules.removeCondition(r, 0)
        assertEquals(1, r.conditions.size)
        assertSame(r, FilterRules.removeCondition(r, 0))
    }

    @Test fun fieldKindKeepsTheHeaderName() {
        assertEquals(FieldKind.HEADER, FilterRules.kind(FilterField.Header("X-A")))
        assertEquals(FilterField.Header("X"), FilterRules.field(FieldKind.HEADER, "X"))
        assertEquals(FilterField.Subject, FilterRules.field(FieldKind.SUBJECT, "X"))
    }

    @Test fun movingARuleUpAndDown() {
        val a = rule("a"); val b = rule("b"); val c = rule("c")
        val list = listOf(a, b, c)
        assertEquals(listOf(b, a, c), FilterRules.move(list, 1, -1))
        assertEquals(listOf(a, c, b), FilterRules.move(list, 1, 1))
        assertSame(list, FilterRules.move(list, 0, -1))
        assertSame(list, FilterRules.move(list, 2, 1))
        assertSame(list, FilterRules.move(list, 5, 1))
    }

    @Test fun aNewRuleGoesOnTopAndAnOldOneIsReplaced() {
        val a = rule("a"); val b = rule("b")
        val fresh = rule("", name = "New")
        assertEquals(listOf(fresh, a, b), FilterRules.withRule(listOf(a, b), fresh))
        val changed = b.copy(name = "B2")
        assertEquals(listOf(a, changed), FilterRules.withRule(listOf(a, b), changed))
        assertEquals(listOf(a), FilterRules.withoutRule(listOf(a, b), "b"))
        assertEquals(listOf(a, b.copy(enabled = false)), FilterRules.withEnabled(listOf(a, b), 1, false))
    }

    @Test fun bannersShowOnlyWhatApplies() {
        assertEquals(emptyList<Banner>(), FilterRules.banners(filters()))
        assertEquals(listOf(Banner.Stale), FilterRules.banners(filters(stale = true)))
        assertEquals(listOf(Banner.Edited), FilterRules.banners(filters(stale = true, edited = true)))
        assertEquals(listOf(Banner.AwayOn, Banner.OtherScript("vacation")), FilterRules.banners(filters(away = true, other = "vacation")))
    }

    @Test fun switchingOnAsksAboutWhatItSwitchesOff() {
        assertEquals(emptyList<SwitchOff>(), FilterRules.askBeforeSwitchingOn(filters()))
        assertEquals(listOf(SwitchOff.Away), FilterRules.askBeforeSwitchingOn(filters(away = true)))
        assertEquals(listOf(SwitchOff.Away, SwitchOff.Other("mine")), FilterRules.askBeforeSwitchingOn(filters(away = true, other = "mine")))
    }

    @Test fun folderTreeNestsAndLeavesOutDrafts() {
        val boxes = listOf(
            box("in", "Inbox", role = "inbox", order = 1u),
            box("dr", "Drafts", role = "drafts", order = 2u),
            box("drsub", "Old drafts", parent = "dr"),
            box("w", "Work", order = 5u),
            box("wb", "Beta", parent = "w"),
            box("wa", "alpha", parent = "w"),
            box("orph", "Orphan", parent = "missing", order = 9u),
        )
        val rows = FilterRules.folderTree(boxes)
        assertEquals(listOf("Inbox", "Work", "alpha", "Beta", "Orphan"), rows.map { it.name })
        assertEquals(listOf(0, 0, 1, 1, 0), rows.map { it.depth })
        assertNull(rows.firstOrNull { it.id == "dr" || it.id == "drsub" })
    }

    @Test fun folderTreeSurvivesALoop() {
        val rows = FilterRules.folderTree(listOf(box("a", "A", parent = "b"), box("b", "B", parent = "a")))
        assertEquals(setOf("a", "b"), rows.map { it.id }.toSet())
    }
}
