package ovh.kweb.mail.accounts

import android.content.Context
import android.util.Log
import java.io.File
import java.security.SecureRandom
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharingStarted
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.flow.distinctUntilChanged
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.flow.stateIn
import kotlinx.coroutines.flow.update
import ovh.kweb.mail.background.Background
import ovh.kweb.mail.background.KeystoreSecrets
import ovh.kweb.mail.background.Notifier
import ovh.kweb.mail.background.Settings
import uniffi.kweb_core.ConnectionStatus
import uniffi.kweb_core.Core

/** The sign-in went through, but the account is one the app already has. */
class AccountAlreadyAdded : Exception("this account is already in the app")

/**
 * The accounts of the app, and one [AccountSession] (one `Core`, with its own cache file and secrets) for each.
 *
 * Call [load] before anything else; it blocks (it may open the cache once and migrate an install made before
 * several accounts existed), so never on the main thread. It is idempotent and cheap after the first time.
 * [records] and [active] are only meaningful after it. A session is made on first use, once, and kept for the
 * life of the process.
 */
object Accounts {
    private lateinit var app: Context
    private val loadLock = Any()
    private val sessionLock = Any()
    private var store: AccountStore? = null
    private val sessions = LinkedHashMap<String, AccountSession>()

    /** The Core opened to read the old account during the migration; the legacy session takes it over. */
    private var preopened: Core? = null

    private var candidate: Candidate? = null

    private val recordsFlow = MutableStateFlow<List<AccountRecord>>(emptyList())
    private val activeFlow = MutableStateFlow<AccountRecord?>(null)
    private val statuses = MutableStateFlow<Map<String, ConnectionStatus>>(emptyMap())
    private val cacheCounts = MutableStateFlow<Map<String, Long>>(emptyMap())
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Default)
    private val random = SecureRandom()

    /** Gives the object the application. Called once by `KwebApp`; later calls from anywhere are harmless. */
    fun attach(context: Context) {
        if (!::app.isInitialized) app = context.applicationContext
    }

    val records: StateFlow<List<AccountRecord>> get() = recordsFlow

    /** The account the screens show; null when there is none. */
    val active: StateFlow<AccountRecord?> get() = activeFlow

    /** All accounts with the last state their connection reported. Not blocking. */
    val connection: StateFlow<ConnectionSummary> by lazy {
        // The first value is computed from what is known now, never an empty placeholder: a reader that acts on
        // "no accounts" (the service ends itself then) must not see one for accounts that exist.
        combine(recordsFlow, statuses, ::summarise)
            .stateIn(scope, SharingStarted.Eagerly, summarise(recordsFlow.value, statuses.value))
    }

    private fun summarise(records: List<AccountRecord>, map: Map<String, ConnectionStatus>) =
        ConnectionSummary(records.map { AccountStatus(it, map[it.id] ?: ConnectionStatus.Connecting) })

    /** One counter per account that grows with every "the cache changed" call of its core. Not blocking. */
    val cacheTicks: StateFlow<Map<String, Long>> get() = cacheCounts

    /** Loads the registry (migrating and sweeping on the first call) and returns the accounts. Blocks. */
    fun load(context: Context): List<AccountRecord> {
        attach(context)
        synchronized(loadLock) {
            if (store == null) {
                val made = AccountStore(
                    storage = AccountRegistry(app),
                    files = AndroidAccountFiles(app),
                    readLegacy = ::readLegacy,
                    newId = ::newId,
                    now = System::currentTimeMillis,
                )
                val state = made.load()
                store = made
                publish(state)
            }
        }
        return recordsFlow.value
    }

    /**
     * The account of the cache an install made before several accounts existed has, or null when it is signed out
     * (no account row at all). A row whose sign-in entry is damaged still counts as an account (it needs a new
     * sign-in, and keeps its drafts and pending mail), so it is read from the row itself.
     */
    private fun readLegacy(): LegacyAccount? {
        val path = File(app.filesDir, AccountPaths.cache("", true)).path
        val core = Core(path, KeystoreSecrets(app, "", true))
        val account = core.account()
        if (account != null) {
            preopened = core
            return LegacyAccount(account.username, account.serverUrl, account.id)
        }
        val row = readAccountRow(path) ?: return null
        preopened = core
        return row
    }

    private fun readAccountRow(path: String): LegacyAccount? = try {
        android.database.sqlite.SQLiteDatabase.openDatabase(path, null, android.database.sqlite.SQLiteDatabase.OPEN_READONLY).use { db ->
            db.rawQuery("SELECT id, server_url, username FROM accounts ORDER BY rowid LIMIT 1", null).use { c ->
                if (c.moveToFirst()) LegacyAccount(c.getString(2), c.getString(1), c.getString(0)) else null
            }
        }
    } catch (e: android.database.SQLException) {
        Log.w(Background.TAG, "reading the account row of the old cache failed", e)
        null
    }

    private fun newId(): String = ByteArray(8).also(random::nextBytes).joinToString("") { "%02x".format(it) }

    private fun publish(state: RegistryState) {
        recordsFlow.value = state.accounts
        activeFlow.value = state.activeRecord
    }

    private fun requireStore(): AccountStore {
        if (store == null) load(app)
        return store!!
    }

    private val openLocks = java.util.concurrent.ConcurrentHashMap<String, Any>()

    /**
     * The session of account [id], made on first use (blocks then). Null when there is no such account. The
     * Core is opened under a lock of this account only, so a caller asking for an account that is already open
     * (the main thread, say) never waits for another account's cache to open.
     */
    fun session(id: String): AccountSession? {
        synchronized(sessionLock) { sessions[id] }?.let { return it }
        requireStore()
        return synchronized(openLocks.getOrPut(id) { Any() }) {
            synchronized(sessionLock) { sessions[id] }?.let { return it }
            val record = recordsFlow.value.firstOrNull { it.id == id } ?: return null
            val taken = synchronized(sessionLock) {
                if (record.legacy) preopened.also { preopened = null } else null
            }
            val core = taken ?: openCore(record.id, record.legacy, record)
            synchronized(sessionLock) { register(record, core) }
        }
    }

    /** The sessions that exist now, without opening any (safe on the main thread). */
    fun openSessions(): List<AccountSession> = synchronized(sessionLock) { sessions.values.toList() }

    /**
     * The sessions of all accounts, opening those not open yet. An account that cannot be opened (a damaged
     * cache) is left out and logged, so it cannot stop the work of the others.
     */
    fun sessions(): List<AccountSession> = recordsFlow.value.mapNotNull {
        try {
            session(it.id)
        } catch (e: Exception) {
            Log.e(Background.TAG, "opening an account failed", e)
            null
        }
    }

    fun setActive(id: String) {
        requireStore().setActive(id)
        publish(store!!.state)
    }

    private fun openCore(id: String, legacy: Boolean, record: AccountRecord): Core {
        val file = File(app.filesDir, AccountPaths.cache(id, legacy))
        file.parentFile?.mkdirs()
        return Core(file.path, KeystoreSecrets(app, record))
    }

    private fun register(record: AccountRecord, core: Core): AccountSession {
        val session = AccountSession(
            record, core, app,
            onStatus = { r, status -> statuses.update { it + (r.id to status) } },
            onCache = { r -> cacheCounts.update { it + (r.id to ((it[r.id] ?: 0L) + 1)) } },
        )
        sessions[record.id] = session
        statuses.update { it + (record.id to session.status.value) }
        return session
    }

    /** What an add in progress holds: the id and a Core on its own files, which no other account shares. */
    class Candidate internal constructor(val id: String, val core: Core)

    /**
     * Starts adding an account: the id is committed as discardable before any file exists, then a Core is made
     * on `accounts/<id>/` with its own secrets file. A previous candidate that was never finished is abandoned.
     */
    fun beginAdd(): Candidate {
        synchronized(sessionLock) { candidate?.let { abandonLocked(it) } }
        val store = requireStore()
        val id = store.beginAdd()
        val stub = AccountRecord(id, false, "", "", "", System.currentTimeMillis())
        // Opening the cache is not done under the session lock: other threads need it for a lookup.
        val core = try {
            openCore(id, false, stub)
        } catch (e: Exception) {
            runCatching { store.abandon(id) }
            throw e
        }
        return Candidate(id, core).also { synchronized(sessionLock) { candidate = it } }
    }

    /**
     * The candidate's sign-in is complete: the account joins the list and becomes the active one. An account
     * the app already has (same server, same login) is refused with [AccountAlreadyAdded]; only the candidate's
     * own grant is revoked then, never the existing account's.
     */
    fun finishAdd(c: Candidate): AccountRecord {
        val account = c.core.account()
        if (account == null) {
            abandon(c)
            throw IllegalStateException("the sign-in did not complete")
        }
        val record = AccountRecord(c.id, false, account.username, account.serverUrl, account.id, System.currentTimeMillis())
        var first = false
        val added = synchronized(sessionLock) {
            if (AccountRules.isDuplicate(recordsFlow.value, record)) {
                false
            } else {
                first = recordsFlow.value.isEmpty()
                val store = requireStore()
                store.promote(record)
                candidate = null
                register(record, c.core)
                publish(store.state)
                true
            }
        }
        if (added) {
            if (first) carryOverLegacySwitches(record)
            return record
        }
        // The revoke is a network call, so it runs outside the lock.
        try {
            c.core.signOut()
        } catch (e: Exception) {
            Log.w(Background.TAG, "revoking the duplicate sign-in failed", e)
        }
        abandon(c)
        throw AccountAlreadyAdded()
    }

    /**
     * The first account added after the app was signed out keeps the three switches the old, single-account app
     * left in its `settings` file (its sign-out kept them on purpose); the file is then gone.
     */
    private fun carryOverLegacySwitches(record: AccountRecord) {
        try {
            Settings.of(app, record).takeSwitchesFrom(Settings.legacy(app))
            app.deleteSharedPreferences(AccountPaths.settingsPrefs("", true))
        } catch (e: Exception) {
            Log.w(Background.TAG, "carrying the old switches over failed", e)
        }
    }

    /** The add did not finish (cancelled, failed, timed out): its files are deleted. Safe to call twice. */
    fun abandon(c: Candidate) = synchronized(sessionLock) { abandonLocked(c) }

    private fun abandonLocked(c: Candidate) {
        if (candidate === c) candidate = null
        try {
            requireStore().abandon(c.id)
        } catch (e: Exception) {
            Log.w(Background.TAG, "removing the files of an unfinished sign-in failed", e)
        }
    }

    /**
     * After signing in again: takes over what the account's own cache now says (the login may have changed). The
     * browser decides who signs in, so it can be a user the app already has under another entry: then this entry
     * is a second copy of that one, so it is removed (its grant revoked) and [AccountAlreadyAdded] is thrown.
     */
    fun refreshRecord(id: String) {
        val session = session(id) ?: return
        val account = session.core.account() ?: return
        val updated = session.record.copy(username = account.username, serverUrl = account.serverUrl, jmapAccountId = account.id)
        if (AccountRules.isDuplicate(recordsFlow.value, updated)) {
            remove(id)
            throw AccountAlreadyAdded()
        }
        requireStore().replace(updated)
        session.record = updated
        publish(store!!.state)
    }

    /**
     * Removes one account: signs it out (its loop stops, its grant is revoked, its rows and secrets go), takes it
     * off the list, cancels its notifications and deletes its files. The other accounts are not touched. Returns
     * false, changing nothing, when the account could not be signed out. Blocks.
     */
    fun remove(id: String): Boolean {
        val session = try {
            session(id)
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            // The cache cannot be opened, so there is no grant to revoke through it; the files still go.
            Log.w(Background.TAG, "the account could not be opened to sign it out", e)
            val record = recordsFlow.value.firstOrNull { it.id == id } ?: return true
            finishRemoval(record)
            return true
        } ?: return true
        try {
            session.core.signOut()
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            Log.w(Background.TAG, "sign-out reported an error", e)
        }
        // Whatever the error was, an account that is gone from the core is signed out.
        if (session.core.account() != null) return false
        finishRemoval(session.record)
        return true
    }

    private fun finishRemoval(record: AccountRecord) {
        val notifier = Notifier(app)
        requireStore().remove(record.id) { notifier.cancelAccount(it) }
        synchronized(sessionLock) {
            sessions.remove(record.id)
        }
        statuses.update { it - record.id }
        cacheCounts.update { it - record.id }
        Settings.forget(record)
        publish(store!!.state)
        Background.afterAccountRemoved(app, record)
    }
}
