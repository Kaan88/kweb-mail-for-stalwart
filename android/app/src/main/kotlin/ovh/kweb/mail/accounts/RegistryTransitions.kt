package ovh.kweb.mail.accounts

/** Every change of the registry as a pure function, so the order of the steps can be tested. */
object RegistryTransitions {
    /** An account is about to be added: its id is in the discard set before any file exists. */
    fun beginAdd(state: RegistryState, id: String): RegistryState =
        state.copy(discard = state.discard + DiscardEntry(id, false).encode())

    /** The sign-in worked: the id leaves the discard set, the account joins and becomes the active one. */
    fun promote(state: RegistryState, record: AccountRecord): RegistryState {
        require(DiscardEntry(record.id, false).encode() in state.discard) { "the account was not started with beginAdd" }
        return state.copy(
            accounts = state.accounts.filter { it.id != record.id } + record,
            active = record.id,
            discard = state.discard - DiscardEntry(record.id, false).encode(),
        )
    }

    /** The account leaves the list and its storage is queued for deletion; the next one (if any) becomes active. */
    fun remove(state: RegistryState, id: String): RegistryState {
        val record = state.accounts.firstOrNull { it.id == id } ?: return state
        val rest = state.accounts.filter { it.id != id }
        return state.copy(
            accounts = rest,
            active = if (state.active == id) rest.firstOrNull()?.id else state.active,
            discard = state.discard + DiscardEntry(record.id, record.legacy).encode(),
        )
    }

    /** The storage of [entry] is gone. */
    fun swept(state: RegistryState, entry: DiscardEntry): RegistryState =
        state.copy(discard = state.discard - entry.encode())

    fun setActive(state: RegistryState, id: String): RegistryState =
        if (state.accounts.any { it.id == id }) state.copy(active = id) else state

    fun replace(state: RegistryState, record: AccountRecord): RegistryState =
        state.copy(accounts = state.accounts.map { if (it.id == record.id) record else it })
}
