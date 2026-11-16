package ovh.kweb.mail.ui.transfer

import java.io.File
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.rules.TemporaryFolder

class SweepPartsTest {
    @get:Rule val folder = TemporaryFolder()

    @Test fun partFilesOfADeadProcessGoAndTheRestStays() {
        val cache = folder.root
        val dir = File(cache, "attachments/acc/hash").apply { mkdirs() }
        val dead = File(dir, "report.pdf.111-0.part").apply { writeText("x") }
        val mine = File(dir, "other.pdf.222-1.part").apply { writeText("x") }
        val whole = File(dir, "report.pdf").apply { writeText("x") }
        val blob = File(cache, "blobs/acc/abc.333-0.part").apply { parentFile.mkdirs(); writeText("x") }
        TransferHub.sweepParts(cache, ownPid = 222)
        assertFalse(dead.exists())
        assertFalse(blob.exists())
        assertTrue("a download of this process is left alone", mine.exists())
        assertTrue("a finished file is never touched", whole.exists())
    }
}
