package ovh.kweb.mail.accounts

/**
 * [RegistryState] as the flat string map the prefs file holds. Unknown keys are ignored and a damaged
 * account entry is skipped, so a newer or older version of the app can read what the other wrote.
 */
object RegistryCodec {
    private const val VERSION = "version"
    private const val ACTIVE = "active"
    private const val DISCARD = "discard"
    private const val COUNT = "count"

    fun encode(state: RegistryState): Map<String, String> {
        val out = linkedMapOf(VERSION to state.version.toString(), COUNT to state.accounts.size.toString())
        state.active?.let { out[ACTIVE] = it }
        if (state.discard.isNotEmpty()) out[DISCARD] = state.discard.sorted().joinToString(",")
        state.accounts.forEachIndexed { i, a ->
            out["a.$i.id"] = a.id
            out["a.$i.legacy"] = a.legacy.toString()
            out["a.$i.username"] = a.username
            out["a.$i.serverUrl"] = a.serverUrl
            out["a.$i.jmapAccountId"] = a.jmapAccountId
            out["a.$i.addedAt"] = a.addedAt.toString()
        }
        return out
    }

    /** The state in [values], or null when there is none (no `version` key: the file was never written). */
    fun decode(values: Map<String, String>?): RegistryState? {
        if (values == null) return null
        val version = values[VERSION]?.toIntOrNull() ?: return null
        val count = values[COUNT]?.toIntOrNull() ?: 0
        val accounts = (0 until count).mapNotNull { i ->
            val id = values["a.$i.id"]?.takeIf { it.isNotEmpty() } ?: return@mapNotNull null
            AccountRecord(
                id = id,
                legacy = values["a.$i.legacy"] == "true",
                username = values["a.$i.username"].orEmpty(),
                serverUrl = values["a.$i.serverUrl"].orEmpty(),
                jmapAccountId = values["a.$i.jmapAccountId"].orEmpty(),
                addedAt = values["a.$i.addedAt"]?.toLongOrNull() ?: 0L,
            )
        }
        val discard = values[DISCARD]?.split(',')?.filter { it.isNotEmpty() }?.toSet().orEmpty()
        val active = values[ACTIVE]?.takeIf { id -> accounts.any { it.id == id } }
        return RegistryState(version, accounts, active, discard)
    }
}
