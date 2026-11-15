package ovh.kweb.mail.background

import android.app.PendingIntent
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.content.ServiceConnection
import android.os.Bundle
import android.os.Handler
import android.os.IBinder
import android.os.Looper
import android.os.Message
import android.os.Messenger
import android.util.Log
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.atomic.AtomicInteger
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.selects.select
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
 * Registration with Play services by the intents Google's own library and the Mastodon app use: no Firebase
 * project and no Google library. The pure parts (extras, reading the reply) are tested; [request] does the Android part.
 */
object GoogleRegistration {
    const val GMS_PACKAGE = "com.google.android.gms"
    const val ACTION_REGISTER = "com.google.android.c2dm.intent.REGISTER"
    const val ACTION_TOKEN_REQUEST = "com.google.iid.TOKEN_REQUEST"
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

    private val replyId = Regex("""^\|ID\|([^|]+)\|:?(.*)$""")

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
        if (!deliver(reply)) unmatched = unmatchedWord(reply, intent.hasExtra(EXTRA_REGISTRATION_ID))
    }

    /**
     * An answer that ended no waiting request, for the Delivery check: without it such an answer looks the same as
     * silence. Says the kind only, never the token.
     */
    fun unmatchedWord(reply: RegistrationReply, hadRegistrationId: Boolean): String = when (reply) {
        is RegistrationReply.Token, is RegistrationReply.Unregistered -> "an answer for another request"
        is RegistrationReply.Error -> "error ${reply.error}, not taken"
        RegistrationReply.Unknown -> if (hadRegistrationId) "an answer in an unknown form" else "an empty answer"
    }

    /** The last answer during the running request that ended nothing (see [unmatchedWord]), or null. */
    @Volatile private var unmatched: String? = null

    /** What a road that was sent and not answered did: silence, or an answer that could not be used. */
    fun unansweredText(unmatched: String?): String = if (unmatched == null) NO_ANSWER else "$NO_ANSWER (got $unmatched)"

    /**
     * Called by [onReply]. A reply nobody waits for (the process died meanwhile, or it came twice) is dropped.
     * True when it ended a waiting request.
     */
    fun deliver(reply: RegistrationReply): Boolean {
        if (mayBeStray(reply, unansweredAt?.let { monotonicMs() - it })) return false
        var taken = false
        for (w in waiters.values) {
            val answer = answerFor(reply, w.accountId, w.delete) ?: continue
            if (w.result.complete(answer)) taken = true
        }
        return taken
    }

    /** The ways to hand Play services the request. */
    enum class Road(val label: String) {
        /**
         * Binding to the `REGISTER` service and sending the request as a message: what Google's own library does on a
         * current Play services.
         */
        Bound("bound"),

        /** `sendBroadcast` of `com.google.iid.TOKEN_REQUEST`: what current Play services answers. */
        Broadcast("broadcast"),

        /** `startService` of `com.google.android.c2dm.intent.REGISTER`: the older road. */
        Service("service"),
    }

    /** What happened on one road. [text] is the word shown on the Delivery check: names only, never a token or key. */
    data class Attempt(val road: Road, val text: String) {
        val sent: Boolean get() = text == SENT || text.startsWith(NO_ANSWER)
    }

    const val SENT = "sent"
    const val NO_ANSWER = "no answer"
    const val NO_RECEIVER = "no receiver"
    const val NO_SERVICE = "no service"

    /**
     * The roads to try, in order, as Google's own library picks them: on Android 8 and later it always takes the
     * broadcast (the receiver is the default when nothing resolves) and the service road only on older phones.
     * Here the service road stays as a second try. A road Play services does not offer is left out; when it
     * offers neither (or the lookup failed), both are tried, the broadcast first.
     */
    fun roadsFor(hasReceiver: Boolean, hasService: Boolean): List<Road> = when {
        hasReceiver && hasService -> listOf(Road.Broadcast, Road.Service)
        hasReceiver -> listOf(Road.Broadcast)
        hasService -> listOf(Road.Service)
        else -> listOf(Road.Broadcast, Road.Service)
    }

    /** [roadsFor] with the bound road in front wherever Play services has the service (or nothing could be looked up). */
    fun allRoads(hasReceiver: Boolean, hasService: Boolean): List<Road> =
        (if (hasService || !hasReceiver) listOf(Road.Bound) else emptyList()) + roadsFor(hasReceiver, hasService)

    /**
     * What the answer on the bound road means for the request that is waiting, or null when nothing in it can be used.
     * There the token may come bare (the answer belongs to the message it answers) or with the kid in front.
     */
    fun boundAnswer(registrationId: String?, unregistered: String?, error: String?, accountId: String, delete: Boolean, packageName: String): RegistrationResult? {
        if (!error.isNullOrEmpty()) return RegistrationResult.Refused(replyId.matchEntire(error)?.groupValues?.get(2) ?: error)
        if (delete) return if (!unregistered.isNullOrEmpty() || registrationId != null) RegistrationResult.Token("") else null
        val raw = registrationId ?: return null
        val match = replyId.matchEntire(raw)
        if (match != null && match.groupValues[1] != accountId) return null
        val token = match?.groupValues?.get(2) ?: raw
        return if (token.isEmpty() || token == packageName) null else RegistrationResult.Token(token)
    }

    const val NO_BINDING = "no binding"
    const val UNSUPPORTED = "not supported"
    private const val MESSENGER_DESCRIPTOR = "android.os.IMessenger"
    private const val BIND_WAIT_MS = 5_000L
    private val boundIds = AtomicInteger(1)

    /** The bound request that is waiting: its id, whose it is, and how its wait ends early. */
    private class BoundWait(val id: Int, val waiter: Waiter, val over: CompletableDeferred<Unit>)

    @Volatile private var boundWait: BoundWait? = null

    private val boundMessenger by lazy {
        Messenger(
            Handler(Looper.getMainLooper()) { message ->
                onBoundReply(message.arg1, message.data)
                true
            },
        )
    }

    /** The answer to a bound request. Logs the names of what came, never a value. */
    private fun onBoundReply(id: Int, data: Bundle?) {
        val wait = boundWait?.takeIf { it.id == id } ?: return
        val answer = try {
            if (data?.getBoolean("unsupported", false) == true) {
                unmatched = UNSUPPORTED
                null
            } else {
                val inner = data?.getBundle("data")
                Log.i(Background.TAG, "Play services answered a bound registration request with: ${inner?.keySet()?.sorted()}")
                boundAnswer(
                    inner?.getString(EXTRA_REGISTRATION_ID), inner?.getString("unregistered"), inner?.getString(EXTRA_ERROR),
                    wait.waiter.accountId, wait.waiter.delete, appPackage,
                ).also { if (it == null) unmatched = "an answer in an unknown form" }
            }
        } catch (e: RuntimeException) {
            unmatched = "an answer that cannot be read"
            null
        }
        if (answer != null) wait.waiter.result.complete(answer) else wait.over.complete(Unit)
    }

    private class Binding : ServiceConnection {
        val binder = CompletableDeferred<IBinder?>()

        override fun onServiceConnected(name: ComponentName?, service: IBinder?) {
            binder.complete(service)
        }

        override fun onServiceDisconnected(name: ComponentName?) = Unit

        override fun onNullBinding(name: ComponentName?) {
            binder.complete(null)
        }

        override fun onBindingDied(name: ComponentName?) {
            binder.complete(null)
        }
    }

    /** Binds and sends the request as Google's library does (`what` 1, the extras under `data`); [SENT] or a short word. */
    private suspend fun sendBound(app: Context, binding: Binding, id: Int, extras: Map<String, String>): String = try {
        if (!app.bindService(Intent(ACTION_REGISTER).setPackage(GMS_PACKAGE), binding, Context.BIND_AUTO_CREATE)) {
            NO_SERVICE
        } else {
            val binder = withTimeoutOrNull(BIND_WAIT_MS) { binding.binder.await() }
            when {
                binder == null -> NO_BINDING
                binder.interfaceDescriptor != MESSENGER_DESCRIPTOR -> UNSUPPORTED
                else -> {
                    val message = Message.obtain()
                    message.what = 1
                    message.arg1 = id
                    message.replyTo = boundMessenger
                    message.data = Bundle().apply {
                        putBoolean("oneWay", false)
                        putString("pkg", app.packageName)
                        putBundle("data", Bundle().apply { extras.forEach { (k, v) -> putString(k, v) } })
                    }
                    Messenger(binder).send(message)
                    SENT
                }
            }
        }
    } catch (e: Exception) {
        "threw ${e.javaClass.simpleName}"
    }

    /** How long road number [index] of [count] waits: an equal share, and the last one gets what is left. */
    fun shareMs(timeoutMs: Long, index: Int, count: Int): Long =
        if (index >= count - 1) timeoutMs - (timeoutMs / count) * index else timeoutMs / count

    /** The line for the Delivery check: the Play services version and what each road did. */
    fun diagnostic(version: Long?, attempts: List<Attempt>): String =
        (listOf("Play services ${version ?: "unknown"}") + attempts.map { "${it.road.label}: ${it.text}" }).joinToString("; ")

    private fun gmsVersion(app: Context): Long? = try {
        app.packageManager.getPackageInfo(GMS_PACKAGE, 0).longVersionCode
    } catch (e: Exception) {
        null
    }

    private fun offers(app: Context, road: Road): Boolean = try {
        val probe = Intent(if (road == Road.Broadcast) ACTION_TOKEN_REQUEST else ACTION_REGISTER).setPackage(GMS_PACKAGE)
        val found = if (road == Road.Broadcast) app.packageManager.queryBroadcastReceivers(probe, 0) else app.packageManager.queryIntentServices(probe, 0)
        found.isNotEmpty()
    } catch (e: Exception) {
        false
    }

    private fun intentFor(app: Context, road: Road, accountId: String, sender: String, subtype: String, delete: Boolean): Intent {
        val intent = Intent(if (road == Road.Broadcast) ACTION_TOKEN_REQUEST else ACTION_REGISTER).setPackage(GMS_PACKAGE)
        intent.putExtra("app", PendingIntent.getBroadcast(app, 0, Intent(), PendingIntent.FLAG_IMMUTABLE))
        intent.putExtra(EXTRA_MESSENGER, replyMessenger)
        extras(accountId, sender, subtype, delete).forEach { (k, v) -> intent.putExtra(k, v) }
        return intent
    }

    /** Sends on one road; the outcome is [SENT] or a short word (an exception's class name, "no service"). */
    private fun send(app: Context, road: Road, intent: Intent): String = try {
        when (road) {
            Road.Broadcast -> {
                app.sendBroadcast(intent)
                SENT
            }
            Road.Service -> if (app.startService(intent) == null) NO_SERVICE else SENT
            Road.Bound -> NO_SERVICE
        }
    } catch (e: Exception) {
        "threw ${e.javaClass.simpleName}"
    }

    /**
     * Asks Play services for a token ([delete] false) or to forget the subtype ([delete] true) and waits
     * [timeoutMs] for the answer, shared between the roads (see [roadsFor]): a road that gets no answer in its share
     * is followed by the next. When no road could be sent, or none was answered, the result is
     * [RegistrationResult.NoPlayServices] whose detail says what each road did ([diagnostic]).
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
            appPackage = app.packageName
            val hasReceiver = offers(app, Road.Broadcast)
            val hasService = offers(app, Road.Service)
            val roads = allRoads(hasReceiver, hasService)
            val attempts = mutableListOf<Attempt>()
            if (!hasReceiver && hasService) attempts += Attempt(Road.Broadcast, NO_RECEIVER)
            if (hasReceiver && !hasService) attempts += Attempt(Road.Service, NO_SERVICE)
            for ((i, road) in roads.withIndex()) {
                unmatched = null
                val binding = if (road == Road.Bound) Binding() else null
                val over = CompletableDeferred<Unit>()
                try {
                    val outcome = if (binding != null) {
                        val id = boundIds.getAndIncrement()
                        boundWait = BoundWait(id, waiter, over)
                        sendBound(app, binding, id, extras(accountId, sender, subtype, delete))
                    } else {
                        send(app, road, intentFor(app, road, accountId, sender, subtype, delete))
                    }
                    Log.i(Background.TAG, "Registration request on the ${road.label} road: $outcome")
                    if (outcome != SENT) {
                        attempts += Attempt(road, outcome)
                        continue
                    }
                    asked = true
                    val answer = withTimeoutOrNull(shareMs(timeoutMs, i, roads.size)) {
                        select<RegistrationResult?> {
                            waiter.result.onAwait { it }
                            over.onAwait { null }
                        }
                    }
                    if (answer != null) return@withLock answer
                    attempts += Attempt(road, unansweredText(unmatched))
                } finally {
                    if (binding != null) {
                        boundWait = null
                        try {
                            app.unbindService(binding)
                        } catch (e: Exception) {
                            // Never bound.
                        }
                    }
                }
            }
            val detail = diagnostic(gmsVersion(app), attempts)
            Log.i(Background.TAG, "Registration failed: $detail")
            RegistrationResult.NoPlayServices(detail, absent = attempts.none { it.sent })
        } finally {
            waiters.remove(accountId)
            // Asked, and left without the answer (time ran out, or the request was cancelled): it may still come.
            if (asked && !waiter.result.isCompleted) unansweredAt = monotonicMs()
        }
    }
}
