package org.rustls.platformverifier

/**
 * Stands in for the build config of the upstream library, so that `CertificateVerifier.kt` beside it stays
 * the upstream file unchanged. TEST is false: the mock roots of upstream's own tests are never used.
 */
internal object BuildConfig {
    const val TEST = false
}
