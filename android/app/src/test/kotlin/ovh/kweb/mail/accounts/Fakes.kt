package ovh.kweb.mail.accounts

/** A map standing in for the prefs file; `failWrites` makes every write fail like a full disk. */
class FakeStorage(var values: Map<String, String>? = null) : RegistryStorage {
    var failWrites = false
    var writes = 0

    override fun read(): Map<String, String>? = values

    override fun write(values: Map<String, String>): Boolean {
        if (failWrites) return false
        this.values = values
        writes++
        return true
    }
}

/** Files as a set of names, with the calls recorded. */
class FakeFiles(var legacyCache: Boolean = false) : AccountFiles {
    val deleted = mutableListOf<DiscardEntry>()
    var blobsCleared = 0

    override fun legacyCacheExists() = legacyCache

    override fun clearBlobCaches() {
        blobsCleared++
    }

    override fun deleteStorage(entry: DiscardEntry) {
        deleted += entry
    }
}

fun record(id: String, user: String = "u-$id", server: String = "https://mail.example.org", jmap: String = "j-$id", legacy: Boolean = false) =
    AccountRecord(id, legacy, user, server, jmap, 1L)

/** A store over fakes, with ids a1, a2, ... */
class Rig(
    val storage: FakeStorage = FakeStorage(),
    val files: FakeFiles = FakeFiles(),
    var legacy: () -> LegacyAccount? = { null },
) {
    private var next = 0
    val store get() = AccountStore(storage, files, { legacy() }, { "a${++next}" }, { 100L })
}
