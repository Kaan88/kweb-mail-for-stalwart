package ovh.kweb.mail.background

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import ovh.kweb.mail.background.MailAction.Archive
import ovh.kweb.mail.background.MailAction.Delete
import ovh.kweb.mail.background.MailAction.MarkRead
import ovh.kweb.mail.background.MailAction.Reply
import uniffi.kweb_core.Action
import uniffi.kweb_core.Mailbox

class NotificationActionRulesTest {
    private val rules = NotificationActionRules

    @Test
    fun theDefaultIsDeleteMarkReadReply() {
        assertEquals(listOf(Delete, MarkRead, Reply), rules.DEFAULT)
    }

    // The migration: an install from before the setting stored nothing, and gets the default (Delete, not Archive).
    @Test
    fun nothingStoredIsTheDefault() {
        assertEquals(rules.DEFAULT, rules.parse(null))
        assertEquals(rules.DEFAULT, Settings(MapValues()).notificationActions)
    }

    @Test
    fun anEmptyTextIsTheChoiceOfNoButtonsNotTheDefault() {
        assertEquals(emptyList<MailAction>(), rules.parse(""))
        assertEquals("", rules.encode(emptyList()))
    }

    @Test
    fun aChoiceComesBackInItsOrder() {
        for (chosen in listOf(listOf(Reply, Archive, Delete), listOf(MarkRead), listOf(Archive, MarkRead, Reply), emptyList())) {
            assertEquals(chosen, rules.parse(rules.encode(chosen)))
        }
    }

    // The keys are what is on the disk of every install: they must never change.
    @Test
    fun theStoredKeysArePinned() {
        assertEquals("archive,delete,read", rules.encode(listOf(Archive, Delete, MarkRead)))
        assertEquals("reply", rules.encode(listOf(Reply)))
    }

    @Test
    fun unknownKeysRepeatsAndAFourthAreLeftOut() {
        assertEquals(listOf(Delete, Reply), rules.parse("delete,snooze,reply"))
        assertEquals(listOf(Delete, Reply), rules.parse("delete, reply ,delete"))
        assertEquals(listOf(Archive, Delete, MarkRead), rules.parse("archive,delete,read,reply"))
        assertEquals(emptyList<MailAction>(), rules.parse("nonsense"))
        assertEquals("archive,delete,read", rules.encode(listOf(Archive, Delete, Archive, MarkRead, Reply)))
    }

    @Test
    fun switchingOnAddsAtTheEndAndOffRemoves() {
        assertEquals(listOf(Delete, Archive), rules.set(listOf(Delete), Archive, on = true))
        assertEquals(listOf(Delete), rules.set(listOf(Delete), Delete, on = true))
        assertEquals(listOf(Delete, Reply), rules.set(rules.DEFAULT, MarkRead, on = false))
        assertEquals(listOf(Delete), rules.set(listOf(Delete), Archive, on = false))
        assertEquals(emptyList<MailAction>(), rules.set(listOf(Delete), Delete, on = false))
    }

    @Test
    fun aFourthCannotBeSwitchedOn() {
        assertFalse(rules.hasRoom(rules.DEFAULT))
        assertEquals(rules.DEFAULT, rules.set(rules.DEFAULT, Archive, on = true))
        assertTrue(rules.hasRoom(listOf(Delete, Reply)))
        assertTrue(rules.hasRoom(emptyList()))
    }

    @Test
    fun movingSwapsNeighboursAndStopsAtTheEnds() {
        assertEquals(listOf(MarkRead, Delete, Reply), rules.move(rules.DEFAULT, MarkRead, up = true))
        assertEquals(listOf(Delete, Reply, MarkRead), rules.move(rules.DEFAULT, MarkRead, up = false))
        assertEquals(rules.DEFAULT, rules.move(rules.DEFAULT, Delete, up = true))
        assertEquals(rules.DEFAULT, rules.move(rules.DEFAULT, Reply, up = false))
        assertEquals(rules.DEFAULT, rules.move(rules.DEFAULT, Archive, up = true))
    }

    @Test
    fun theRowsAreTheChosenInOrderThenTheRest() {
        val rows = rules.rows(rules.DEFAULT)
        assertEquals(listOf(Delete, MarkRead, Reply, Archive), rows.map { it.action })
        assertEquals(listOf(0, 1, 2, null), rows.map { it.position })
        assertEquals(listOf(false, true, true, false), rows.map { it.canMoveUp })
        assertEquals(listOf(true, true, false, false), rows.map { it.canMoveDown })
        // Three are on: the fourth cannot be switched on, the three can be switched off.
        assertEquals(listOf(true, true, true, false), rows.map { it.canSwitch })
    }

    @Test
    fun withRoomEveryRowCanBeSwitchedAndNoneChosenListsAllInTheirOwnOrder() {
        val some = rules.rows(listOf(Reply))
        assertEquals(listOf(Reply, Archive, Delete, MarkRead), some.map { it.action })
        assertTrue(some.all { it.canSwitch })
        assertFalse(some.first().canMoveUp || some.first().canMoveDown)
        val none = rules.rows(emptyList())
        assertEquals(MailAction.entries.toList(), none.map { it.action })
        assertTrue(none.all { it.position == null && it.canSwitch })
    }

    @Test
    fun theSettingIsStoredPerAccountAndCanGoBackToNeverChosen() {
        val values = MapValues()
        val settings = Settings(values)
        val other = Settings(MapValues())
        val before = settings.changes.value
        settings.notificationActions = listOf(Archive, Reply)
        assertEquals(listOf(Archive, Reply), settings.notificationActions)
        assertEquals("archive,reply", values.map["notification_actions"])
        assertTrue(settings.changes.value > before)
        assertEquals(rules.DEFAULT, other.notificationActions)
        settings.notificationActions = emptyList()
        assertEquals(emptyList<MailAction>(), settings.notificationActions)
        settings.resetNotificationActions()
        assertNull(values.map["notification_actions"])
        assertEquals(rules.DEFAULT, settings.notificationActions)
    }

    @Test
    fun aChoiceOfAnOlderInstallIsTakenOverAndNoChoiceStaysNoChoice() {
        val old = Settings(MapValues())
        old.notificationActions = listOf(Archive)
        val fresh = Settings(MapValues())
        fresh.takeSwitchesFrom(old)
        assertEquals(listOf(Archive), fresh.notificationActions)

        val values = MapValues()
        Settings(values).takeSwitchesFrom(Settings(MapValues()))
        assertNull("never chosen stays never chosen", values.map["notification_actions"])
    }

    // The Delete button must never delete for good: it is a move to the Trash, which leaves mail in the Trash alone.
    @Test
    fun theDeleteButtonIsAMoveToTheTrashFolderAndNeverTheCoresDelete() {
        fun box(id: String, role: String?) = Mailbox(id, id, role, null, 0u, 0u, 0u, false)
        val boxes = listOf(box("a", "inbox"), box("b", null), box("t9", "trash"), box("c", "archive"))
        assertEquals(Action.Move("m1", "t9"), rules.deleteAction("m1", boxes))
        assertNull("no Trash folder: nothing is submitted", rules.deleteAction("m1", boxes.filter { it.role != "trash" }))
        assertNull(rules.deleteAction("m1", emptyList()))
    }

    @Test
    fun onlyANotificationWithOtherButtonsIsPostedAgain() {
        assertFalse("has them already", rules.needsRefresh("m1", "delete,read,reply", rules.DEFAULT))
        assertTrue(rules.needsRefresh("m1", "archive,read,reply", rules.DEFAULT))
        assertTrue("another order", rules.needsRefresh("m1", "read,delete,reply", rules.DEFAULT))
        assertFalse("no buttons chosen and none shown", rules.needsRefresh("m1", "", emptyList()))
        assertTrue(rules.needsRefresh("m1", "", rules.DEFAULT))
        assertTrue("posted before the buttons were recorded", rules.needsRefresh("m1", null, rules.DEFAULT))
        assertFalse("posted before the setting: it cannot be built again", rules.needsRefresh(null, null, rules.DEFAULT))
    }
}
