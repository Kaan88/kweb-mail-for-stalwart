package ovh.kweb.mail.ui.files

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class UploadQueueTest {
    @Test fun theQueueStartsAtTheFirstFile() {
        val q = UploadQueue(listOf("a", "b", "c"))
        assertEquals("a", q.current)
        assertEquals(1, q.position)
        assertEquals(3, q.total)
    }

    @Test fun advancingGoesThroughAllFilesInOrder() {
        var q = UploadQueue(listOf("a", "b", "c"))
        val seen = mutableListOf<String>()
        while (true) {
            seen += q.current ?: break
            q = q.advance()
        }
        assertEquals(listOf("a", "b", "c"), seen)
        assertNull(q.current)
    }

    @Test fun thePositionCountsFromOneAndStopsAtTheTotal() {
        var q = UploadQueue(listOf("a", "b"))
        assertEquals(1, q.position)
        q = q.advance()
        assertEquals(2, q.position)
        q = q.advance()
        assertEquals(2, q.position)
    }

    @Test fun stoppingSendsNoFurtherFile() {
        val q = UploadQueue(listOf("a", "b", "c")).advance().stop()
        assertNull(q.current)
    }

    @Test fun anEmptyQueueIsFinishedAtOnce() {
        val q = UploadQueue(emptyList<String>())
        assertNull(q.current)
        assertEquals(0, q.position)
    }

    @Test fun theTallyCountsEachOutcome() {
        val t = UploadTally()
            .record(UploadOutcome.Uploaded)
            .record(UploadOutcome.Uploaded)
            .record(UploadOutcome.Skipped)
            .record(UploadOutcome.Failed)
        assertEquals(UploadTally(uploaded = 2, skipped = 1, failed = 1), t)
        assertEquals(4, t.handled)
    }

    @Test fun filesNeverTriedAreCounted() {
        val t = UploadTally(uploaded = 1)
        assertEquals(4, t.notSent(5))
        assertTrue(t.notSent(0) == 0)
    }
}
