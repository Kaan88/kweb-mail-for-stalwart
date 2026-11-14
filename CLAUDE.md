# kweb mail for Stalwart: notes for a new session

Read this first, then [PLAN.md](PLAN.md) (decisions, architecture, milestones) and [dev/README.md](dev/README.md) (the local test server and emulator). This file says where the project stands and how to work in it. It does not repeat the plan.

## What this is

A native Android mail client for Stalwart 1.1+, JMAP only. Rust core (`core/`, crate `kweb-core`) with a Kotlin/Compose UI (`android/`) joined by UniFFI 0.32 (proc-macro mode, blocking functions on an owned tokio runtime, callback interfaces for the secret store and the push listener). `cli/` is a desktop developer tool that drives the core. The Kotlin side never talks to the server.

## Where it stands

- M0 to M2 are built and pass CI. On the real phone: sign-in with a passkey, live notifications, Mark read and Archive from a notification, and the settings all work.
- **Still unverified on a real phone (M2 table in PLAN.md):** a notification going away when the mail is read elsewhere, a swiped notification staying away, overnight idle, and calendar reminders (never tried against a real server). Restart without opening the app works (owner, 2026-11-14). The owner tests these; keep them in mind and do not claim M2 is done.
- On the emulator against the local test server (`dev/README.md`): sign-in with the app link, live notification within 5 s, removal on read, archive or delete elsewhere, swipe staying away, and the offline-then-reconnect case all passed. The emulator needs KVM and a VirtualBox VM of the owner's must not be running at the same time (see `dev/README.md`).
- **M3 (reading) is built (2026-11-14) and passes the tests and the emulator check, but is not committed and not tried on the real phone.** PLAN.md's M3 section says what was decided, what was checked and what was not. The core gained bodies, conversations, folder windows with "load older", search, star and move; Android gained the mail list with folder drawer, chips and swipes, the thread screen (locked-down WebView), attachments and search. The work was delegated to Sonnet agents and reviewed; the review findings that were fixed are in the session, not in a file.
- **M4 (sending) is built (2026-11-14)**: drafts, send with a 10-second Undo, reply and forward, attachments, suggestions, Settings. PLAN.md's M4 section says how it works and what was and was not checked. The rule to keep: a message must never be sent twice or lost (see `core/src/compose/flush.rs`).
- **M5 (calendar) is built (2026-11-14)**: agenda with repeating events expanded in the core (the server does not expand them), event screen, create/change/delete, invitations, tasks, the bottom bar. PLAN.md's M5 section says how it works. The rule to keep: a save patches only what the app owns and must leave the rest of an event on the server untouched.
- **M6 (contacts and away message) is built (2026-11-14)** and not checked on any device. PLAN.md's M6 section says how it works.
- **Next step:** M7 (sharing, files, UnifiedPush, the AI assistant: the owner allowed building Stalwart from source for it), then M8 and M9 (the owner's standing goal). The owner still has to try M3 to M6 on the phone.
- While the owner's VirtualBox VM runs, start the emulator with `KWEB_EMU_ACCEL=off dev/emulator.sh` (slow) and never touch the VM. `design/mockups.html` was reworked in the same session (screens marked built, planned or optional; an optional "Home overview" of tiles is proposed, not agreed).
- **Roadmap decisions (owner, 2026-11-14, all in PLAN.md):** JMAP only, Stalwart only. M7 also holds an AI assistant: the Rust core calls an OpenAI-compatible or Anthropic provider itself (Stalwart has no client AI endpoint), starting from the owner's Thunderbird extension (`ai-assistant-1.2.3.xpi`, in the owner's Downloads; its design is summarised in PLAN.md). When that work starts, build Stalwart from source with `RUSTFLAGS="--cfg stalwart_tests"` as a second local test server (that build grants Enterprise features with no licence; the owner will test a real licence later). M8 is a simplified Sieve editor (RFC 9661). M9 is optional S/MIME and OpenPGP.
- Git: M3 is committed and pushed (fc7f076, 9a6e3e5, 3c326c9). Seven files (the two Gradle files, the Ward SVGs) always show as modified: line-ending noise, never stage them; stage files by name. `.figma-import/` is the owner's, untracked, not ours.
- The owner set a standing goal (2026-11-14): go on through the milestones, decide open points yourself and write them down in PLAN.md under "Decided without asking".
- Known limits: an installed M2 build does one full resync on the first start with M3 (the cache schema changed); `http:` pictures in a mail never load (cleartext is off) although "Show" is offered; no Reply action until M4; a sign-in cannot be cancelled inside the core; a calendar alert pushed during a reconnect is lost.

## Development machine

The owner moved from Windows to Linux (CachyOS) and the repository stays on the NTFS drive (`/mnt/C/...`): slow builds, no execute bit (run `sh gradlew`), and every file may show as modified in `git status` because of CRLF line endings in the working copy. That is noise: check with `git diff --ignore-space-at-eol --stat`, and do not commit or "fix" it unasked.

Everything needed is installed under `~/tools` without root: JDK 21, a private Rust toolchain with the Android targets, the Android SDK and NDK, an emulator image, a local Stalwart server. **[dev/README.md](dev/README.md) says how to use and rebuild all of it.** In short: `source dev/env.sh`, then `dev/stalwart-run.sh`, `python3 dev/seed_mail.py`, `dev/emulator.sh`.

## Commands

Run `source dev/env.sh` first (it also moves `CARGO_TARGET_DIR` off the NTFS drive).

```
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cd android && sh gradlew assembleDebug lintDebug testDebugUnitTest -PskipNative   # Kotlin only, host-built bindings
cd android && sh gradlew assembleDebug                                            # needs NDK + cargo-ndk (installed)
```

CI is `.github/workflows/ci.yml` (core job, then android job that uploads the APK as `kweb-mail-debug-apk`).

## Conventions

- `rustfmt.toml`: `max_width = 120`, `use_small_heuristics = "Max"`. Clippy is `-D warnings`.
- Package `ovh.kweb.mail`, minSdk 29, compile and target 36. Dependency versions are pinned on purpose (lifecycle 2.10.0 because newer ones need compileSdk 37).
- Look: Material You. Take every colour from `MaterialTheme.colorScheme`; never write a colour into a screen. Rows that belong together are one group (`ui/list/Groups.kt`: large outer corners, small inner ones, no divider lines). The theme is the user's choice (`ui/theme`: wallpaper palette or a built-in one); the picker is in the folder drawer. `design/mockups-material.html` is the current design page.
- Notification rule: what is on screen equals the unread Inbox mail that arrived after sign-in and was not swiped. Android notification ids are derived from the message id and are 1000 or more; lower ids belong to the app's own entries.
- Android sign-in returns through the app link `ovh.kweb.mail:/oauth/callback` (`SignInReturnActivity`). A loopback socket does not work on a phone. The desktop CLI keeps the loopback socket.
- Tests for the core run against the mock server in `core/tests/support/`. Prefer a core test plus a real-server check over a UI-only check.

## How the owner wants to work

- Commit and push only when asked, and each separately. Commit messages are short and plain, with a one-line summary and a paragraph on why.
- Cheap subagents (Sonnet, Haiku) are welcome for work that does not need the strongest model. Do not use Opus when it can be avoided.
- Answers are short and plain: say what changed, what was checked, what was not.
- The owner's real server and mailbox are theirs. Never use them for destructive experiments; use the local test server and the emulator (see `dev/README.md`). The real phone is only for what they cannot show: delivery over hours, reboot, battery behaviour.
- Credentials: never read, print or commit secrets. A file the owner keeps outside the repository with server credentials exists, and is not needed. Do not open it unless the owner confirms it is truly necessary. Test credentials for the local server live under `~/tools/stalwart-test/etc/` outside the repository; using the `tester` login of that local server is fine.
