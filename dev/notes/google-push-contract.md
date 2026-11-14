# Push through Google: the contract (2026-11-14)

For the two builders (core, Android). The decisions are the owner's and are not repeated here; shapes, extras and timings are in `stalwart-fcm-experiment.md`. The specification of every core function is its doc comment in `core/src/ffi/webpush.rs`; this note says who builds what, in which order things happen, and the names both sides use. Nothing in this note was run: the core functions are placeholders.

## Core functions (Kotlin names in brackets)

```
fn web_push_supported(&self) -> Result<bool>                                   [webPushSupported]
fn web_push_prepare(&self) -> Result<Option<WebPushRegistration>>              [webPushPrepare]
fn web_push_subscribe(&self, token: String) -> Result<WebPushState>            [webPushSubscribe]
fn web_push_receive(&self, body: Vec<u8>, shade: Shade) -> Result<WebPushMessage>  [webPushReceive]
fn web_push_renew(&self) -> Result<WebPushState>                               [webPushRenew]
fn web_push_drop(&self) -> Result<()>                                          [webPushDrop]
fn web_push_forget(&self) -> Result<()>                                        [webPushForget]
fn web_push_state(&self) -> WebPushState            (does not block)           [webPushState]
fn work_waiting(&self) -> Result<WorkWaiting>       (cache only)               [workWaiting]

struct WebPushRegistration { sender: String, subtype: String }
enum WebPushState   { Off, Verifying { since }, Registered { expires, last_message_at }, KeyChanged, Gone }
enum WebPushMessage { Verified, Checked { plan }, Reminder { reminder }, Ignored, Undecryptable }
enum WorkWaiting    { Nothing, Due { in_ms } }
```

Until the core is built: `web_push_drop` and `web_push_forget` succeed, `web_push_state` is `Off`, `work_waiting` is `Nothing`, the rest fail with `Invalid` ("push through Google is not built yet"). The Android builder can write against the generated bindings today.

## Who builds what

**Core** (new files unless marked):
- `core/src/webpush/`: the key pair and auth secret (secret store), RFC 8291 decryption (`p256` with `ecdh`, `hkdf`, `aes-gcm`; `sha2`, `base64`, `getrandom` are there), the state record (kv `web_push`), reading a decrypted message.
- `core/src/jmap/pushsub.rs`: `PushSubscription/get` and `/set` (create, update `expires`, update `verificationCode`, destroy), and reading `applicationServerKey` from a session fetched anew.
- `core/src/ffi/webpush.rs`: replace the bodies, keep signatures and doc comments.
- The store query behind `work_waiting` (a new file under `core/src/store/`).
- Small hunks in files the other team changed: `core/Cargo.toml` (three dependency lines), `core/src/lib.rs` (`pub mod webpush;`), `core/src/jmap/mod.rs` (`mod pushsub;`), `core/src/ffi.rs` `sign_out` (one line: forget the push keys, next to `assistant::forget_key`), `core/tests/support/mod.rs` (one `mod` line for a mock of the PushSubscription calls).
- Tests (`core/tests/webpush.rs`): the RFC 8291 appendix A vector; padding; a body for other keys is `Undecryptable`; no key, token, code or body in any error or `Debug`; subscribe destroys stale ones by `deviceClientId`; the one-minute gap; verification written back; an EmailPush runs the pass and returns the plan `check_once` would; drop then forget; sign-out removes the secrets; each case of `work_waiting`. Then once against the local Stalwart with the CLI (a create with a made-up token is accepted by the server; destroy it afterwards).

**Android** (new files unless marked), package `ovh.kweb.mail.background` unless said:
- `DeliveryMode.kt`: `enum DeliveryMode { Connection, Google }` and the pure rules (who needs the service, when to register again, which reason).
- `GoogleRegistration.kt`: the register and unregister intents and reading the reply.
- `PushRegistrationReceiver.kt`, `PushMessageReceiver.kt`: the two exported receivers.
- `GooglePush.kt`: the sequences below (switch on, switch off, fall back, app start, the renew step).
- `LiveWindow.kt`: the live connection of a Google-mode account while the app is on screen.
- `ui/delivery/`: a new file for the mode group and its dialog; `res/values/strings_push.xml` (names start with `push_`).
- Small hunks: `AndroidManifest.xml`, `Settings.kt` (the keys below), `PushService.kt` (gate in `start` and `begin`, and `onDestroy` stops only connection-mode accounts), `CheckWorker.kt` (the renew step, the due-work step), `CheckScheduler.kt` (`checkAt`), `Background.kt` (`onAppBackground`, the gates), `MainActivity.kt` (`onStop`), `Accounts.kt` (`remove`), `AccountSession.kt` / `ServiceEntryRules.kt` (the summary), `SettingsScreen.kt` and `SettingsViewModel.kt` (one row), `DeliveryScreen.kt` and `DeliveryViewModel.kt`.

## Android names

Settings keys (per account, in that account's settings file):

| Key | Value |
|---|---|
| `delivery_mode` | `connection` (default, also when absent) or `google`. Written to `google` only after `Verified`. |
| `push_setup` | true while a switch-on or a re-registration is under way |
| `push_subtype` | the subtype from `webPushPrepare`; how a message finds its account |
| `push_token`, `push_token_at` | the token Play services gave, and when (ms) |
| `push_app_version` | the app's version code at that registration |
| `push_tries` | verification tries used (0 to 3) |
| `push_fallback`, `push_fallback_at`, `push_fallback_detail` | why the account went back to the connection, when, and the error name from Play services if any |

Fallback reasons (`push_fallback`): `no_play_services`, `no_server_key`, `registration_refused`, `not_verified`, `subscription_gone`. The subscription id and expiry are the core's (kv `web_push`); the keys are in the account's secret store (`web_push_private_key`, `web_push_auth`).

Registration: action `com.google.android.c2dm.intent.REGISTER`, package `com.google.android.gms`, `startService` in try/catch. Extras: `app` (an immutable empty PendingIntent), `sender`, `subscription`, `X-subscription` (all the `sender` of the core), `subtype`, `X-subtype` (the `subtype` of the core), `scope` = `GCM`, `kid` = `|ID|<AccountRecord.id>|`. Unregister: the same plus `delete` = `1`. The reply `registration_id` is `|ID|<AccountRecord.id>|:<token>`; a token equal to the package name is the answer to an unregister; an `error` extra is a refusal.

Manifest: `uses-permission com.google.android.c2dm.permission.RECEIVE`; `<queries>` with package `com.google.android.gms`; `.background.PushMessageReceiver` (action `com.google.android.c2dm.intent.RECEIVE`) and `.background.PushRegistrationReceiver` (action `com.google.android.c2dm.intent.REGISTRATION`), both exported with `android:permission="com.google.android.c2dm.permission.SEND"`.

Work names: `kweb.check.now`, `kweb.check.periodic`, `kweb.check.after-boot` (existing, unchanged); `kweb.check.due` (new: the check for work that is due, input `notBefore` in wall-clock ms). No separate renew worker: renewing is a step of `CheckWorker`.

Timings: verification wait 90 s, three tries; registration reply wait 30 s; receiver budget 8 s of the 10; live connection stops 30 s after the app left the screen; register again when the token is missing, the version code changed or 30 days passed.

## Sequences

**Switching on** (Settings, app on screen; the connection road stays up throughout):
1. Confirmation with the warning. `push_setup` = true, `push_tries` = 0.
2. `webPushPrepare()`. `null`: stop, reason `no_server_key`.
3. Register intent. It throws or nothing answers in 30 s: `no_play_services`. An `error` extra: `registration_refused`. Otherwise store `push_subtype`, `push_token`, `push_token_at`, `push_app_version`.
4. `webPushSubscribe(token)`. `Registered`: go to 6. `Verifying`: wait.
5. `PushMessageReceiver` gets the verification, `webPushReceive` returns `Verified`: go to 6. Nothing after 90 s: `push_tries` + 1 and step 4 again; after the third: `webPushDrop()`, unregister, `webPushForget()`, reason `not_verified`.
6. `delivery_mode` = `google`, `push_setup` = false, fallback keys cleared. Only now `PushService.start()` is called so the service looks again and ends if no account needs it. The account's live connection goes on under `LiveWindow` because the app is on screen.

A stop in steps 2 to 5 leaves `delivery_mode` at `connection`, so nothing was ever interrupted; the reason is shown in the dialog and kept for the Delivery check.

**Receiving** (`PushMessageReceiver`):
1. `setResultCode(Activity.RESULT_OK)`, `goAsync()`, a thread.
2. Find the account whose `push_subtype` equals the `subtype` extra. None, or no `rawData`: drop (log the fact only).
3. `webPushReceive(rawData, Shade.Ids(notifier.activeMailIds(record)))`.
   - `Checked`: `Notifier.apply(record, plan)`, `settings.recordCheck`.
   - `Reminder`: `Notifier.showReminder(record, reminder)`.
   - `Verified`: tell `GooglePush` (step 5 above).
   - `Ignored`, `Undecryptable`: nothing.
   - an error, or no answer after 8 s: `CheckScheduler.checkSoon()`.
4. `finish()`. The core call may still be running after a timeout; that is safe (see its doc comment).

A message for an account in connection mode (during a switch, or a token not yet dead) is handled the same way.

**While the app is on screen** (`LiveWindow`, decision 4): `Background.onAppForeground` starts the live connection of every Google-mode account (`core.start(session.listener)`); a new `Background.onAppBackground` (from `MainActivity.onStop`) stops it 30 s later unless the app came back. The service owns the connections of connection-mode accounts only, `LiveWindow` those of Google-mode accounts; neither stops the other's.

**Work that is due when no connection runs** (decision 4): in `onAppBackground`, and again when the connection is stopped and at the end of every `CheckWorker` run, ask each Google-mode account `workWaiting()` and take the earliest `Due`. Then `CheckScheduler.checkAt(notBefore)`:
- up to 60 s ahead: an expedited one-off that starts now, waits inside the worker until `notBefore` and then runs the checks (WorkManager refuses a delay on expedited work, and a timer in the process does not count: a process without a service may be frozen);
- further ahead: a plain one-off with that initial delay and the network constraint.

The existing `if (core.status() !is ConnectionStatus.Live) CheckScheduler.checkSoon()` after an action stays as it is and is right for Google mode too (`status()` is `Offline` when no connection runs). Trap: a worker running under `kweb.check.due` must not enqueue that name with REPLACE, or it cancels itself.

To be tested by the Android builder on the emulator, none of it tried yet: Send with the 10-second Undo and leave the app at once (server reachable, and in flight mode until a minute later); an archive from a notification with the app closed; the same in forced idle; that the waiting expedited worker survives the app going to the background.

**Renewing** (a step of `CheckWorker`, for each Google-mode account, after its check): `webPushRenew()`. `Registered`: nothing. `Gone`: fall back, `subscription_gone`. `KeyChanged`: register again (as on app start). An error: leave it to the next run.

**App start and foreground** (`GooglePush.onAppStart`, from `KwebApp.warmUp` and `onAppForeground`): for each Google-mode account, when `push_token` is missing, `push_app_version` differs, `push_token_at` is 30 days old or the state is `KeyChanged`: `webPushPrepare()`, register, `webPushSubscribe(token)`, and the 90-second rule if it answers `Verifying`. A `push_setup` left true by a process that died is finished the same way. Because force-stop loses messages for good, `onAppForeground` keeps its pass for every account (the live connection's first pass does it).

**Switching off** (Settings):
1. `delivery_mode` = `connection`, `PushService.start()`; wait until the account's status is `Live` (the app is on screen, so Android allows the start). If the service cannot start, put the mode back and say so.
2. `webPushDrop()` (an error does not stop the rest).
3. Unregister intent (`delete` = `1`).
4. `webPushForget()`, remove the `push_*` keys.

**Falling back** (automatic): the same four steps with the reason written to `push_fallback`. In the background Android may refuse the service; then `ServiceLog` records it as today, the 15-minute check carries the account, and the next time the app is opened the service starts.

**Sign-out and removing an account** (`Accounts.remove`, before `core.signOut()`): `webPushDrop()`, unregister, then `signOut()` (which forgets the keys itself). Errors do not stop the removal.

**The service** (decision 5): `PushService.start` returns without starting when no account is in connection mode; `begin()` starts only those and ends the service when there are none. `CheckWorker`, `BootReceiver`, `onAppForeground`, `afterAccountAdded` and `afterAccountRemoved` keep calling `start` and need no gate of their own. The service entry and the summary count connection-mode accounts only; a Google-mode account shows "through Google" with its `webPushState()` (registered until, verifying, or the fallback reason), never "not connected".

## Open points for the builders

- Re-registering in the background (`KeyChanged` found by the renew step) leaves the account without push until the verification arrives; the 15-minute check covers it. Accepted for this version.
- `web_push_drop` on a sign-out without network leaves a subscription on the server until it expires (7 days at most). Play services is told, so nothing arrives.
- The exact `types` of the subscription are the live connection's three (`Email`, `Mailbox`, `CalendarAlert`); the experiment ran with five. If a pass turns out to be missed for a change the three do not cover, widen `TYPES` in `ffi/webpush.rs`.
