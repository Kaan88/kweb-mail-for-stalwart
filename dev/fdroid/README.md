# F-Droid

## Decision (owner, 2026-11-17)

Both are published, with one signature. The GitHub release keeps the updater (the `github` flavor). F-Droid ships the `fdroid` flavor, which has no updater, signed with the project's key: F-Droid builds it from the tag and publishes the project's APK only if its own build is the same file apart from the signature (`Binaries:` and `AllowedAPKSigningKeys:` in the recipe). A user can then change between the GitHub and the F-Droid build without reinstalling. The price: the build must be reproducible, and a release whose build is not does not appear on F-Droid.

Per release, beside the normal one: build `assembleFdroidRelease` with the keystore, make a GitHub **pre-release** with the tag `vX.Y.Z-fdroid` on the same commit, and attach the APK as `kweb-mail-fdroid-X.Y.Z.apk`. Never attach it to the normal release: the app's updater takes the first `.apk` of the latest release (`core/src/update.rs`), and installs of 0.2.1 to 0.2.3 would fetch the build without the updater. Pre-releases are skipped by the updater and by F-Droid's tag check.

Done: the pre-built `rustls-platform-verifier` AAR is gone (its source is copied in, `android/third_party/rustls-platform-verifier/README.md`). Next steps, in order: (1) pin NDK, Rust and cargo-ndk and put the same versions in the recipe; (2) build `assembleFdroidRelease` twice on two paths, once on Linux, and compare with `diffoscope`; (3) run the recipe with F-Droid's own tools (`fdroid build`); (4) release 0.2.4 with the pre-release beside it; (5) open the merge request on fdroiddata with `ovh.kweb.mail.yml`. The recipe has not been run on F-Droid's build server.

## How it works

- `./gradlew assembleFdroidRelease` (in `android/`) builds the F-Droid variant: the product flavor `fdroid` is the release build without the updater (no `REQUEST_INSTALL_PACKAGES`, no `UpdateInstallActivity`, no `UpdateActionReceiver`, no weekly check, no update rows in About). It is signed with the project's key where the keystore is present and unsigned elsewhere (F-Droid's server). Output: `android/app/build/outputs/apk/fdroid/release/`. There is no debug variant of this flavor; `assembleRelease` builds both flavors.
- `ovh.kweb.mail.yml` is a draft of the recipe for the fdroiddata repository. The NDK, Rust and cargo-ndk versions in it are the pinned ones (`rust-toolchain.toml`, `ndkVersion`, CI).
- The store text and changelogs are in `fastlane/metadata/android/en-US/`; add `changelogs/<versionCode>.txt` per release.

## Reproducible build

F-Droid's reproducible mode compares its own build with an APK signed by you, so the two must be the same file apart from the signature. The `github` build (with the updater) and the `fdroid` build differ, so they can never match. Three ways:

1. Publish the `fdroid` build as the GitHub release too (the updater goes away there too). Then F-Droid can ship your signature (`Binaries:` and `AllowedAPKSigningKeys:` in the recipe).
2. Keep both. F-Droid signs its build with its own key, which is not the key of the GitHub APK: users cannot switch from one to the other without reinstalling. Reproducibility is then only a property to check (two builds of the `fdroid` flavor are byte-identical), not something F-Droid verifies.
3. (Chosen.) Keep both and share the signature: publish the `fdroid` APK, signed with the project's key, in a GitHub pre-release of its own and name it in the recipe (`Binaries:`, `AllowedAPKSigningKeys:`). F-Droid then ships the project's signature if its own build is identical, and a user can change between the GitHub and the F-Droid build without reinstalling. It needs a build that really is reproducible, and a release fails on F-Droid whenever it is not.

What is in place: no signing blob in the APK (`dependenciesInfo`), the Rust core built with `--locked` and with build paths and the build id removed (`CargoNdkTask`), shrinking (R8) is deterministic. What is not done or not checked:
- Nothing has been built twice and compared yet. Do that first (`diffoscope`), on two different checkout paths.
- Pin the NDK (`ndkVersion` in `build.gradle.kts`), the Rust version (1.97.0 was current at writing) and cargo-ndk, and use the same versions in the recipe.
- Gradle dependency verification (`gradle/verification-metadata.xml`) is not set up.
- The classes of the updater are still in the F-Droid APK, only unreachable and without their manifest entries.
