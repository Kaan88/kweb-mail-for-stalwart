package ovh.kweb.mail.ui.mailextras

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import uniffi.kweb_core.Label
import uniffi.kweb_core.MessageLabels

class LabelRulesTest {
    private val work = Label("work", "Work")
    private val home = Label("home", "Home")
    private val bills = Label("bills", "Bills")
    private val all = listOf(work, home, bills)

    @Test fun aGoodNameHasNoProblem() {
        assertNull(LabelRules.nameProblem("Work"))
        assertNull(LabelRules.nameProblem("  Tax 2026  "))
        assertNull(LabelRules.nameProblem("a".repeat(60)))
    }

    @Test fun blankTooLongAndMultilineNamesAreRefused() {
        assertEquals(LabelRules.NameProblem.BLANK, LabelRules.nameProblem(""))
        assertEquals(LabelRules.NameProblem.BLANK, LabelRules.nameProblem("   "))
        assertEquals(LabelRules.NameProblem.TOO_LONG, LabelRules.nameProblem("a".repeat(61)))
        assertEquals(LabelRules.NameProblem.MULTILINE, LabelRules.nameProblem("a\nb"))
    }

    @Test fun lengthCountsCharactersNotUnits() {
        // 60 emoji are 120 UTF-16 units but 60 characters.
        assertNull(LabelRules.nameProblem("😀".repeat(60)))
        assertEquals(LabelRules.NameProblem.TOO_LONG, LabelRules.nameProblem("😀".repeat(61)))
    }

    @Test fun carryTellsAllSomeOrNone() {
        val by = mapOf("a" to listOf("work"), "b" to listOf("work", "home"), "c" to emptyList())
        assertEquals(Carry.ALL, LabelRules.carry(listOf("a", "b"), by, "work"))
        assertEquals(Carry.SOME, LabelRules.carry(listOf("a", "b", "c"), by, "work"))
        assertEquals(Carry.NONE, LabelRules.carry(listOf("a", "c"), by, "home"))
        assertEquals(Carry.NONE, LabelRules.carry(listOf("x"), by, "work"))
        assertEquals(Carry.NONE, LabelRules.carry(emptyList(), by, "work"))
    }

    @Test fun aTapTakesTheLabelOffOnlyWhenEveryMessageHasIt() {
        assertFalse(LabelRules.turnOn(Carry.ALL))
        assertTrue(LabelRules.turnOn(Carry.SOME))
        assertTrue(LabelRules.turnOn(Carry.NONE))
    }

    @Test fun namesFollowTheOrderOfTheUsersListAndSkipUnknownLabels() {
        assertEquals(listOf("Work", "Bills"), LabelRules.namesOf(listOf("bills", "gone", "work"), all))
        assertEquals(emptyList<String>(), LabelRules.namesOf(null, all))
        assertEquals(emptyList<String>(), LabelRules.namesOf(emptyList(), all))
    }

    @Test fun aRowOfSeveralMessagesShowsEveryLabelOnce() {
        val by = mapOf("a" to listOf("home"), "b" to listOf("home", "work"))
        assertEquals(listOf("Work", "Home"), LabelRules.namesOfAny(listOf("a", "b", "c"), by, all))
    }

    @Test fun rowChipsAreCappedWithACount() {
        assertEquals(listOf("a", "b") to 0, LabelRules.shown(listOf("a", "b")))
        assertEquals(listOf("a", "b", "c") to 2, LabelRules.shown(listOf("a", "b", "c", "d", "e")))
    }

    @Test fun theAnswerFillsInMessagesItLeavesOut() {
        val index = LabelRules.index(listOf("a", "b"), listOf(MessageLabels("a", listOf("work"))))
        assertEquals(mapOf("a" to listOf("work"), "b" to emptyList<String>()), index)
    }

    @Test fun applyingAChangeShowsItAtOnce() {
        val by = mapOf("a" to listOf("work"))
        val on = LabelRules.applied(by, listOf("a", "b"), "home", true)
        assertEquals(listOf("work", "home"), on["a"])
        assertEquals(listOf("home"), on["b"])
        assertEquals(listOf("work"), LabelRules.applied(on, listOf("a"), "home", false)["a"])
        // Putting it on twice does not double it.
        assertEquals(listOf("work", "home"), LabelRules.applied(on, listOf("a"), "home", true)["a"])
        // The map that was given is not changed.
        assertEquals(mapOf("a" to listOf("work")), by)
    }

    @Test fun aMessageWithNoEntryHasNotBeenRead() {
        assertEquals(false, LabelRules.known(listOf("a", "b"), mapOf("a" to emptyList())))
        assertEquals(true, LabelRules.known(listOf("a", "b"), mapOf("a" to emptyList(), "b" to listOf("x"))))
        assertEquals(false, LabelRules.known(emptyList(), emptyMap()))
    }

    @Test fun mailThatLostTheLabelDropsOutOfTheListAndUnreadMailStays() {
        val by = mapOf("a" to listOf("work"), "b" to listOf("home"), "c" to emptyList())
        assertEquals(listOf("a", "d"), LabelRules.stillCarrying(listOf("a", "b", "c", "d"), by, "work"))
    }
}
