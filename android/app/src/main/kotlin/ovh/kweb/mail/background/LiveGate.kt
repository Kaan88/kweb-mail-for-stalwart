package ovh.kweb.mail.background

/** One live loop of a Google-mode account, as the gate sees it. Both calls block. */
interface LiveLoop {
    fun start()
    fun stop()
}

/**
 * When the live loops of Google-mode accounts run: while the app is on screen and [ModeRules.LIVE_STOP_DELAY_MS] after
 * it left. Kept free of Android so the orders that go wrong can be tested with a fake clock and fake loops.
 *
 * The flag follows the activity at once ([screenOn], [screenOff] run on the thread that reports the change, in order);
 * the slow work (starting loops) comes after and checks the flag again, so a start that was asked for before the app
 * left and runs after it cannot leave a loop behind: the stop that follows every [screenOff] takes it. A stop that
 * runs at the moment the user comes back starts the loop again.
 *
 * [later] runs a task after a delay and returns what cancels it; [owed] asks what the user left owed to the server.
 */
class LiveGate(
    private val loops: () -> List<LiveLoop>,
    private val later: (Long, () -> Unit) -> () -> Unit,
    private val owed: () -> Unit,
    private val stopDelayMs: Long = ModeRules.LIVE_STOP_DELAY_MS,
) {
    private val lock = Any()
    private var cancelStop: (() -> Unit)? = null

    @Volatile var onScreen = false
        private set

    /** The app came to the screen: no stop is pending any more. Call [startLoops] after (off the main thread). */
    fun screenOn() = synchronized(lock) {
        onScreen = true
        cancelStop?.invoke()
        cancelStop = null
    }

    /** The app left the screen: the loops stop later, and what is owed is asked for. */
    fun screenOff() {
        synchronized(lock) {
            onScreen = false
            cancelStop?.invoke()
            cancelStop = later(stopDelayMs) { stopNow() }
        }
        later(0) { owed() }
    }

    /** Starts every loop, unless the app already left again (the pending stop handles any that started). */
    fun startLoops() {
        for (loop in loops()) {
            if (!onScreen) return
            runCatching { loop.start() }
        }
    }

    /** The stop at the end of the delay, also called when the mode of an account changed while the app is away. */
    fun stopNow() {
        if (onScreen) return
        for (loop in loops()) {
            if (onScreen) return
            runCatching { loop.stop() }
            // The user came back while this one was stopping: it runs again.
            if (onScreen) runCatching { loop.start() }
        }
        if (!onScreen) owed()
    }
}
