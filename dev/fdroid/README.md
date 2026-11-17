# F-Droid

## Decision (owner, 2026-11-17)

Both are published, with one signature. The GitHub release keeps the updater (the `github` flavor). F-Droid ships the `fdroid` flavor, which has no updater, signed with the project's key: F-Droid builds it from the tag and publishes the project's APK only if its own build is the same file apart from the signature (`Binaries:` and `AllowedAPKSigningKeys:` in the recipe). A user can then change between the GitHub and the F-Droid build without reinstalling. The price: the build must be reproducible, and a release whose build is not does not appear on F-Droid.

Per release, beside the normal one:

1. Start the CI workflow by hand on the release commit (`gh workflow run CI --ref main`). Its `reproducible` job builds the `fdroid` flavor twice on Linux (two folders, Java 21 and 17), fails if the two differ and uploads the unsigned APK as `kweb-mail-fdroid-apk`. Its `fdroid` job runs F-Droid's own tool on the recipe and prints the hash of what it built: the two hashes must be equal.
2. Download that APK and sign it here with the release key (`apksigner sign`, see `dev/README.md`, "Release"). It must be the Linux build: a build on Windows differs in the two Rust libraries (everything else is identical), so an APK built on this PC would never match F-Droid's.
3. Make a GitHub **pre-release** with the tag `vX.Y.Z-fdroid` on the same commit and attach the signed APK as `kweb-mail-fdroid-X.Y.Z.apk`. Never attach it to the normal release: the app's updater takes the first `.apk` of the latest release (`core/src/update.rs`), and installs of 0.2.1 to 0.2.3 would fetch the build without the updater. Pre-releases are skipped by the updater and by F-Droid's tag check.
4. Add the version to `Builds:` in the recipe on fdroiddata if F-Droid's update check has not done it.

Checked (2026-11-18, for 0.2.4): two builds at two paths on Windows are identical; two builds on Linux in two folders with Java 21 and 17 are identical; the build made by F-Droid's tool (fdroidserver 2.4.5, on a GitHub runner, with no Rust on its PATH) is identical to those; the shrunk `github` release build signs in on the emulator and refuses an expired certificate. Not checked: F-Droid's real build server (its Debian image, its own Gradle launcher, which must know the Gradle version of the wrapper) and its scanner. Open: the merge request on fdroiddata.

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
- Pinned: Rust (`rust-toolchain.toml`), the NDK (`ndkVersion`), cargo-ndk (CI and the recipe). Raise them together.
- Gradle dependency verification (`gradle/verification-metadata.xml`) is not set up.
- The classes of the updater are still in the F-Droid APK, only unreachable and without their manifest entries.
