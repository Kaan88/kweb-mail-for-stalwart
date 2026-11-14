package ovh.kweb.mail.ui.files

import java.time.ZoneId
import java.util.Locale
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import uniffi.kweb_core.FileEntry
import uniffi.kweb_core.FileKind

internal fun entry(
    id: String = "1",
    name: String = "a.txt",
    kind: FileKind = FileKind.FILE,
    parentId: String? = null,
    size: ULong = 0uL,
    mediaType: String = "text/plain",
    modified: Long? = null,
    mayRename: Boolean = true,
    mayDelete: Boolean = true,
    mayReplace: Boolean = true,
) = FileEntry(
    accountId = null,
    id = id,
    parentId = parentId,
    name = name,
    kind = kind,
    size = size,
    mediaType = mediaType,
    blobId = if (kind == FileKind.FOLDER) null else "blob-$id",
    modified = modified,
    mayAdd = kind == FileKind.FOLDER,
    mayRename = mayRename,
    mayDelete = mayDelete,
    mayReplace = mayReplace,
)

class FileRulesTest {
    private val en = Locale.ENGLISH

    @Test fun smallSizesAreBytes() {
        assertEquals("0 B", FileRules.sizeText(0uL, en))
        assertEquals("999 B", FileRules.sizeText(999uL, en))
    }

    @Test fun sizesUseThousandsAndOneDecimalBelowTen() {
        assertEquals("1.0 KB", FileRules.sizeText(1000uL, en))
        assertEquals("1.5 KB", FileRules.sizeText(1500uL, en))
        assertEquals("2.1 MB", FileRules.sizeText(2_097_152uL, en))
    }

    @Test fun sizesFromTenUpHaveNoDecimal() {
        assertEquals("25 MB", FileRules.sizeText(25_000_000uL, en))
        assertEquals("50 MB", FileRules.sizeText(50_000_000uL, en))
        assertEquals("12 KB", FileRules.sizeText(12_345uL, en))
    }

    @Test fun aSizeJustBelowTheNextUnitRoundsUp() = assertEquals("1.0 MB", FileRules.sizeText(999_990uL, en))

    @Test fun theDecimalMarkFollowsTheLocale() = assertEquals("1,5 KB", FileRules.sizeText(1500uL, Locale.GERMANY))

    @Test fun aDateIsTheLocalDayInTheZone() {
        // 2026-11-14 23:30 UTC is already the 15th in Istanbul.
        val seconds = 1_794_699_000L
        assertEquals("Nov 14, 2026", FileRules.dateText(seconds, ZoneId.of("UTC"), en))
        assertEquals("Nov 15, 2026", FileRules.dateText(seconds, ZoneId.of("Europe/Istanbul"), en))
    }

    @Test fun aFileHasSizeAndDateOnItsLine() {
        val e = entry(size = 1500uL, modified = 1_794_699_000L)
        assertEquals("1.5 KB · Nov 14, 2026", FileRules.subtitle(e, ZoneId.of("UTC"), en))
    }

    @Test fun aFileWithoutADateHasOnlyItsSize() = assertEquals("12 B", FileRules.subtitle(entry(size = 12uL), ZoneId.of("UTC"), en))

    @Test fun aFolderHasNoLine() = assertEquals("", FileRules.subtitle(entry(kind = FileKind.FOLDER), ZoneId.of("UTC"), en))

    @Test fun iconsFollowKindAndMediaType() {
        assertEquals(FileIcon.Folder, FileRules.iconFor(FileKind.FOLDER, "", "Docs"))
        assertEquals(FileIcon.Other, FileRules.iconFor(FileKind.OTHER, "", "link"))
        assertEquals(FileIcon.Picture, FileRules.iconFor(FileKind.FILE, "image/png", "x"))
        assertEquals(FileIcon.Pdf, FileRules.iconFor(FileKind.FILE, "application/pdf", "x"))
        assertEquals(FileIcon.Text, FileRules.iconFor(FileKind.FILE, "text/markdown", "x"))
        assertEquals(FileIcon.Text, FileRules.iconFor(FileKind.FILE, "application/json", "x"))
        assertEquals(FileIcon.Text, FileRules.iconFor(FileKind.FILE, "text/plain; charset=utf-8", "x"))
        assertEquals(FileIcon.Other, FileRules.iconFor(FileKind.FILE, "application/zip", "a.png"))
    }

    @Test fun plainBinaryIsJudgedByItsExtension() {
        assertEquals(FileIcon.Picture, FileRules.iconFor(FileKind.FILE, "application/octet-stream", "Photo.JPG"))
        assertEquals(FileIcon.Pdf, FileRules.iconFor(FileKind.FILE, "", "paper.pdf"))
        assertEquals(FileIcon.Text, FileRules.iconFor(FileKind.FILE, "application/octet-stream", "notes.md"))
        assertEquals(FileIcon.Other, FileRules.iconFor(FileKind.FILE, "application/octet-stream", "two-mb.bin"))
        assertEquals(FileIcon.Other, FileRules.iconFor(FileKind.FILE, "application/octet-stream", "noextension"))
    }

    @Test fun renameSelectsUpToTheLastDot() {
        assertEquals(5, FileRules.renameSelectionEnd("hello.txt", FileKind.FILE))
        assertEquals(5, FileRules.renameSelectionEnd("a.tar.gz", FileKind.FILE))
    }

    @Test fun renameSelectsTheWholeNameWithoutAnExtension() {
        assertEquals(6, FileRules.renameSelectionEnd("README", FileKind.FILE))
        assertEquals(4, FileRules.renameSelectionEnd(".git", FileKind.FILE))
    }

    @Test fun renameOfAFolderSelectsTheWholeName() = assertEquals(9, FileRules.renameSelectionEnd("v1.2 docs", FileKind.FOLDER))

    @Test fun aFreeNameStaysAsItIs() = assertEquals("a.txt", FileRules.freeName("a.txt", listOf("b.txt")))

    @Test fun aTakenNameGetsTheNextNumberBeforeTheExtension() {
        assertEquals("a (2).txt", FileRules.freeName("a.txt", listOf("a.txt")))
        assertEquals("a (3).txt", FileRules.freeName("a.txt", listOf("a.txt", "a (2).txt")))
        assertEquals("a.tar (2).gz", FileRules.freeName("a.tar.gz", listOf("a.tar.gz")))
    }

    @Test fun aTakenNameWithoutExtensionGetsTheNumberAtTheEnd() {
        assertEquals("Makefile (2)", FileRules.freeName("Makefile", listOf("Makefile")))
        assertEquals(".git (2)", FileRules.freeName(".git", listOf(".git")))
    }

    @Test fun namesAreComparedByCase() = assertEquals("A.txt", FileRules.freeName("A.txt", listOf("a.txt")))

    @Test fun theMoveDialogOffersFoldersButNotTheItemItself() {
        val moving = entry(id = "f1", name = "Mine", kind = FileKind.FOLDER)
        val entries = listOf(
            moving,
            entry(id = "f2", name = "Other", kind = FileKind.FOLDER),
            entry(id = "x", name = "file.txt"),
        )
        assertEquals(listOf("f2"), FileRules.moveChoices(entries, moving).map { it.id })
    }

    @Test fun aFileBeingMovedIsNotAmongTheChoicesAnyway() {
        val moving = entry(id = "x", name = "file.txt")
        val entries = listOf(moving, entry(id = "f2", kind = FileKind.FOLDER))
        assertEquals(listOf("f2"), FileRules.moveChoices(entries, moving).map { it.id })
    }

    @Test fun moveHereIsOffAtTheItemsOwnPlace() {
        val moving = entry(id = "x", parentId = "p")
        assertFalse(FileRules.canMoveHere(moving, listOf(entry(id = "p", kind = FileKind.FOLDER)), mayAdd = true))
    }

    @Test fun moveHereIsOffAtTheTopWhenItIsAlreadyThere() {
        assertFalse(FileRules.canMoveHere(entry(id = "x", parentId = null), emptyList(), mayAdd = true))
    }

    @Test fun moveHereIsOnInAnotherFolderThatTakesFiles() {
        val moving = entry(id = "x", parentId = "p")
        assertTrue(FileRules.canMoveHere(moving, listOf(entry(id = "q", kind = FileKind.FOLDER)), mayAdd = true))
        assertTrue(FileRules.canMoveHere(moving, emptyList(), mayAdd = true))
    }

    @Test fun moveHereIsOffInAFolderThatDoesNotTakeFiles() {
        assertFalse(FileRules.canMoveHere(entry(id = "x", parentId = "p"), listOf(entry(id = "q", kind = FileKind.FOLDER)), mayAdd = false))
    }

    @Test fun moveHereIsOffInsideTheFolderBeingMoved() {
        val moving = entry(id = "f1", kind = FileKind.FOLDER, parentId = null)
        val trail = listOf(moving, entry(id = "f9", kind = FileKind.FOLDER, parentId = "f1"))
        assertFalse(FileRules.canMoveHere(moving, trail, mayAdd = true))
    }

    @Test fun aFileOffersEverythingTheRightsAllow() {
        assertEquals(
            listOf(RowAction.Open, RowAction.Save, RowAction.Rename, RowAction.Move, RowAction.Delete),
            FileRules.rowActions(entry()),
        )
    }

    @Test fun aFolderCannotBeSaved() {
        assertEquals(
            listOf(RowAction.Open, RowAction.Rename, RowAction.Move, RowAction.Delete),
            FileRules.rowActions(entry(kind = FileKind.FOLDER)),
        )
    }

    @Test fun withoutRenameRightsThereIsNoRenameAndNoMove() {
        assertEquals(listOf(RowAction.Open, RowAction.Save, RowAction.Delete), FileRules.rowActions(entry(mayRename = false)))
    }

    @Test fun withoutDeleteRightsThereIsNoDelete() {
        assertEquals(
            listOf(RowAction.Open, RowAction.Save, RowAction.Rename, RowAction.Move),
            FileRules.rowActions(entry(mayDelete = false)),
        )
    }

    @Test fun aReadOnlyEntryCanOnlyBeOpenedOrSaved() {
        assertEquals(listOf(RowAction.Open, RowAction.Save), FileRules.rowActions(entry(mayRename = false, mayDelete = false)))
    }

    @Test fun anItemTheAppCannotOpenOffersOnlyDelete() {
        assertEquals(listOf(RowAction.Delete), FileRules.rowActions(entry(kind = FileKind.OTHER)))
        assertEquals(emptyList<RowAction>(), FileRules.rowActions(entry(kind = FileKind.OTHER, mayDelete = false)))
    }

    @Test fun aFileOverTheLimitIsTooLarge() {
        assertTrue(FileRules.tooLarge(50_000_001L, 50_000_000uL))
        assertFalse(FileRules.tooLarge(50_000_000L, 50_000_000uL))
    }

    @Test fun aLimitOfZeroMeansNoLimitIsKnown() = assertFalse(FileRules.tooLarge(1L shl 40, 0uL))

    @Test fun anUnknownSizeIsNotTooLarge() = assertFalse(FileRules.tooLarge(-1L, 10uL))

    @Test fun thePathLineJoinsSpaceAndFolders() {
        val trail = listOf(entry(name = "Docs", kind = FileKind.FOLDER), entry(name = "Sub", kind = FileKind.FOLDER))
        assertEquals("My files / Docs / Sub", FileRules.pathLine("My files", trail))
        assertEquals("My files", FileRules.pathLine("My files", emptyList()))
    }

    @Test fun aNameIsTrimmed() = assertEquals("x y", FileRules.cleanName("  x y \n"))

    @Test fun sizeWordsComeFromTheCaller() = assertEquals("1.5 Ko", FileRules.sizeText(1500uL, en, SizeUnits(byte = "o", kilo = "Ko")))

    @Test fun aBinaryFileIsOpenedAsWhatItsExtensionSays() {
        assertEquals("application/pdf", FileRules.viewType("application/octet-stream", "report.PDF"))
        assertEquals("application/pdf", FileRules.viewType("", "report.pdf"))
        assertEquals("application/octet-stream", FileRules.viewType("application/octet-stream", "thing.unknownext"))
        assertEquals("text/plain", FileRules.viewType("text/plain", "report.pdf"))
    }

    @Test fun theServerNameKeepsWhatTheServerTakes() {
        assertEquals(".htaccess", FileRules.serverName(".htaccess"))
        assertEquals("env.", FileRules.serverName("env."))
        assertEquals(" a ", FileRules.serverName(" a "))
        val long = "x".repeat(200) + ".txt"
        assertEquals(long, FileRules.serverName(long))
    }

    @Test fun theServerNameChangesWhatTheServerRefuses() {
        assertEquals("a_b_c.txt", FileRules.serverName("a/b:c.txt"))
        assertEquals("attachment", FileRules.serverName(".."))
        assertEquals("attachment", FileRules.serverName(null))
        assertEquals("_NUL.txt", FileRules.serverName("NUL.txt"))
        assertEquals("CONSOLE.txt", FileRules.serverName("CONSOLE.txt"))
        val cut = FileRules.serverName("é".repeat(200) + ".txt")
        assertTrue(cut.toByteArray().size <= 255 && cut.endsWith(".txt"))
    }
}
