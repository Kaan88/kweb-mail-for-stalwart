package ovh.kweb.mail.ui.shared

import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.receiveAsFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import ovh.kweb.mail.ui.UiMessage

/** What a controller tells the screen besides its state. */
sealed interface SharedEvent {
    /** Something to tell the user, as a snackbar. */
    data class Say(val message: UiMessage) : SharedEvent

    /** The share is gone (see [SharedRules.isGone]): go back to the shared screen, which reads itself again. */
    data object Gone : SharedEvent

    /** What the user asked for is done and the screen has nothing left to show (a message was moved or deleted): go back. */
    data object Done : SharedEvent
}

/** Runs [block] on this dispatcher; a failure is a [Result], a cancellation is not swallowed. */
internal suspend fun <T> CoroutineDispatcher.attempt(block: () -> T): Result<T> = try {
    Result.success(withContext(this) { block() })
} catch (e: CancellationException) {
    throw e
} catch (e: Exception) {
    Result.failure(e)
}

/** The events of a controller: a channel the screen collects from. */
internal class SharedEvents {
    private val channel = Channel<SharedEvent>(Channel.UNLIMITED)
    val flow: Flow<SharedEvent> get() = channel.receiveAsFlow()

    fun send(event: SharedEvent) {
        channel.trySend(event)
    }

    fun say(message: UiMessage) = send(SharedEvent.Say(message))

    fun clear() {
        while (channel.tryReceive().isSuccess) { }
    }
}

data class LoadState<T>(
    val value: T? = null,
    /** Nothing to show yet: the first answer is on its way. */
    val loading: Boolean = false,
    /** Pull to refresh is under way over what is shown. */
    val refreshing: Boolean = false,
    /** Why there is nothing to show (or why the share is gone). An error over something already shown is only said. */
    val error: UiMessage? = null,
)

/**
 * One thing read from the core and shown: an account list, the folders of an account, an event, a contact.
 * Only the newest request counts, so an answer that comes late never replaces a newer one. A read that is
 * not the first is quiet: what is shown stays until the new answer is there, and an error over it is not shown.
 * With [goneOnInvalid] a refusal by the core ends in [SharedEvent.Gone].
 */
class LoadController<T>(
    private val scope: CoroutineScope,
    private val io: CoroutineDispatcher,
    private val goneOnInvalid: Boolean,
    private val fetch: () -> T,
) {
    private val flow = MutableStateFlow(LoadState<T>())
    private val bus = SharedEvents()
    private var generation = 0

    val state: StateFlow<LoadState<T>> get() = flow
    val events: Flow<SharedEvent> get() = bus.flow

    /** The screen came up (also from the background): the first time it reads, later it reads again quietly. */
    fun show() = start(manual = false)

    /** Pull to refresh, and Retry. */
    fun refresh() = start(manual = true)

    /** Signed out or left for good: forget everything and ignore what is still on its way. */
    fun reset() {
        generation++
        flow.value = LoadState()
        bus.clear()
    }

    private fun start(manual: Boolean) {
        val gen = ++generation
        flow.update {
            val has = it.value != null
            it.copy(loading = !has, refreshing = manual && has, error = null)
        }
        scope.launch {
            val result = io.attempt(fetch)
            if (gen != generation) return@launch
            result.onSuccess { value -> flow.value = LoadState(value = value) }.onFailure { e ->
                val gone = goneOnInvalid && SharedRules.isGone(e)
                val message = SharedRules.message(e)
                val had = flow.value.value != null
                flow.update { it.copy(loading = false, refreshing = false, error = if (!had || gone) message else null) }
                if (gone) bus.send(SharedEvent.Gone) else if (manual && had) bus.say(message)
            }
        }
    }
}
