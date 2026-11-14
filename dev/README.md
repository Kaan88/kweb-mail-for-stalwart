# Local test environment

A Stalwart server and an Android emulator on the Linux development machine, so the app can be tried without the owner's real mailbox or phone. Nothing here is part of the app.

What it cannot replace: the real phone. Doze, the maker's battery handling, reboot and overnight behaviour, and real notification timing are checked there only (the M2 table in [PLAN.md](../PLAN.md)).

Everything lives outside the repository, under `~/tools` (`$KWEB_TOOLS` overrides it), and is installed for the user only: no root, no system packages touched. The repository holds only the scripts. The repository itself is on an NTFS drive (`/mnt/C/...`), where builds are slow and files have no execute bit, so build output goes to `~/.cache/kweb-mail/target` and Gradle is run as `sh gradlew`.

| Path under `~/tools` | What |
|---|---|
| `jdk/` | Temurin JDK 21 |
| `rust/` | a private rustup toolchain (stable) with the `aarch64-linux-android` and `x86_64-linux-android` targets, the `rustfmt` and `clippy` components (`rustup component add rustfmt clippy`), and `cargo-ndk`; the system's own `cargo` is left alone |
| `android-sdk/` | SDK command-line tools, platform 36, build-tools (Gradle fetches what it needs), NDK 28.2, emulator, platform-tools, system image Android 35 Google APIs x86_64 |
| `stalwart-test/` | Stalwart 1.1.0 (`bin/stalwart`), `stalwart-cli` (`node/`), config, data, logs, credentials (`etc/`) |
| `dl/` | the downloaded archives and install logs; can be deleted |

## Use

```bash
source dev/env.sh          # JAVA_HOME, ANDROID_HOME, the NDK, the private Rust toolchain, PATH
dev/stalwart-run.sh        # start the test server (http://127.0.0.1:18080); dev/stalwart-stop.sh stops it
python3 dev/seed_mail.py   # sample mail (see below)
dev/emulator.sh            # start the emulator without a window; `window` shows it, `stop` ends it
dev/gradle.sh assembleDebug
adb install -r android/app/build/outputs/apk/debug/app-debug.apk
```

In the emulator the app signs in to `http://127.0.0.1:18080`: `dev/emulator.sh` runs `adb reverse`, which maps the emulator's port to this machine's. (The core accepts plain `http` only for 127.0.0.1, `localhost` and `::1`, so `10.0.2.2` does not work.) Sign in as `tester@kweb.test`; the password is in `~/tools/stalwart-test/etc/tester.txt`. The Allow page then returns to the app through `ovh.kweb.mail:/oauth/callback`, as on the phone.

Screenshots without a window: `adb exec-out screencap -p > shot.png`. Log: `adb logcat -s kweb:V` (the app's tag is in `Background.TAG`).

## The test server

Stalwart 1.1.0, plain HTTP on 127.0.0.1:18080 (not 8080, which tends to be taken). Files in `~/tools/stalwart-test/`:

| Path | What |
|---|---|
| `etc/admin.txt` | `admin:<password>`, the recovery administrator, used for management with `stalwart-cli` |
| `etc/admin-account.txt` | `admin@kweb.test:<password>`, the account the set-up created |
| `etc/tester.txt` | `tester@kweb.test:<password>`, the user to sign in as |
| `etc/config.json` | the data store location, written by the first-time set-up |
| `data/`, `logs/` | the store and the log (`logs/stalwart.out` is the server's output) |

The credentials are generated, mode 600, and not in the repository.

### Setting it up from nothing

1. Download `stalwart-x86_64-unknown-linux-gnu.tar.gz` of the release `v1.1.0` from GitHub (`stalwartlabs/stalwart`) and unpack the `stalwart` binary into `stalwart-test/bin/`. `npm install -g --prefix ~/tools/stalwart-test/node stalwart-cli` gives the management tool; `source dev/env.sh` puts it on the PATH.
2. `dev/stalwart-setup.sh`. It starts the server in bootstrap mode and submits the `Bootstrap` object with `stalwart-cli` (hostname `localhost`, domain `kweb.test`, no TLS certificate, no DKIM, RocksDB under `data/`); restarts in recovery mode (`STALWART_RECOVERY_MODE=1`), where the whole management API answers to the recovery administrator; creates the `http` listener and the user; and restarts normally. It does nothing if `etc/config.json` exists. To start over, stop the server and remove `etc/config.json`, `data/*` and `etc/*account*`/`etc/tester.txt` by hand.

`stalwart-cli describe` lists every object and `describe <Object>` its fields; `get`, `query`, `create`, `update` work on them. A fresh server accepts only the `Bootstrap` object until the set-up is done.

### Sample mail

```bash
python3 dev/seed_mail.py            # 12 mails: HTML with remote images and a script, HTML only,
                                    # inline image, attachments, a four-mail thread, no subject,
                                    # a very long subject, Turkish characters
python3 dev/seed_mail.py one        # one new plain mail, to watch a notification arrive
python3 dev/seed_mail.py bulk 200   # 200 short mails, for lists and search
```

It imports into the Inbox over JMAP with the credentials in `etc/tester.txt` (`KWEB_TEST_CREDS` overrides the path) and creates an Archive folder if there is none. SMTP is not used: the server only takes unauthenticated mail on port 25 and would run its spam checks.

### The desktop CLI against it

```bash
source dev/env.sh && cargo build -p kweb-cli
export KWEB_HOME=~/tools/stalwart-test/cli-home
$CARGO_TARGET_DIR/debug/kweb sign-in http://127.0.0.1:18080    # then sign in as tester in a browser
$CARGO_TARGET_DIR/debug/kweb inbox -n 15
```

Checked against this server: sign-in, sync of the mailboxes and the Inbox (with Turkish characters), `read`, `archive` and `delete`.

## The emulator

AVD `kweb35`: Pixel 6 profile, Android 15 (API 35), Google APIs image, x86_64, 2 GB RAM, KVM-accelerated (boots in about 20 s). `/dev/kvm` must be accessible to the user. It has Chrome, which the app opens for sign-in.

Created once with:

```bash
avdmanager create avd -n kweb35 -k "system-images;android-35;google_apis;x86_64" -d pixel_6
```

(`Could not load devices from .../devices.xml` in the output is harmless.)

### VirtualBox and KVM

The emulator runs with KVM beside the owner's VirtualBox VMs (owner, 2026-11-14: the earlier conflict, `KVM: entry failed, hardware error 0x0`, is fixed). Never stop or change a VM. `KWEB_EMU_ACCEL=off dev/emulator.sh` still starts the emulator without KVM, slowly, if it is ever needed.

## Driving the app on the emulator

`adb shell input` works for taps and text; the screen is 1080x2400. Screenshots: `adb exec-out screencap -p > shot.png`, then look at the PNG. Notes from doing the sign-in by hand:

- The keyboard changes the layout, so a tap computed before it opened can land in the wrong field. In the Stalwart login page use `Tab` (`adb shell input keyevent 61`) to go from the username to the password and `Enter` (`keyevent 66`) to submit; clear a field with `adb shell input keycombination 113 29` then `keyevent 67`.
- Chrome shows a first-run page once ("Use without an account"); the notification permission dialog appears after the first sign-in (`adb shell pm grant ovh.kweb.mail android.permission.POST_NOTIFICATIONS` avoids it, but `pm clear` revokes it again).
- List what the app has on screen: `adb shell dumpsys notification --noredact | grep -E "pkg=ovh.kweb.mail|android.title=|android.text="`. Ids: 1 summary, 2 the "Connected" service entry, 1000 and up the mails. Open the shade with `adb shell cmd statusbar expand-notifications`; a swipe is `adb shell input swipe 300 804 1050 804 200` on the first notification.
- `python3 dev/mail_ops.py read|unread|archive|delete|list` acts on the newest Inbox mail as another client would; `python3 dev/seed_mail.py one` delivers a new mail.
- To cut the app off from the server for a while: `adb reverse --remove tcp:18080`, and `adb reverse tcp:18080 tcp:18080` to restore it.

### Checked on the emulator (2026-11-14, build of the app-link sign-in commit)

Sign-in with the app link (the consent page says it returns to `ovh.kweb.mail`), 12 seeded mails in the Inbox, new mail notifies within 5 s with Archive and Mark read buttons, read elsewhere removes the notification and the summary within 1 s, archive and delete elsewhere remove it, a swiped notification stays away when the next mail arrives, and with the connection cut for a while, three mails delivered and one read elsewhere, exactly the two unread mails show within 5 s of reconnecting.

Not checked: the 15-minute background check (WorkManager will not run a periodic job early), restart without opening the app, overnight, reminders, the Archive/Mark read buttons themselves (tapping them), TLS.

### Checked on the emulator for M3 (2026-11-14)

The mail list of the seeded Inbox as conversations, the folder drawer, the four-mail thread, the newsletter with its remote image hidden and then shown, the inline picture, swipe to archive with Undo, search for "invoice" answered by the server, and a prefetched mail read with the server cut off (`adb reverse --remove tcp:18080`).

Two things to know: after switching between `-PskipNative` and a full build, Kotlin may fail with "Conflicting declarations" in the generated bindings; `dev/gradle.sh clean assembleDebug` cures it. And `adb shell uiautomator dump /sdcard/u.xml` followed by `adb shell cat /sdcard/u.xml` gives the texts on screen with their positions, which beats guessing tap coordinates from a screenshot.

## Known differences from the real phone

- No battery optimisation, no maker's task killer: the foreground service is never killed by the system, so it proves the logic, not the delivery.
- The emulator's clock and time zone are UTC unless set: `adb shell service call alarm 3 s16 Europe/Istanbul` changes the zone.
- The server is plain HTTP here; the real one is HTTPS, so the TLS path (rustls with the platform verifier) is only exercised on the phone.

## What the scouting left on the test server (2026-11-14)

- Extra users: `second@kweb.test` (password in `~/tools/stalwart-test/etc/second.txt`), `filesq@kweb.test` (small quotas) and `files2@kweb.test`. The tester shares mail folders, calendars, address books and a file folder with them at different levels; `dev/notes/stalwart-sharing.md` and `stalwart-files.md` list them.
- `Sharing.allowDirectoryQueries` is on (the default is off), so users can be looked up. To put it back: `stalwart-cli update Sharing --field allowDirectoryQueries=false`, then the `ReloadSettings` action (commands in the sharing note).
- The tester has a tree of test files (Docs, Photos, Shared).

## The CLI against the test server

`KWEB_HOME=~/.cache/kweb-mail/cli-home $CARGO_TARGET_DIR/debug/kweb sign-in http://127.0.0.1:18080` prints an address; open it in a browser on this machine and sign in as the tester. After that `kweb files`, `kweb inbox` and the rest run against the real local server, which is the quickest real check of a core change.
A second home, `KWEB_HOME=~/.cache/kweb-mail/cli-home-second`, is signed in as `second@kweb.test`, the user the tester shares with: `kweb shared` there shows the shared area for real.
