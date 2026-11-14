package ovh.kweb.mail.ui.thread

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class FileNamesTest {
    @Test fun anOrdinaryNameStays() = assertEquals("Invoice 2026-11.pdf", FileNames.safe("Invoice 2026-11.pdf"))

    @Test fun separatorsBecomeUnderscores() {
        assertEquals("a_b_c.txt", FileNames.safe("a/b\\c.txt"))
        assertFalse(FileNames.safe("../../etc/passwd").contains('/'))
    }

    @Test fun dotsAtTheEndsGo() {
        assertEquals("etc_passwd", FileNames.safe("../etc/passwd").removePrefix("_").removePrefix("_"))
        assertEquals("htaccess", FileNames.safe(".htaccess"))
        assertEquals("report", FileNames.safe("report.."))
    }

    @Test fun controlCharactersGo() = assertEquals("ab.txt", FileNames.safe("a\u0000b\n.txt"))

    @Test fun neverEmpty() {
        assertEquals("attachment", FileNames.safe(""))
        assertEquals("attachment", FileNames.safe("  "))
        assertEquals("attachment", FileNames.safe(".."))
        assertEquals("attachment", FileNames.safe("\u0001"))
        assertEquals("file", FileNames.safe("", fallback = "file"))
    }

    private fun bytes(text: String) = text.toByteArray(Charsets.UTF_8).size

    private fun isWellFormed(text: String) = String(text.toByteArray(Charsets.UTF_8), Charsets.UTF_8) == text

    @Test fun aLongNameKeepsItsExtension() {
        val safe = FileNames.safe("x".repeat(300) + ".pdf")
        assertEquals(180, safe.length)
        assertTrue(safe.endsWith(".pdf"))
    }

    @Test fun aLongCjkNameFitsInBytes() {
        val safe = FileNames.safe("日".repeat(100) + ".pdf")
        assertTrue(bytes(safe) <= 180)
        assertTrue(safe.endsWith(".pdf"))
        assertTrue(safe.removeSuffix(".pdf").all { it == '日' })
    }

    @Test fun anEmojiAtTheCutIsNotSplit() {
        // 175 ASCII + two 4-byte emoji + ".pdf" is 187 bytes: the cut falls inside the emoji.
        val safe = FileNames.safe("a".repeat(175) + "😀😀" + ".pdf")
        assertTrue(bytes(safe) <= 180)
        assertTrue(isWellFormed(safe))
        assertTrue(safe.endsWith(".pdf"))
        assertEquals("a".repeat(175) + ".pdf", safe)
    }

    @Test fun aNameThatFitsIsNotTouched() {
        val name = "日".repeat(55) + ".pdf"
        assertEquals(name, FileNames.safe(name))
    }

    @Test fun nonAsciiLettersStay() = assertEquals("Özet şubat.docx", FileNames.safe("Özet şubat.docx"))

    @Test fun sha256IsHex() = assertEquals("2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824", FileNames.sha256Hex("hello"))

    @Test
    fun theKindOfAFileIsItsShortExtension() {
        assertEquals("PDF", FileNames.kind("Offer-2026.pdf"))
        assertEquals("GZ", FileNames.kind("backup.tar.gz"))
        assertEquals(null, FileNames.kind("README"))
        assertEquals(null, FileNames.kind(".hidden"))
        assertEquals(null, FileNames.kind("notes.backup-of-monday"))
        assertEquals(null, FileNames.kind(""))
    }
}
