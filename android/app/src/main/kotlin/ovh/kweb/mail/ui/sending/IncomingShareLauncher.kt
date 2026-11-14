package ovh.kweb.mail.ui.sending

import android.app.Application
import android.widget.Toast
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import ovh.kweb.mail.R
import ovh.kweb.mail.accounts.AccountSession
import ovh.kweb.mail.ui.SessionState
import ovh.kweb.mail.ui.UiMessage
import ovh.kweb.mail.ui.compose.StartedFromShare

/** What the app does with a share that arrived: the steps around [IncomingShareIntake] that need the session and the screen. */
object IncomingShareLauncher {
    /**
     * Parses [raw], and when somebody is signed in starts a new draft from it and calls [open] with the draft's id
     * (even when a draft is open already: nothing is asked) in the account [session] gives. [notify] gets what the user should be told.
     */
    fun handle(
        app: Application,
        scope: CoroutineScope,
        raw: RawShare,
        session: suspend () -> AccountSession?,
        open: (String) -> Unit,
        notify: (UiMessage) -> Unit,
    ) {
        val content = IncomingShareRules.parse(raw, IntentShare.ownAuthority(app))
        scope.launch {
            val signedIn = SessionState.signedIn.first { it != null }
            if (signedIn != true) {
                // The sign-in screen has no message bar of its own.
                Toast.makeText(app, R.string.incoming_share_sign_in_first, Toast.LENGTH_LONG).show()
                return@launch
            }
            if (content == null) {
                notify(UiMessage.Res(R.string.incoming_share_nothing))
                return@launch
            }
            val target = session() ?: return@launch
            val outcome = try {
                withContext(Dispatchers.IO) { IncomingShareIntake(CoreShareBackend(target.core), AndroidShareFiles(app)).run(content) }
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                ShareOutcome.Failed
            }
            when (outcome) {
                ShareOutcome.Failed -> notify(UiMessage.Res(R.string.incoming_share_failed))
                is ShareOutcome.Ready -> {
                    // Text the user shared in is not a blank message: leaving the screen must keep it.
                    if (content.body.isNotEmpty()) StartedFromShare.mark(outcome.draftId)
                    open(outcome.draftId)
                    if (outcome.left > 0) notify(UiMessage.Text(app.resources.getQuantityString(R.plurals.incoming_share_files_left, outcome.left, outcome.left)))
                }
            }
        }
    }
}
