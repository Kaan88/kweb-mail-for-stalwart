package ovh.kweb.mail

import android.content.Context

/**
 * Lets the Rust core check server certificates with Android's own trust store.
 * The core's first HTTPS connection fails without it, so [init] runs once per process,
 * before anything else touches the core.
 */
object Tls {
    init {
        // The same library UniFFI loads; loading it twice gives the same handle.
        System.loadLibrary("kweb_core")
    }

    /** Implemented in core/src/android.rs. Throws a RuntimeException if the core refuses. */
    @JvmStatic
    external fun init(context: Context)
}
