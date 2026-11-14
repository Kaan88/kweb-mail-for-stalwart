package ovh.kweb.mail.background

import android.app.PendingIntent
import android.content.Context
import android.content.Intent
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
        if (!error.isNullOrEmpty()) return RegistrationReply.Error(account, error)
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

    /** One request at a time: an error without an account then belongs to the one that is waiting. */
    private val oneAtATime = Mutex()

    /** Called by [PushRegistrationReceiver]. A reply nobody waits for (the process died meanwhile) is dropped. */
    fun deliver(reply: RegistrationReply) {
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
        try {
            val intent = Intent(ACTION_REGISTER).setPackage(GMS_PACKAGE)
            intent.putExtra("app", PendingIntent.getBroadcast(app, 0, Intent(), PendingIntent.FLAG_IMMUTABLE))
            extras(accountId, sender, subtype, delete).forEach { (k, v) -> intent.putExtra(k, v) }
            try {
                if (app.startService(intent) == null) return@withLock RegistrationResult.NoPlayServices("no service")
            } catch (e: Exception) {
                return@withLock RegistrationResult.NoPlayServices(e.javaClass.simpleName)
            }
            withTimeoutOrNull(timeoutMs) { waiter.result.await() } ?: RegistrationResult.NoPlayServices("no answer")
        } finally {
            waiters.remove(accountId)
        }
    }
}
