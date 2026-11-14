package ovh.kweb.mail.background

import kotlinx.coroutines.CancellationException

/** One failed step of a [CheckRound]. */
internal data class CheckFailure<T>(val item: T, val error: Exception)

/** The loop of the background check over all accounts, in order, as a plain function. */
internal object CheckRound {
    /**
     * Runs [step] for every item. One failing does not skip the next; the failures come back in order.
     * A cancellation stops the whole round.
     */
    fun <T> runAll(items: List<T>, step: (T) -> Unit): List<CheckFailure<T>> {
        val failures = mutableListOf<CheckFailure<T>>()
        for (item in items) {
            try {
                step(item)
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                failures += CheckFailure(item, e)
            }
        }
        return failures
    }
}
