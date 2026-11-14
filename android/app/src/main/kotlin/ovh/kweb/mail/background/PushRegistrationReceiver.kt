package ovh.kweb.mail.background

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.util.Log

/**
 * Play services answers a registration request with a plain broadcast: `registration_id` is
 * `|ID|<account id>|:<token>` (the `kid` we sent names the account) or an `error`. Exported, guarded by the
 * permission only Play services holds. Nothing here blocks or logs the token.
 */
class PushRegistrationReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        if (intent.action != GoogleRegistration.ACTION_REGISTRATION) return
        val reply = GoogleRegistration.parseReply(
            intent.getStringExtra(GoogleRegistration.EXTRA_REGISTRATION_ID),
            intent.getStringExtra(GoogleRegistration.EXTRA_ERROR),
            context.packageName,
        )
        // The kind only (and Play services' error name): never the token.
        val kind = when (reply) {
            is RegistrationReply.Token -> "a token"
            is RegistrationReply.Unregistered -> "unregistered"
            is RegistrationReply.Error -> "error ${reply.error}"
            RegistrationReply.Unknown -> "nothing usable"
        }
        Log.i(Background.TAG, "Play services answered a registration request: $kind")
        GoogleRegistration.deliver(reply)
    }
}
