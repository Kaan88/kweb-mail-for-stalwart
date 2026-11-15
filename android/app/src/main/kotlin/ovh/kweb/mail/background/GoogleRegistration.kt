package ovh.kweb.mail.background

import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.os.Handler
import android.os.Looper
import android.os.Messenger
import android.util.Log
import java.util.concurrent.ConcurrentHashMap
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withTimeoutOrNull

/** What Play services said in its `REGISTRATION` broadcast. */
sealed interface RegistrationReply {
    /** The token for the account named in the `kid`. */
    data class Token(val accountId: String, val token: String) : RegistrationReply

    /** The answer to an unregister (the "token" is the package name). */
    data class Unregistered(val accountId: String) : RegistrationReply

    /** An `error` extra. [accountId] is null when the reply does not say whose. */
    data class Error(val accountId: String?, val error: String) : RegistrationReply

    /** Nothing usable in it. */
    data object Unknown : RegistrationReply
}

/**
 * Registration with Play services by the intent the Mastodon app uses: no Firebase project and no Google
 * library. The pure parts (extras, reading the reply) are tested; [request] does the Android part.
 */
object GoogleRegistration {
    const val GMS_PACKAGE = "com.google.android.gms"
    const val ACTION_REGISTER = "com.google.android.c2dm.intent.REGISTER"
    const val ACTION_REGISTRATION = "com.google.android.c2dm.intent.REGISTRATION"
    const val ACTION_RECEIVE = "com.google.android.c2dm.intent.RECEIVE"
    const val PERMISSION_SEND = "com.google.android.c2dm.permission.SEND"
    const val PERMISSION_RECEIVE = "com.google.android.c2dm.permission.RECEIVE"
    const val EXTRA_REGISTRATION_ID = "registration_id"
    const val EXTRA_ERROR = "error"
    const val EXTRA_SUBTYPE = "subtype"
    const val EXTRA_RAW_DATA = "rawData"
    const val EXTRA_MESSAGE_TYPE = "message_type"

    /** The `message_type` of Google's notice that it discarded pending messages for this app. */
    const val TYPE_DELETED_MESSAGES = "deleted_messages"

    private val replyId = Regex("""^\|ID\|([^|]+)\|:(.*)$""")

    /** The `kid` that comes back in the reply and names the account. */
    fun kid(accountId: String): String = "|ID|$accountId|"

    /**
     * Every extra, old names and new, as the Mastodon app sends them (Play services answers
     * `INVALID_PARAMETERS` to the new names alone and to the old names alone it only registers legacy ones).
     * [delete] adds `delete=1`, which unregisters.
     */
    fun extras(accountId: String, sender: String, subtype: String, delete: Boolean): Map<String, String> = buildMap {
        put("sender", sender)
        put("subscription", sender)
        put("X-subscription", sender)
        put("subtype", subtype)
        put("X-subtype", subtype)
        put("scope", "GCM")
        put("kid", kid(accountId))
        if (delete) put("delete", "1")
    }

    /** Reads `registration_id` and `error` of the reply. [packageName]: a "token" equal to it answers an unregister. */
    fun parseReply(registrationId: String?, error: String?, packageName: String): RegistrationReply {
        val match = registrationId?.let { replyId.matchEntire(it) }
        val account = match?.groupValues?.get(1)
        if (!error.isNullOrEmpty()) {
            // Play services can put the kid in front of the error as it does with a token: then it says whose it is.
            val named = replyId.matchEntire(error)
            return if (named != null) RegistrationReply.Error(named.groupValues[1], named.groupValues[2]) else RegistrationReply.Error(account, error)
        }
        if (match == null) return RegistrationReply.Unknown
        val token = match.groupValues[2]
        return when {
            token == packageName -> RegistrationReply.Unregistered(match.groupValues[1])
            token.isEmpty() -> RegistrationReply.Unknown
            else -> RegistrationReply.Token(match.groupValues[1], token)
        }
    }

    /** What a waiting request gets from [parseReply] result, or null when the reply is not for it. */
    fun answerFor(reply: RegistrationReply, accountId: String, delete: Boolean): RegistrationResult? = when (reply) {
        is RegistrationReply.Token -> if (reply.accountId == accountId && !delete) RegistrationResult.Token(reply.token) else null
        is RegistrationReply.Unregistered -> if (reply.accountId == accountId && delete) RegistrationResult.Token("") else null
        is RegistrationReply.Error -> if (reply.accountId == null || reply.accountId == accountId) RegistrationResult.Refused(reply.error) else null
        RegistrationReply.Unknown -> null
    }

    private class Waiter(val accountId: String, val delete: Boolean, val result: CompletableDeferred<RegistrationResult>)

    private val waiters = ConcurrentHashMap<String, Waiter>()

    /**
     * One request at a time: an error without an account then belongs to the one that is waiting, unless an earlier
     * request ended without its answer (see [mayBeStray]).
     */
    private val oneAtATime = Mutex()

    /** When a request last ended without an answer (monotonic ms), or null. Its answer may still come. */
    @Volatile private var unansweredAt: Long? = null

    private fun monotonicMs(): Long = System.nanoTime() / 1_000_000

    /** How long after a request that got no answer an error naming no account is not trusted. */
    const val STRAY_WINDOW_MS = 60_000L

    /**
     * An error that names no account, arriving within [STRAY_WINDOW_MS] of a request that ended without its answer,
     * may be the late answer to that request and not to the one waiting now (possibly another account's). It is
     * dropped: the waiting request then ends as "no answer", which never tears down a working registration at once.
     */
    fun mayBeStray(reply: RegistrationReply, unansweredAgoMs: Long?): Boolean =
        reply is RegistrationReply.Error && reply.accountId == null && unansweredAgoMs != null && unansweredAgoMs < STRAY_WINDOW_MS

    /** The extra that hands Play services a direct way to answer, as Google's own library does. */
    const val EXTRA_MESSENGER = "google.messenger"

    /**
     * The second road for the answer. Play services answers a request that carries a messenger through it and
     * not by broadcast; on some phones the broadcast never reaches the app, and the request then ended as
     * "no answer". The broadcast receiver stays for a Play services that ignores the messenger.
     */
    private val replyMessenger by lazy {
        Messenger(
            Handler(Looper.getMainLooper()) { message ->
                (message.obj as? Intent)?.let { onReply(it, appPackage, "directly") }
                true
            },
        )
    }

    @Volatile private var appPackage: String = ""

    /** An answer from Play services, by broadcast or through the messenger. Never logs the token. */
    fun onReply(intent: Intent, packageName: String, road: String) {
        val reply = try {
            parseReply(intent.getStringExtra(EXTRA_REGISTRATION_ID), intent.getStringExtra(EXTRA_ERROR), packageName)
        } catch (e: RuntimeException) {
            // Extras that cannot be read (a class this app does not have).
            RegistrationReply.Unknown
        }
        // The kind only (and Play services' error name): never the token.
        val kind = when (reply) {
            is RegistrationReply.Token -> "a token"
            is RegistrationReply.Unregistered -> "unregistered"
            is RegistrationReply.Error -> "error ${reply.error}"
            RegistrationReply.Unknown -> "nothing usable"
        }
        Log.i(Background.TAG, "Play services answered a registration request $road: $kind")
        deliver(reply)
    }

    /** Called by [onReply]. A reply nobody waits for (the process died meanwhile, or it came twice) is dropped. */
    fun deliver(reply: RegistrationReply) {
        if (mayBeStray(reply, unansweredAt?.let { monotonicMs() - it })) return
        for (w in waiters.values) {
            val answer = answerFor(reply, w.accountId, w.delete) ?: continue
            w.result.complete(answer)
        }
    }

    /**
     * Asks Play services for a token ([delete] false) or to forget the subtype ([delete] true) and waits
     * [timeoutMs] for the answer. The intent throwing, or no answer in time, is [RegistrationResult.NoPlayServices].
     * An unregister that was answered gives `Token("")`.
     */
    suspend fun request(
        context: Context,
        accountId: String,
        sender: String,
        subtype: String,
        delete: Boolean,
        timeoutMs: Long,
    ): RegistrationResult = oneAtATime.withLock {
        val app = context.applicationContext
        val waiter = Waiter(accountId, delete, CompletableDeferred())
        waiters[accountId] = waiter
        var asked = false
        try {
            val intent = Intent(ACTION_REGISTER).setPackage(GMS_PACKAGE)
            intent.putExtra("app", PendingIntent.getBroadcast(app, 0, Intent(), PendingIntent.FLAG_IMMUTABLE))
            appPackage = app.packageName
            intent.putExtra(EXTRA_MESSENGER, replyMessenger)
            extras(accountId, sender, subtype, delete).forEach { (k, v) -> intent.putExtra(k, v) }
            try {
                if (app.startService(intent) == null) return@withLock RegistrationResult.NoPlayServices("no service", absent = true)
            } catch (e: Exception) {
                return@withLock RegistrationResult.NoPlayServices(e.javaClass.simpleName, absent = true)
            }
            asked = true
            withTimeoutOrNull(timeoutMs) { waiter.result.await() } ?: RegistrationResult.NoPlayServices("no answer")
        } finally {
            waiters.remove(accountId)
            // Asked, and left without the answer (time ran out, or the request was cancelled): it may still come.
            if (asked && !waiter.result.isCompleted) unansweredAt = monotonicMs()
        }
    }
}
