package ovh.kweb.mail.accounts

/**
 * Where one account keeps its things, as names relative to `filesDir`, the prefs directory or `cacheDir`.
 * Pure, so a test pins the names the migrated (legacy) account must keep: `cache.db`, `secrets`, `settings`.
 */
object AccountPaths {
    private const val LEGACY_CACHE = "cache.db"
    private const val LEGACY_SECRETS = "secrets"
    private const val LEGACY_SETTINGS = "settings"

    /** The directory under `filesDir` that holds an account's own files; legacy keeps them directly in `filesDir`. */
    fun directory(id: String, legacy: Boolean): String? = if (legacy) null else "accounts/$id"

    fun cache(id: String, legacy: Boolean): String = if (legacy) LEGACY_CACHE else "accounts/$id/cache.db"

    /** SQLite's side files of [cache]. */
    fun cacheSideFiles(id: String, legacy: Boolean): List<String> =
        listOf("-wal", "-shm", "-journal").map { cache(id, legacy) + it }

    fun secretsPrefs(id: String, legacy: Boolean): String = if (legacy) LEGACY_SECRETS else "secrets.$id"

    fun settingsPrefs(id: String, legacy: Boolean): String = if (legacy) LEGACY_SETTINGS else "settings.$id"

    fun blobs(id: String): String = "blobs/$id"

    fun attachments(id: String): String = "attachments/$id"

    /**
     * The associated data a secret's ciphertext is bound to. The legacy account keeps today's (the bare key
     * name), so what is stored decrypts; every other account binds the id too, so a ciphertext cannot be
     * moved between accounts.
     */
    fun secretAad(id: String, legacy: Boolean, key: String): ByteArray =
        (if (legacy) key else "$id\u0000$key").toByteArray()
}
