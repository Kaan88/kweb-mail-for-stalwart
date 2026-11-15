package ovh.kweb.mail.background

import java.util.concurrent.Callable
import java.util.concurrent.ExecutionException
import java.util.concurrent.ExecutorService
import java.util.concurrent.TimeUnit
import java.util.concurrent.TimeoutException
import uniffi.kweb_core.Plan
import uniffi.kweb_core.Reminder
import uniffi.kweb_core.WebPushMessage

/** What the receiver did with one message. */
sealed interface MessageOutcome {
    data object Applied : MessageOutcome
    data object ReminderShown : MessageOutcome
    data object Verified : MessageOutcome

    /** Ignored, undecryptable, or not for any account: nothing to do. */
    data object Nothing : MessageOutcome

    /** The work did not finish in the budget or failed: the expedited one-off check was asked for. */
    data object HandedOver : MessageOutcome
}

/**
 * The decision logic of a received Web Push, with every side effect passed in so a test can use fakes
 * (decision 3). The core call runs on a thread of [executor]; if it has not finished after [budgetMs] or
 * fails, [checkSoon] is called and the receiver finishes. The call goes on meanwhile and its result still counts
 * when it comes (a late plan was shown with its alert by the account's listener inside the pass, a late reminder
 * is shown); that is safe: the core's passes take turns. [apply] is what follows a pass that went well; it must
 * not post the plan a second time.
 */
class PushMessageHandler(
    private val executor: ExecutorService,
    private val budgetMs: Long = ModeRules.RECEIVER_BUDGET_MS,
    private val receive: () -> WebPushMessage,
    private val apply: (Plan) -> Unit,
    private val showReminder: (Reminder) -> Unit,
    private val checkSoon: () -> Unit,
) {
    fun run(): MessageOutcome {
        // The task finishes its own work whether or not the receiver still waits: the plan of the pass that found the
        // mail (with its alert) is the one that is shown, by the listener inside that pass.
        val task = executor.submit(Callable { deliver() })
        return try {
            task.get(budgetMs, TimeUnit.MILLISECONDS)
        } catch (e: TimeoutException) {
            handOver()
        } catch (e: ExecutionException) {
            // The pass failed, or showing failed: a second pass puts it right.
            handOver()
        } catch (e: InterruptedException) {
            Thread.currentThread().interrupt()
            handOver()
        }
    }

    private fun deliver(): MessageOutcome = when (val message = receive()) {
        is WebPushMessage.Checked -> { apply(message.plan); MessageOutcome.Applied }
        is WebPushMessage.Reminder -> { showReminder(message.reminder); MessageOutcome.ReminderShown }
        is WebPushMessage.Verified -> MessageOutcome.Verified
        is WebPushMessage.Ignored, is WebPushMessage.Undecryptable -> MessageOutcome.Nothing
    }

    private fun handOver(): MessageOutcome {
        checkSoon()
        return MessageOutcome.HandedOver
    }

    companion object {
        /** The account whose registration subtype is [subtype]; [subtypes] pairs account ids with theirs. Null drops the message. */
        fun accountFor(subtype: String?, subtypes: List<Pair<String, String?>>): String? {
            if (subtype.isNullOrEmpty()) return null
            return subtypes.firstOrNull { it.second == subtype }?.first
        }

        /**
         * A message that cannot be read (no body, or no account) is dropped. Whether a check must look instead:
         * yes when Google says it discarded messages ([messageType] `deleted_messages`, which names no account), and
         * yes when the message is for an account of ours ([accountKnown]) and only its body is missing. A message for
         * nobody we know stays a plain drop.
         */
        fun dropNeedsCheck(messageType: String?, accountKnown: Boolean): Boolean =
            messageType == GoogleRegistration.TYPE_DELETED_MESSAGES || accountKnown
    }
}
