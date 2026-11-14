package ovh.kweb.mail.accounts

/** Where the registry is kept: one flat map, replaced whole. Null from [read] means nothing was ever written. */
interface RegistryStorage {
    fun read(): Map<String, String>?

    /** Replaces everything in one atomic step; false when it could not be written. */
    fun write(values: Map<String, String>): Boolean
}

/** What the store does to files. Everything here may block. */
interface AccountFiles {
    /** Whether the cache file the app had before several accounts (`cache.db`) exists. */
    fun legacyCacheExists(): Boolean

    /** Deletes every downloaded blob and attachment of every account; they come back on demand. */
    fun clearBlobCaches()

    /** Deletes everything an account has on the device: cache, prefs, secrets, blobs. Missing files are fine. */
    fun deleteStorage(entry: DiscardEntry)
}

/** What the old, single account looked like (read from its cache). */
data class LegacyAccount(val username: String, val serverUrl: String, val jmapAccountId: String)

/**
 * The list of accounts and its changes, with no Android in it. Every change is one commit of the whole
 * registry, and the order of the steps is what makes a process death anywhere harmless: an id is in the
 * discard set before any file of it exists, and leaves it only after the files are gone.
 *
 * [readLegacy] opens the cache the single-account app had and returns its account (null when signed out);
 * it throws when it cannot be read, and then nothing is written.
 */
class AccountStore(
    private val storage: RegistryStorage,
    private val files: AccountFiles,
    private val readLegacy: () -> LegacyAccount?,
    private val newId: () -> String,
    private val now: () -> Long,
) {
    private var current: RegistryState? = null

    val state: RegistryState get() = current ?: error("AccountStore.load() has not run")

    /** Migrates a single-account install when needed, finishes interrupted removals, and returns the state. Idempotent. */
    @Synchronized
    fun load(): RegistryState {
        current?.let { return it }
        val stored = RegistryCodec.decode(storage.read())
        var s = stored ?: migrate()
        current = s
        for (entry in s.discard.map(DiscardEntry::decode)) {
            files.deleteStorage(entry)
            s = RegistryTransitions.swept(s, entry)
            commit(s)
        }
        return s
    }

    /**
     * Runs when the registry was never written. Nothing is moved or rewritten: the legacy account keeps its
     * cache, secrets and settings under their old names. If the app dies before the commit the next start
     * repeats this from the same untouched files.
     */
    private fun migrate(): RegistryState {
        val empty = RegistryState()
        if (!files.legacyCacheExists()) return empty.also { writeOrThrow(it) }
        val legacy = readLegacy()
        if (legacy == null) {
            // Signed out, but the old files are still there: queue them for deletion.
            val entry = DiscardEntry(newId(), true)
            return empty.copy(discard = setOf(entry.encode())).also { writeOrThrow(it) }
        }
        files.clearBlobCaches()
        val record = AccountRecord(newId(), true, legacy.username, legacy.serverUrl, legacy.jmapAccountId, now())
        return empty.copy(accounts = listOf(record), active = record.id).also { writeOrThrow(it) }
    }

    /** Makes the id of an account about to be added and commits it as discardable before any file exists. */
    @Synchronized
    fun beginAdd(): String {
        val id = newId()
        commit(RegistryTransitions.beginAdd(state, id))
        return id
    }

    /** The sign-in worked: one commit adds [record] and makes it the active account. */
    @Synchronized
    fun promote(record: AccountRecord) = commit(RegistryTransitions.promote(state, record))

    /** An add did not finish: deletes whatever it made, then forgets the id. */
    @Synchronized
    fun abandon(id: String) {
        val entry = DiscardEntry(id, false)
        if (entry.encode() !in state.discard) return
        files.deleteStorage(entry)
        commit(RegistryTransitions.swept(state, entry))
    }

    /**
     * Removes an account: one commit moves it to the discard set (and picks the next active account), then
     * [beforeDelete] runs (notifications), then its files go, then the discard entry. Returns the removed
     * record, or null when there was none with that id.
     */
    @Synchronized
    fun remove(id: String, beforeDelete: (AccountRecord) -> Unit = {}): AccountRecord? {
        val record = state.accounts.firstOrNull { it.id == id } ?: return null
        commit(RegistryTransitions.remove(state, id))
        try {
            beforeDelete(record)
        } finally {
            val entry = DiscardEntry(record.id, record.legacy)
            files.deleteStorage(entry)
            commit(RegistryTransitions.swept(state, entry))
        }
        return record
    }

    @Synchronized
    fun setActive(id: String) {
        val next = RegistryTransitions.setActive(state, id)
        if (next != state) commit(next)
    }

    @Synchronized
    fun replace(record: AccountRecord) {
        val next = RegistryTransitions.replace(state, record)
        if (next != state) commit(next)
    }

    private fun commit(next: RegistryState) {
        writeOrThrow(next)
        current = next
    }

    private fun writeOrThrow(next: RegistryState) {
        check(storage.write(RegistryCodec.encode(next))) { "the list of accounts could not be saved" }
    }
}
