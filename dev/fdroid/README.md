# F-Droid

- `./gradlew assembleFdroid` (in `android/`) builds the F-Droid variant: the build type `fdroid` is the release build without the updater (no `REQUEST_INSTALL_PACKAGES`, no `UpdateInstallActivity`, no `UpdateActionReceiver`, no weekly check, no update rows in About) and unsigned. Output: `android/app/build/outputs/apk/fdroid/`.
- `ovh.kweb.mail.yml` is a draft of the recipe for the fdroiddata repository. The NDK (`r28c`) and cargo-ndk (`4.1.2`) versions in it are placeholders: use the ones the release is really built with.
- The store text and changelogs are in `fastlane/metadata/android/en-US/`; add `changelogs/<versionCode>.txt` per release.

## Reproducible build

F-Droid's reproducible mode compares its own build with an APK signed by you, so the two must be the same file apart from the signature. The `release` build (with the updater) and the `fdroid` build differ, so they can never match. Two ways:

1. Publish the `fdroid` build as the GitHub release too (the updater goes away there too). Then F-Droid can ship your signature (`Binaries:` and `AllowedAPKSigningKeys:` in the recipe).
2. Keep both. F-Droid signs its build with its own key, which is not the key of the GitHub APK: users cannot switch from one to the other without reinstalling. Reproducibility is then only a property to check (two builds of the `fdroid` type are byte-identical), not something F-Droid verifies.

What is in place: no signing blob in the APK (`dependenciesInfo`), the Rust core built with `--locked` and with build paths and the build id removed (`CargoNdkTask`), shrinking (R8) is deterministic. What is not done or not checked:
- Nothing has been built twice and compared yet. Do that first (`diffoscope`), on two different checkout paths.
- Pin the NDK (`ndkVersion` in `build.gradle.kts`), the Rust version (1.97.0 was current at writing) and cargo-ndk, and use the same versions in the recipe.
- `rustls-platform-verifier` (the Kotlin half) comes as a ready-made AAR from `github.com/rustls/rustls-platform-verifier` (branch `maven-archive`, see `android/settings.gradle.kts`). F-Droid does not accept pre-built binaries: this is the main blocker. Either build it from the upstream source in the recipe (`srclibs`) or copy its few Kotlin classes into `app/src/main/kotlin/org/rustls/platformverifier/` and drop the dependency (the licence is MIT/Apache-2.0). It could not be fetched in the session that wrote this.
- Gradle dependency verification (`gradle/verification-metadata.xml`) is not set up.
- The classes of the updater are still in the F-Droid APK, only unreachable and without their manifest entries.
