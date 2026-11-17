# rustls-platform-verifier (Kotlin half)

The Rust core checks TLS certificates through Android's own trust store. The Rust crate
`rustls-platform-verifier-android` calls a small Kotlin class for that. Upstream ships the class as a
pre-built AAR; F-Droid accepts no pre-built binaries, so the source is copied into the app instead.

- Source: https://github.com/rustls/rustls-platform-verifier, tag `v/0.7.1`
  (commit `252e25161a91af476cbea620e29d277592d27ec7`), crate `rustls-platform-verifier-android` 0.2.0.
- `android/rustls-platform-verifier/src/main/java/org/rustls/platformverifier/CertificateVerifier.kt`
  is `app/src/main/kotlin/org/rustls/platformverifier/CertificateVerifier.kt`, unchanged.
- `app/src/main/res/xml/network_security_config.xml` is the same file from upstream (no cleartext, except
  to the certificate authorities' revocation lists), and the app's manifest names it as the AAR's did.
- `BuildConfig.kt` beside the class is ours (TEST = false).
- Licence: MIT or Apache-2.0, both files are here.

When `rustls-platform-verifier-android` changes in `Cargo.lock`, the build stops (`vendoredRustlsVerifier`
in `app/build.gradle.kts`). Copy the three upstream files again from the commit of the new crate version
(`.cargo_vcs_info.json` in the crate names it), update this file and the version in the Gradle file, then
start a release build on the emulator and sign in: that is the proof that TLS still works.
