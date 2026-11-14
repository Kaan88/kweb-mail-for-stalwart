package ovh.kweb.mail.background

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class LiveGateTest {
    private class Loop(val name: String, val log: MutableList<String>) : LiveLoop {
        var running = false
        var onStop: () -> Unit = {}
        override fun start() { running = true; log += "start $name" }
        override fun stop() { running = false; log += "stop $name"; onStop() }
    }

    private val log = mutableListOf<String>()
    private val a = Loop("a", log)
    private val b = Loop("b", log)
    private var now = 0L
    private val tasks = mutableListOf<Triple<Long, () -> Unit, BooleanArray>>()
    private var owedAsked = 0

    private val gate = LiveGate(
        loops = { listOf(a, b) },
        later = { ms, task ->
            val cancelled = BooleanArray(1)
            tasks += Triple(now + ms, task, cancelled)
            return@LiveGate { cancelled[0] = true; Unit }
        },
        owed = { owedAsked++ },
        stopDelayMs = 30_000,
    )

    private fun advanceTo(t: Long) {
        now = t
        while (true) {
            val due = tasks.filter { it.first <= now && !it.third[0] }.minByOrNull { it.first } ?: break
            tasks.remove(due)
            due.second()
        }
    }

    @Test
    fun theLoopsRunWhileOnScreenAndStopThirtySecondsAfterLeaving() {
        gate.screenOn()
        gate.startLoops()
        assertTrue(a.running && b.running)
        gate.screenOff()
        advanceTo(29_999)
        assertTrue(a.running)
        advanceTo(30_000)
        assertFalse(a.running || b.running)
    }

    @Test
    fun leavingBeforeTheSlowStartRunsLeavesNoLoopBehind() {
        // onStart and onStop were reported in order; the slow start of the loops comes after the stop.
        gate.screenOn()
        gate.screenOff()
        gate.startLoops()
        assertFalse("the flag says the app is away: nothing starts", a.running || b.running)
        assertFalse(gate.onScreen)
        advanceTo(30_000)
        assertFalse(a.running || b.running)
        assertEquals(emptyList<String>(), log.filter { it.startsWith("start") })
    }

    @Test
    fun aLoopStartedJustBeforeLeavingIsStoppedByTheStopThatFollows() {
        gate.screenOn()
        a.start()
        gate.screenOff()
        gate.startLoops() // late: a is already running; b must not start
        advanceTo(30_000)
        assertFalse(a.running || b.running)
    }

    @Test
    fun comingBackWhileALoopIsBeingStoppedStartsItAgain() {
        gate.screenOn()
        gate.startLoops()
        gate.screenOff()
        a.onStop = { gate.screenOn() } // the user is back at the moment a is stopped
        advanceTo(30_000)
        assertTrue("a runs again", a.running)
        assertFalse("b was never stopped", log.contains("stop b"))
    }

    @Test
    fun comingBackBeforeTheStopCancelsIt() {
        gate.screenOn()
        gate.startLoops()
        gate.screenOff()
        advanceTo(10_000)
        gate.screenOn()
        advanceTo(60_000)
        assertTrue(a.running && b.running)
        assertFalse(log.any { it.startsWith("stop") })
    }

    @Test
    fun owedWorkIsAskedForWhenLeavingAndAfterTheStop() {
        gate.screenOn()
        gate.screenOff()
        advanceTo(0)
        assertEquals(1, owedAsked)
        advanceTo(30_000)
        assertEquals(2, owedAsked)
    }
}
