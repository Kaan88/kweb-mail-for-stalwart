package ovh.kweb.mail.background

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/** A map standing in for SharedPreferences. */
private class MapValues : KeyValues {
    val map = mutableMapOf<String, Any>()
    override fun getBoolean(key: String, default: Boolean) = map[key] as Boolean? ?: default
    override fun putBoolean(key: String, value: Boolean) { map[key] = value }
    override fun getLong(key: String, default: Long) = map[key] as Long? ?: default
    override fun getString(key: String) = map[key] as String?
    override fun putLongAndString(longKey: String, long: Long, stringKey: String, string: String?) {
        map[longKey] = long
        if (string == null) map.remove(stringKey) else map[stringKey] = string
    }
    override fun remove(vararg keys: String) { keys.forEach { map.remove(it) } }
}

class SettingsTest {
    private val settings = Settings(MapValues())

    @Test
    fun allSwitchesAreOnByDefault() {
        assertTrue(settings.mailEnabled)
        assertTrue(settings.remindersEnabled)
        assertTrue(settings.removeWhenReadElsewhere)
    }

    @Test
    fun switchesKeepWhatWasSetAndTickTheChangeCounter() {
        val before = settings.changes.value
        settings.mailEnabled = false
        assertFalse(settings.mailEnabled)
        assertTrue(settings.changes.value > before)
    }

    @Test
    fun nothingIsRecordedAtFirst() {
        assertNull(settings.lastCheck)
        assertNull(ServiceLog(MapValues()).lastStartFailure)
    }

    @Test
    fun aCheckRecordsItsTimeAndResult() {
        settings.recordCheck(100L, null)
        assertEquals(CheckRecord(100L, null), settings.lastCheck)
        settings.recordCheck(200L, "offline")
        assertEquals(CheckRecord(200L, "offline"), settings.lastCheck)
    }

    @Test
    fun removingAnAccountClearsItsBookkeepingButNotItsSwitches() {
        settings.mailEnabled = false
        settings.recordCheck(1L, null)
        settings.clearBookkeeping()
        assertNull(settings.lastCheck)
        assertFalse(settings.mailEnabled)
    }

    @Test
    fun twoAccountsKeepTheirOwnSwitchesAndChecks() {
        val other = Settings(MapValues())
        settings.mailEnabled = false
        settings.recordCheck(7L, "offline")
        assertTrue(other.mailEnabled)
        assertNull(other.lastCheck)
    }

    @Test
    fun theSwitchesOfAnOlderInstallCanBeTakenOver() {
        val old = Settings(MapValues())
        old.mailEnabled = false
        old.remindersEnabled = false
        val fresh = Settings(MapValues())
        fresh.takeSwitchesFrom(old)
        assertFalse(fresh.mailEnabled)
        assertFalse(fresh.remindersEnabled)
        assertTrue(fresh.removeWhenReadElsewhere)
    }

    @Test
    fun theServiceLogRecordsClearsAndTicks() {
        val log = ServiceLog(MapValues())
        val before = log.changes.value
        log.recordStartFailure(5L, "not allowed")
        assertEquals(StartFailure(5L, "not allowed"), log.lastStartFailure)
        assertTrue(log.changes.value > before)
        log.clearStartFailure()
        assertNull(log.lastStartFailure)
    }
}
