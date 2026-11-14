package ovh.kweb.mail.accounts

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Assert.fail
import org.junit.Test

class RegistryTransitionsTest {
    private fun loaded(vararg existing: AccountRecord, active: String? = existing.firstOrNull()?.id, discard: Set<String> = emptySet()): Rig {
        val rig = Rig(storage = FakeStorage(RegistryCodec.encode(RegistryState(accounts = existing.toList(), active = active, discard = discard))))
        return rig
    }

    @Test
    fun beginAddCommitsTheIdAsDiscardableBeforeAnyFileExists() {
        val rig = loaded()
        val store = rig.store
        store.load()
        val id = store.beginAdd()
        assertEquals(setOf(id), RegistryCodec.decode(rig.storage.values)!!.discard)
        assertTrue(rig.files.deleted.isEmpty())
    }

    @Test
    fun promoteMovesTheIdFromDiscardToAccountsAndMakesItActive() {
        val rig = loaded(record("old"))
        val store = rig.store
        store.load()
        val id = store.beginAdd()
        store.promote(record(id))
        val state = RegistryCodec.decode(rig.storage.values)!!
        assertEquals(listOf("old", id), state.accounts.map { it.id })
        assertEquals(id, state.active)
        assertTrue(state.discard.isEmpty())
    }

    @Test
    fun promoteWithoutBeginAddIsRefused() {
        val store = loaded().store
        store.load()
        try {
            store.promote(record("zzz"))
            fail()
        } catch (_: IllegalArgumentException) {
        }
    }

    @Test
    fun abandonDeletesTheStorageThenForgetsTheId() {
        val rig = loaded(record("old"))
        val store = rig.store
        store.load()
        val id = store.beginAdd()
        store.abandon(id)
        assertEquals(listOf(DiscardEntry(id, false)), rig.files.deleted)
        val state = RegistryCodec.decode(rig.storage.values)!!
        assertTrue(state.discard.isEmpty())
        assertEquals(listOf("old"), state.accounts.map { it.id })
        // Twice is harmless.
        store.abandon(id)
        assertEquals(1, rig.files.deleted.size)
    }

    @Test
    fun removeQueuesTheStorageRunsTheCleanupThenDeletesAndPicksTheNextAccount() {
        val rig = loaded(record("a"), record("b"), active = "a")
        val store = rig.store
        store.load()
        var seenDuringCleanup: RegistryState? = null
        val removed = store.remove("a") { seenDuringCleanup = RegistryCodec.decode(rig.storage.values) }
        assertEquals("a", removed!!.id)
        assertEquals(setOf("a"), seenDuringCleanup!!.discard)
        assertEquals("b", seenDuringCleanup!!.active)
        assertEquals(listOf(DiscardEntry("a", false)), rig.files.deleted)
        val state = RegistryCodec.decode(rig.storage.values)!!
        assertEquals(listOf("b"), state.accounts.map { it.id })
        assertEquals("b", state.active)
        assertTrue(state.discard.isEmpty())
    }

    @Test
    fun removingAnotherAccountKeepsTheActiveOne() {
        val rig = loaded(record("a"), record("b"), active = "a")
        val store = rig.store
        store.load()
        store.remove("b")
        assertEquals("a", store.state.active)
    }

    @Test
    fun removingTheLastAccountLeavesNoActiveOne() {
        val rig = loaded(record("a", legacy = true))
        val store = rig.store
        store.load()
        store.remove("a")
        assertTrue(store.state.accounts.isEmpty())
        assertNull(store.state.active)
        assertEquals(listOf(DiscardEntry("a", true)), rig.files.deleted)
    }

    @Test
    fun removingAnUnknownAccountDoesNothing() {
        val rig = loaded(record("a"))
        val store = rig.store
        store.load()
        assertNull(store.remove("nope"))
        assertEquals(1, store.state.accounts.size)
    }

    @Test
    fun setActiveOnlyAcceptsAKnownAccountAndReplaceKeepsTheOrder() {
        val rig = loaded(record("a"), record("b"), active = "a")
        val store = rig.store
        store.load()
        store.setActive("ghost")
        assertEquals("a", store.state.active)
        store.setActive("b")
        assertEquals("b", store.state.active)
        store.replace(record("a", user = "renamed"))
        assertEquals(listOf("renamed", "u-b"), store.state.accounts.map { it.username })
    }

    // The sweep from every intermediate state a process death can leave.

    @Test
    fun theSweepFinishesAnAbandonedAdd() {
        val rig = loaded(record("a"), discard = setOf("half"))
        val state = rig.store.load()
        assertEquals(listOf(DiscardEntry("half", false)), rig.files.deleted)
        assertTrue(state.discard.isEmpty())
        assertEquals(listOf("a"), state.accounts.map { it.id })
    }

    @Test
    fun theSweepFinishesARemovalCutShortAfterTheFirstCommit() {
        val rig = loaded(record("b"), active = "b", discard = setOf("a:legacy"))
        val state = rig.store.load()
        assertEquals(listOf(DiscardEntry("a", true)), rig.files.deleted)
        assertTrue(state.discard.isEmpty())
        assertEquals("b", state.active)
    }

    @Test
    fun theSweepHandlesSeveralEntriesAndLeavesTheAccountsAlone() {
        val rig = loaded(record("keep"), discard = setOf("x", "y:legacy"))
        val state = rig.store.load()
        assertEquals(setOf(DiscardEntry("x", false), DiscardEntry("y", true)), rig.files.deleted.toSet())
        assertEquals(listOf("keep"), state.accounts.map { it.id })
        assertTrue(RegistryCodec.decode(rig.storage.values)!!.discard.isEmpty())
    }

    @Test
    fun theSweepRunsAgainIfItWasCutShortItself() {
        val rig = loaded(discard = setOf("x", "y"))
        // The first process died after deleting x but before committing: the entry is still there.
        rig.store.load()
        val second = loaded(discard = setOf("x"))
        second.store.load()
        assertEquals(listOf(DiscardEntry("x", false)), second.files.deleted)
    }

    @Test
    fun aStateThatDiesBetweenMigrationAndSweepIsRepeatedWithoutHarm() {
        val rig = Rig(files = FakeFiles(legacyCache = true), legacy = { null })
        // Died right after the migration's commit: the registry already holds the discard entry.
        val first = rig.store
        first.load()
        val again = Rig(storage = FakeStorage(rig.storage.values), files = FakeFiles(legacyCache = true), legacy = { error("must not open") })
        assertTrue(again.store.load().accounts.isEmpty())
    }
}
