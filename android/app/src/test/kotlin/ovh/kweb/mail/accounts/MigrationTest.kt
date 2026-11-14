package ovh.kweb.mail.accounts

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Assert.fail
import org.junit.Test

class MigrationTest {
    private val legacyAccount = LegacyAccount("kaan", "https://mail.kweb.ovh", "d333333")

    @Test
    fun aSignedInLegacyInstallGivesOneActiveLegacyRecordAndClearsOnlyTheBlobCaches() {
        val rig = Rig(files = FakeFiles(legacyCache = true), legacy = { legacyAccount })
        val state = rig.store.load()
        assertEquals(1, state.accounts.size)
        val record = state.accounts.single()
        assertTrue(record.legacy)
        assertEquals("kaan", record.username)
        assertEquals("https://mail.kweb.ovh", record.serverUrl)
        assertEquals("d333333", record.jmapAccountId)
        assertEquals(record.id, state.active)
        assertEquals(1, rig.files.blobsCleared)
        assertTrue(rig.files.deleted.isEmpty())
        assertEquals(state, RegistryCodec.decode(rig.storage.values))
    }

    @Test
    fun aSignedOutLegacyInstallGivesAnEmptyRegistryAndQueuesTheOldFilesForDeletion() {
        val rig = Rig(files = FakeFiles(legacyCache = true), legacy = { null })
        val state = rig.store.load()
        assertTrue(state.accounts.isEmpty())
        assertNull(state.active)
        // The sweep that follows deletes them at once, and nothing is left to discard.
        assertEquals(1, rig.files.deleted.size)
        assertTrue(rig.files.deleted.single().legacy)
        assertTrue(state.discard.isEmpty())
    }

    @Test
    fun aFreshInstallGivesAnEmptyRegistryAndTouchesNoFiles() {
        val rig = Rig(files = FakeFiles(legacyCache = false))
        val state = rig.store.load()
        assertEquals(RegistryState(), state)
        assertTrue(rig.files.deleted.isEmpty())
        assertEquals(0, rig.files.blobsCleared)
        assertEquals(RegistryState(), RegistryCodec.decode(rig.storage.values))
    }

    @Test
    fun aSecondStartChangesNothing() {
        val rig = Rig(files = FakeFiles(legacyCache = true), legacy = { legacyAccount })
        val first = rig.store.load()
        val writes = rig.storage.writes
        var opened = false
        rig.legacy = { opened = true; legacyAccount }
        val second = rig.store.load()
        assertEquals(first, second)
        assertEquals(writes, rig.storage.writes)
        assertTrue("the legacy cache is not opened again", !opened)
        assertEquals(1, rig.files.blobsCleared)
    }

    @Test
    fun aFailureBeforeTheCommitLeavesNoRegistryAndTheNextStartRepeatsIt() {
        val rig = Rig(files = FakeFiles(legacyCache = true), legacy = { error("cannot open the cache") })
        try {
            rig.store.load()
            fail("the error must reach the caller")
        } catch (e: IllegalStateException) {
            assertEquals("cannot open the cache", e.message)
        }
        assertNull(rig.storage.values)
        rig.legacy = { legacyAccount }
        assertEquals(1, rig.store.load().accounts.size)
    }

    @Test
    fun aFailedWriteIsAnErrorAndNoStateIsKept() {
        val rig = Rig(storage = FakeStorage().also { it.failWrites = true }, files = FakeFiles(legacyCache = true), legacy = { legacyAccount })
        try {
            rig.store.load()
            fail()
        } catch (_: IllegalStateException) {
        }
        assertNull(rig.storage.values)
    }
}
