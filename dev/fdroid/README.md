# F-Droid

## Decision (owner, 2026-11-17)

Both are published, with one signature. The GitHub release keeps the updater (the `github` flavor). F-Droid ships the `fdroid` flavor, which has no updater, signed with the project's key: F-Droid builds it from the tag and publishes the project's APK only if its own build is the same file apart from the signature (`Binaries:` and `AllowedAPKSigningKeys:` in the recipe). A user can then change between the GitHub and the F-Droid build without reinstalling. The price: the build must be reproducible, and a release whose build is not does not appear on F-Droid.

## Each release

Do these with the normal release (`dev/README.md`, "Release"), on the same day. F-Droid finds a new `vX.Y.Z` tag by itself, usually within a day, and its build fails if the signed APK of step 4 is not there yet.

1. Add `fastlane/metadata/android/en-US/changelogs/<versionCode>.txt` (a few lines, what F-Droid shows as "What's new") before tagging. The store text in the same folder is read from the released tag, so a change to it shows with the next release.
2. Start the CI workflow by hand on the release commit: `gh workflow run CI --ref main`. Two jobs matter:
   - `reproducible` builds the `fdroid` flavor twice on Linux (two folders, Java 21 and 17), fails if the two differ and uploads the unsigned APK as `kweb-mail-fdroid-apk`.
   - `fdroid` runs F-Droid's own tool on the recipe here and prints the hash of what it built. The two hashes must be equal. It takes the version from the Gradle file, so the recipe here needs no edit per release.
3. Download that APK (`gh run download <run> -n kweb-mail-fdroid-apk`) and sign it here with the release key; the command is in `dev/README.md`, "Release". It must be the Linux build: a build on Windows differs in the two Rust libraries (everything else is identical), so an APK built on this PC would never match F-Droid's. It must be signed with `--alignment-preserved true`, or the signer changes the file and the comparison fails.
4. Make a GitHub **pre-release** with the tag `vX.Y.Z-fdroid` on the same commit and attach the signed APK as `kweb-mail-fdroid-X.Y.Z.apk` (`gh release create vX.Y.Z-fdroid <apk> --prerelease --latest=false`). Never attach it to the normal release: the app's updater takes the first `.apk` of the latest release (`core/src/update.rs`), and installs of 0.2.1 to 0.2.3 would fetch the build without the updater. Pre-releases are skipped by the updater and by F-Droid's tag check.
5. Nothing to do on fdroiddata for an ordinary release: its update check (`AutoUpdateMode: Version`, `UpdateCheckMode: Tags`) adds the new version by copying the last build entry. Look at the app's page on f-droid.org a few days later; if the version is missing, the build log linked from https://monitor.f-droid.org says why.

## When the recipe on fdroiddata must change

The recipe that counts is `metadata/ovh.kweb.mail.yml` in https://gitlab.com/fdroid/fdroiddata. F-Droid copies the last build entry for each new version, so anything in that entry that no longer fits the source makes the next build fail or differ. Change it with a merge request from the fork `Kaan88/fdroiddata` (the owner's GitLab token for that expires on 2027-11-17) **before** tagging the release that needs it, and change the copy here (`ovh.kweb.mail.yml`) in the same step so CI tests the same thing:

- **Rust** (`rust-toolchain.toml`): `--default-toolchain` in the recipe.
- **The NDK** (`ndkVersion` in `android/app/build.gradle.kts`): `ndk:` in the recipe, by its full version number, and the two `sdkmanager "ndk;..."` lines in `.github/workflows/ci.yml`.
- **cargo-ndk**: `--version` in the recipe and in `.github/workflows/ci.yml`.
- **New Rust targets or system packages** the build needs: `--target` and `sudo:` in the recipe.
- **The flavor name, the `android/app` folder, the tag format `vX.Y.Z`, the address of the pre-release APK or the signing key**: `gradle:`, `subdir:`, `UpdateCheckMode`, `Binaries`, `AllowedAPKSigningKeys`.

Things that break F-Droid without touching the recipe:

- **Renaming `appVersionCode` or `appVersionName`** in `android/app/build.gradle.kts`: F-Droid's update check reads them by name (`UpdateCheckData`).
- **A new Gradle version** (the wrapper, usually from a dependency update): F-Droid's Gradle launcher has a list of versions it knows, and a very new one may be missing for a while. Not checked for any version but 9.9.1.
- **A new version of `rustls-platform-verifier-android`** in `Cargo.lock`: the build stops until the copied Kotlin file is renewed (`android/third_party/rustls-platform-verifier/README.md`).
- **Any pre-built file** (an AAR or JAR in the repository, a Maven repository other than Google's and Maven Central): F-Droid's scanner refuses the build.
- **Anything that makes the build depend on the machine**: a timestamp, an absolute path, an unpinned tool. The `reproducible` job is the check; run it before tagging when the build setup changed.

If the two hashes of step 2 differ, or F-Droid's build does not match the signed APK: unpack both APKs and compare (`diffoscope`, or `unzip` and `diff -r`); so far the only part that ever differed was `lib/*/libkweb_core.so`, between a Windows and a Linux build. Fix the cause and release again with a new version; a published version cannot be repaired on F-Droid.

The copy of the recipe here differs from the one on fdroiddata in three ways, on purpose: the commit is a tag (CI puts the commit in), Rust is downloaded instead of taken from F-Droid's `rustup` source library (`$$rustup$$/rustup-init.sh`), and there is no `UpdateCheckData`.

## How it works

- `./gradlew assembleFdroidRelease` (in `android/`) builds the F-Droid variant: the product flavor `fdroid` is the release build without the updater (no `REQUEST_INSTALL_PACKAGES`, no `UpdateInstallActivity`, no `UpdateActionReceiver`, no weekly check, no update rows in About; `android/app/src/fdroid/AndroidManifest.xml`, `BuildConfig.UPDATER`). It is signed with the project's key where the keystore is present and unsigned elsewhere (F-Droid's server). Output: `android/app/build/outputs/apk/fdroid/release/`. There is no debug variant of this flavor; `assembleRelease` builds both flavors.
- What makes the build reproducible: no signing blob in the APK (`dependenciesInfo`), the Rust core built with `--locked` and with build paths and the build id removed (`CargoNdkTask`), pinned Rust, NDK and cargo-ndk, and R8, which is deterministic.
- The Gradle build looks for cargo in `~/.cargo/bin` when it is not on the PATH (`Cargo` in `android/app/build.gradle.kts`): F-Droid's tool does not pass the recipe's PATH on to Gradle.

## Checked and open

Checked (2026-11-18, for 0.2.4): two builds at two paths on Windows are identical; two builds on Linux in two folders with Java 21 and 17 are identical; the build made by F-Droid's tool (fdroidserver 2.4.5, on a GitHub runner, with no Rust on its PATH) is identical to those; the shrunk `github` release build signs in on the emulator and refuses an expired certificate. The merge request is https://gitlab.com/fdroid/fdroiddata/-/merge_requests/57860 (from the fork `Kaan88/fdroiddata`, branch `ovh.kweb.mail`). Its pipeline ran F-Droid's own build image: the build succeeded, the published APK was fetched and "compared built binary to supplied reference binary successfully"; lint, rewritemeta, checkupdates and the source scan pass.

Open: the review by F-Droid's maintainers (the fork must stay public until the request is merged). Not checked: a second release through F-Droid's update check. Not done: Gradle dependency verification (`gradle/verification-metadata.xml`); store screenshots in `fastlane/`; an ABI split; the classes of the updater are still in the F-Droid APK, only unreachable and without their manifest entries.
