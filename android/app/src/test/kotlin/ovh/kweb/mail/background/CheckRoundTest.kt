package ovh.kweb.mail.background

import kotlinx.coroutines.CancellationException
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Assert.fail
import org.junit.Test

class CheckRoundTest {
    @Test
    fun everyAccountIsCheckedInOrder() {
        val seen = mutableListOf<String>()
        val failures = CheckRound.runAll(listOf("a", "b", "c")) { seen += it }
        assertEquals(listOf("a", "b", "c"), seen)
        assertTrue(failures.isEmpty())
    }

    @Test
    fun oneFailingAccountDoesNotSkipTheNext() {
        val seen = mutableListOf<String>()
        val failures = CheckRound.runAll(listOf("a", "b", "c")) {
            seen += it
            if (it == "a") error("down")
        }
        assertEquals(listOf("a", "b", "c"), seen)
        assertEquals(listOf("a"), failures.map { it.item })
        assertEquals("down", failures.single().error.message)
    }

    @Test
    fun aCancellationStopsTheRound() {
        val seen = mutableListOf<String>()
        try {
            CheckRound.runAll(listOf("a", "b")) {
                seen += it
                throw CancellationException("stopped")
            }
            fail()
        } catch (_: CancellationException) {
        }
        assertEquals(listOf("a"), seen)
    }
}
