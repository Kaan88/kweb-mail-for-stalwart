# Google push experiment on the emulator (2026-11-14)

REPORT: the FCM experiment (2026-11-14, emulator kweb35 with Play services 24.23.35, local Stalwart 1.1.0, users tester and second)

RESULT: the direct route works end to end. The pass condition in dev/notes/stalwart-fcm.md is met: steps 1 to 4 work, and EmailPush at urgency high arrives in under a second in forced idle. StateChange (normal urgency) is held for the whole of Doze. The repository was not changed.

Times below are "host time of the JMAP call" to "receiver ran", with the emulator clock corrected by its measured 268 ms lag.

**a. Registration: a token comes back**
- The Mastodon intent (`c2dm.intent.REGISTER` via `startService`, all extra names) answered in 2.4 s the first time and about 1.2 s later, with `registration_id=|ID|acct1|:<152-char token>`.
- The reply is a plain, unordered `c2dm.intent.REGISTRATION` broadcast with that one extra.
- Variants tried:
  - old names only (`sender`, `subtype`): token.
  - new names only (`X-subscription`, `X-subtype`): `error=INVALID_PARAMETERS` on this Play services. Send all names, as Mastodon does.
  - `com.google.iid.TOKEN_REQUEST` as a broadcast: token.
  - key with `=` padding: `error=SERVICE_NOT_AVAILABLE`.
  - legacy subtype `wp:<uuid>`: token.
- Registering the same subtype again returns the same token. The token survived `adb install -r` (an app update).
- The Messenger route was not tried, because the intent route worked.
- The probe declared `<queries>` for `com.google.android.gms`; I did not test without it.

**b. Stalwart's POST to Google succeeds**
- The server logs `push-subscription.success` for the `fcm.googleapis.com/fcm/send/...` URL on every push. It does not log the status number, so I only know it was 2xx.
- It worked with `webPushContact` unset (the server's state) and after setting it to a mailto address and reloading. I could not see the header, so that the `sub` claim was really sent is not shown.

**c. PushVerification arrives, decrypts and is accepted**
- It arrived 0.34 s after the create (0.14 s for the second user), 230 bytes of `rawData`, original and delivered priority `normal`.
- Decrypted: `{"@type":"PushVerification","pushSubscriptionId":"<id>","verificationCode":"<32 chars>"}`.
- Writing the code back returned `"updated":{"<id>":null}`. After that, `PushSubscription/get` shows the code (the earlier note said it is always null).

**d. What arrives on new mail**
- **Only an EmailPush, no StateChange.** With an `emailPush` entry for the account, the server sends the EmailPush in place of the StateChange for new mail; the source confirms this (`push.rs` 373-383). The EmailPush carries the new state.
- Create shape the server accepted: `"emailPush":{"<accountId>":{"properties":["id","threadId","mailboxIds","keywords","receivedAt","subject","from","preview"],"urgency":"high"}}`. The source also accepts a `filter`; not tried.
- EmailPush plaintext (338 bytes, `rawData` 486):
  `{"@type":"EmailPush","accountId":"<acct>","emails":[{"id":"<id>","threadId":"<tid>","mailboxIds":{"<inbox>":true},"keywords":{"$hasnoattachment":true},"receivedAt":"2026-11-14T18:48:50Z","subject":"<subject>","from":[{"name":"<name>","email":"<addr>"}],"preview":"<text>"}],"state":"<state>"}`
- StateChange (from flipping `$seen` elsewhere; 75 bytes, `rawData` 230): `{"@type":"StateChange","changed":{"<acct>":{"Mailbox":"<state>","Email":"<state>"}}}`.
- Small bodies are padded: `rawData` is 86 header bytes plus the plaintext rounded up to a multiple of 128 plus a 16-byte tag. Record size 4096, one record.
- Three mails imported in one call came as two pushes: one mail at +0.11 s (742 bytes) and the other two together at +1.12 s (1766 bytes), which is the 1 s `pushThrottle`. The preview is cut at about 255 characters.

| Probe app state | EmailPush (high) | StateChange (normal) | Process |
|---|---|---|---|
| Foreground | 0.64 s | 0.22 s, 0.52 s | alive |
| Background (Home) | 0.13 s, 0.12 s | 0.43 s | alive |
| Swiped from recents | 0.20 s | 0.12 s | dead, started for the broadcast (`procAgeMs=8`) |
| `am kill` | 0.16 s | 0.15 s | dead, started (`procAgeMs` 15 and 4) |
| `am force-stop` | nothing | nothing | stays dead |
| Standby bucket restricted, screen on | 0.22 s | not tried | started |
| Background restricted (appops), screen on | 0.16 s | 0.12 s | started |

- After force-stop the server still logged success, and Play services dropped both messages. They were **not** delivered after the app was opened again (waited 15 s). The next mail after the relaunch arrived normally (1.8 s).

**e. Doze (battery unplugged, screen off, `deviceidle force-idle`, deep state IDLE, process killed before each push)**
- EmailPush at high, 5 tries in two sessions: 0.21, 0.14, 0.15, 0.15 and 0.16 s. The process was started for the broadcast each time (`Start proc ... for broadcast`, `procAgeMs` 4 to 14).
- StateChange at normal, 3 tries: none arrived while idle (process still dead after 40 to 170 s). All three arrived 1.2 to 1.9 s after the device woke and went back on power, each as its own message, 172, 132 and 92 s late.
- `deviceidle unforce` alone did not release them, because the device stayed idle.
- A high EmailPush sent while a StateChange was held did not bring the StateChange with it.
- `deviceidle unforce` and `battery reset` were run; the emulator is ACTIVE, awake and on AC.

**f. Extras and acknowledgement**
- Extras on every message:
  - `subtype` (the `wp:` string given at registration)
  - `rawData`
  - `content-encoding=aes128gcm`
  - `from` (the server's VAPID public key)
  - `google.message_id`
  - `google.sent_time` (ms)
  - `google.ttl=86400`
  - `google.original_priority` and `google.delivered_priority` (`high` for EmailPush, `normal` for the rest)
  - `google.product_id`
  - `google.c.sender.id`
- No collapse key.
- The intent flags also differ: high comes with `FLAG_RECEIVER_FOREGROUND` (0x11000010), normal without (0x1000010).
- The broadcast is ordered, with result code 0 on arrival. If the receiver leaves it, Play services logs `W GCM: broadcast intent callback: result=CANCELLED`. With `setResultCode(-1)` that line is gone. Nothing was redelivered either way.
- The Messenger acknowledgement that the UnifiedPush library sends was not tried.

**g. Second account, unregister, and the server afterwards**
- A second subtype gave a different token. A subscription for user `second` on it verified, and its mail arrived with its own `subtype` and `accountId`, while tester's arrived under the first. The subtype tells accounts apart.
- Unregister (`delete=1`) answers `registration_id=|ID|<kid>|:ovh.kweb.pushprobe`, the package name.
- It is not immediate: a mail 3 s after the unregister was still delivered. At 68 s Google answered the server with 410: `details = "HTTP POST failed", code = 410, reason = "push subscription has unsubscribed or expired."`
- Stalwart tried three times, one minute apart, then stopped. It did not remove the subscription.

**h. Receiver time and network in Doze**
- From the receiver (`goAsync`) in forced idle, started by a high push: `http://127.0.0.1:18080/.well-known/jmap` through adb reverse answered 307 in 1 to 5 ms, and `https://www.google.com/generate_204` answered 204 in about 560 ms, all three times. The local server is plain http, so the https try went to Google; loopback alone would prove little.
- A receiver holding a high push for 40 s got `ANR in ovh.kweb.pushprobe` and was killed at exactly 10 s.
- A receiver holding a normal push for 30 s with the screen on finished without complaint.
- Network from a receiver started by a normal push in Doze could not be tested, because none is delivered there.

**i. What changes how the feature must be built**
- New mail is one EmailPush with no StateChange. The notification can be built from the push body alone, and the app must take the new state from it or sync later.
- "Read elsewhere" removals and anything else on StateChange will not arrive on an idle phone until it wakes. Calendar alerts are also sent at normal urgency by the server, so expect the same; no calendar alert was sent in this experiment.
- A high push leaves 10 s in total. Set the result code to OK, show the notification from the body, and hand anything longer to WorkManager.
- Messages sent while the app is force-stopped are lost for good. The app must sync on every start.
- The subscription is created with `types` plus `emailPush`. EmailPush ignores `types`.
- An unregistered token keeps working for some seconds, and the server never drops a dead subscription. Destroy the subscription on the server first, then unregister.
- The server log shows push results only at trace level (success) and debug level (errors).

**Could not be tested on the emulator; needs the real phone**
- Real Doze overnight, as opposed to forced idle, and the maker's battery saver.
- Whether Google downgrades high-priority pushes over days. Every push here showed `delivered_priority=high`, but the probe never showed a notification, and the test ran for 15 minutes.
- A current Play services: this image has 24.23 from 2024, and the new-names-only refusal may be reversed on newer ones.
- Reboot before first unlock, token rotation over weeks, mobile data, and a public server with TLS.
- The restricted bucket and background restriction with the screen off.

**Tidy-up**
- Both push subscriptions are destroyed; `PushSubscription/get` is empty for tester and second. All registered subtypes are unregistered and the probe app is uninstalled. The kweb mail app was not touched, and the emulator is running.
- Server settings I changed and put back, each with a ReloadSettings action:
  - Tracer `jolskymeaaaa`: level went info, then debug for about 90 s, then trace limited to the three push-subscription events; now info, exclude, no events again. No Authorization header was logged in that time (counted 0).
  - `Jmap.webPushContact`: set to a mailto address for one test, now null again.
- I used the local admin login from `~/tools/stalwart-test/etc/admin.txt` with stalwart-cli for those two settings. No password was printed.
- Left behind: about 22 test mails in tester's Inbox and 3 in second's (subjects like "Push probe", "doze mail", "batch"). Today's server log holds the two FCM URLs with tokens, which are now dead.

**Files in /home/kaan/.cache/kweb-mail/pushprobe (432 KB)**
- `probe.py`: the host script (key, create, verify, logs, timing, mail, flag, list, destroy, offset).
- `state.json`: key pairs and tokens of the destroyed subscriptions, mode 600.
- `app/AndroidManifest.xml`
- `app/build.sh`: builds without Gradle (javac, d8, aapt2, apksigner).
- `app/probe.keystore`
- `app/src/ovh/kweb/pushprobe/App.java`, `Cmd.java`, `Main.java`, `Push.java`
- `app/out/pushprobe.apk`, plus build leftovers in `app/out/`
- `logs/actions.log`: host times of every action.
- `logs/timing.txt`: the merged timeline.
- `logs/decoded.txt`: every event with decrypted bodies.
- `logs/logcat-final.txt`, `logs/logcat-all.txt`
- `logs/recents.png`
