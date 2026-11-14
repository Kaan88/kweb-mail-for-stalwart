package ovh.kweb.mail.ui.sharing

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import uniffi.kweb_core.FileEntry
import uniffi.kweb_core.FileKind
import uniffi.kweb_core.Person
import uniffi.kweb_core.Share
import uniffi.kweb_core.ShareLevel

private fun person(id: String, name: String = id, email: String = "") = Person(id, name, email)

private fun file(account: String?, kind: FileKind) =
    FileEntry(account, "f1", null, "Photos", kind, 0uL, "", null, null, true, true, true, false)

class ShareRulesTest {
    @Test fun draftsTrashAndJunkAreNeverOfferedButTheOtherFoldersAre() {
        assertFalse(ShareRules.mailboxShareable("drafts"))
        assertFalse(ShareRules.mailboxShareable("trash"))
        assertFalse(ShareRules.mailboxShareable("junk"))
        assertFalse(ShareRules.mailboxShareable("Trash"))
        assertTrue(ShareRules.mailboxShareable("inbox"))
        assertTrue(ShareRules.mailboxShareable("sent"))
        assertTrue(ShareRules.mailboxShareable("archive"))
        assertTrue(ShareRules.mailboxShareable(null))
    }

    @Test fun onlyAFolderOfTheOwnSpaceCanBeShared() {
        assertTrue(ShareRules.fileFolderShareable(file(null, FileKind.FOLDER)))
        assertFalse(ShareRules.fileFolderShareable(file("acc", FileKind.FOLDER)))
        assertFalse(ShareRules.fileFolderShareable(file(null, FileKind.FILE)))
    }

    @Test fun aLookupNeedsTwoCharactersNotCountingBlanks() {
        assertFalse(ShareRules.canSearch(""))
        assertFalse(ShareRules.canSearch("a"))
        assertFalse(ShareRules.canSearch("  a "))
        assertTrue(ShareRules.canSearch("al"))
        assertTrue(ShareRules.canSearch(" al "))
    }

    @Test fun peopleAlreadySharedWithAreNotOfferedAgainAndEachIsOfferedOnce() {
        val shares = listOf(Share(person("p1"), ShareLevel.READ))
        val answer = listOf(person("p1"), person("p2"), person("p2"), person("p3"))
        assertEquals(listOf("p2", "p3"), ShareRules.candidates(answer, shares).map { it.id })
    }

    @Test fun sharesAreOrderedByNameThenId() {
        val shares = listOf(
            Share(person("3", "bob"), ShareLevel.READ),
            Share(person("2", "Alice"), ShareLevel.WRITE),
            Share(person("1", "alice"), ShareLevel.READ),
        )
        assertEquals(listOf("1", "2", "3"), ShareRules.ordered(shares).map { it.person.id })
    }

    @Test fun aPersonIsNamedByNameThenAddressThenId() {
        assertEquals("Alice", ShareRules.label(person("1", "Alice", "a@x.org")))
        assertEquals("a@x.org", ShareRules.label(person("1", " ", "a@x.org")))
        assertEquals("p9", ShareRules.label(person("p9", "", "")))
    }

    @Test fun theAddressIsSaidUnderTheNameOnlyWhenItAddsSomething() {
        assertEquals("a@x.org", ShareRules.secondLine(person("1", "Alice", "a@x.org")))
        assertNull(ShareRules.secondLine(person("1", "a@x.org", "a@x.org")))
        assertNull(ShareRules.secondLine(person("1", "Alice", "")))
    }

    @Test fun theLevelsAreOfferedSeeFirst() {
        assertEquals(listOf(ShareLevel.READ, ShareLevel.WRITE), ShareRules.LEVELS)
    }
}
