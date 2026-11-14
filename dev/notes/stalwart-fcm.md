# Push through Google without a relay: research (2026-11-14)

Nothing here was tried on a device. The owner has not decided on Google push; this is what was found when asked how a relay would be set up. The sceptic's verdict comes first, then the two reports it checked.

## Verdict

VERDICT: push without the permanent connection (checked 2026-11-14)

Short answer: no relay is needed. Go straight from Stalwart to Google's push endpoint. I checked the deciding claims against the sources and they hold. Nothing was tried on a device or emulator, so one experiment (below) still has to settle it.

WHAT I CHECKED AND FOUND

1. The Mastodon app really does the direct route today. Confirmed.
   - github.com/mastodon/mastodon-android, HEAD 9ae49ec (2026-11-11; the clone's HEAD matches the remote).
   - `PushSubscriptionManager.java` line 118: intent `com.google.android.c2dm.intent.REGISTER`, with the server's VAPID key as `sender` (line 121) and subtype `wp:https://<domain>/#<id>` (line 127).
   - Line 222: the endpoint handed to the server is `"https://fcm.googleapis.com/fcm/send/"+session.pushToken`.
   - No relay URL is left in the Java, Gradle or XML sources, and the manifest and `mastodon/build.gradle` have no Firebase or Play services library.
   - The switch commit is 0c3cab991 (2026-06-09), "Make web push notifications go directly to FCM removing the need for relay" (GitHub API).
   - `PushNotificationReceiver.java` lines 66-73 read `subtype` and the byte array `rawData`, as reported.

2. The UnifiedPush library does the same. Confirmed.
   - codeberg.org/UnifiedPush/android-embedded_fcm_distributor, commit 2fe24cc (2026-11-04).
   - `Constants.kt` has the same endpoint and the same `c2dm.intent.REGISTER` action.
   - The relay report's `com.google.iid.TOKEN_REQUEST` is from the older 3.0.0. Use the direct report's description.
   - Not checked: the 3.1.0 version number on Maven, and the "01-2026" note in `Play-IPC.md` (a search of the two clones did not find it).

3. Stalwart's behaviour (stalwartlabs/stalwart, tag v1.1.0, commit e7cde9b, 2026-11-09). All confirmed in source.
   - `crates/services/src/state_manager/http.rs` lines 289-290: `TTL: 86400` and `Urgency` on every POST.
   - Urgency is hard-coded `Normal` for CalendarAlert (line 93), StateChange (line 166) and PushVerification (`push.rs` line 558). Only EmailPush uses the subscription's own urgency (line 192).
   - `push.rs` lines 534-562: the verification goes through the same `http_request`, so it is encrypted and VAPID-signed like any other push. It will arrive through Google like a normal message.
   - `webpush.rs` lines 139-160: the header is `vapid t=<ES256 JWT>, k=<key>`, with `aud` set to the endpoint origin, a 12-hour expiry, and `sub` only if a contact is set.
   - `ece.rs` line 39: the body is capped at 4096 bytes.
   - `push.rs` lines 461-512: any non-2xx counts as a failure. After `maxPushAttempts` the pending batch is cleared and the counter reset. Nothing removes a subscription on 404 or 410.
   - The URL check from dev/notes (https, public name) passes for `fcm.googleapis.com`. The send-time guard only blocks private addresses, so even the local test server can post to Google.

4. A live probe of Google's endpoint, with a random token, throwaway keys and a random body (nothing of the project):

   | Request | Answer |
   |---|---|
   | No Authorization header | 401 "Authorization header must be specified" |
   | Garbage JWT | 403 "invalid JWT provided" |
   | Valid VAPID JWT without `sub`, exactly Stalwart's shape | 410 "push subscription has unsubscribed or expired" |
   | Valid VAPID JWT with `sub` | 410, same text |

   So the endpoint is alive, it checks the JWT before the token, and Stalwart's header format without `sub` passes that check. The reports' open question about `sub` is very likely a non-issue. Both known senders (Mastodon server, UnifiedPush proxy) do send `sub`, so setting `webPushContact` is still the safe choice.

WHAT IN STALWART COULD BITE (none of it blocks the route)

- **The verification is sent once.** Its result is ignored and there is no retry (`push.rs` lines 542-561). If that one message is lost, the subscription stays unverified for good. The app must time out, destroy and create again.
- **One verification per account per minute.** `pushVerifyTimeout` is 1 minute and only the newest unverified subscription is verified (`push.rs` lines 247-306). Creating two quickly gives the "Too many requests" lines the notes saw. Create one, then wait.
- **New mail and everything else ride at different urgency.** StateChange and calendar alerts go out at normal urgency, which Google may hold back on an idle phone. Only EmailPush can be `high`. So "read elsewhere" removals and calendar reminders can be late in Doze, and calendar reminders are better scheduled locally with AlarmManager from synced data.
- **A dead token is never dropped by the server.** Google answers 410; Stalwart retries 3 times, drops the batch, and tries again on every later change until the 7-day expiry. That is harmless noise, but the app must destroy old subscriptions itself (15 per account at most).
- **A server without `webPushKey` sends no Authorization**, and Google answers 401. The app must check that the session has `applicationServerKey` and otherwise keep the foreground service.
- **Payload size is fine.** Stalwart caps at 4096 bytes and truncates EmailPush to fit. Google's limit for this route was not found; 4096 is from memory.

WHAT TO DO, IN ORDER

1. Run the experiment below before designing anything.
2. If it passes, build the direct route:
   - manifest permission and two receivers;
   - the register intent with `applicationServerKey`;
   - a P-256 key pair and auth secret per account;
   - RFC 8291 decryption in the core (the `p256`, `hkdf` and `aes-gcm` crates);
   - `PushSubscription/set` with an `emailPush` entry at urgency `high`, then the verification code written back;
   - the new-mail notification shown from the EmailPush body itself, so no network is needed in the 10-second broadcast window.
3. Housekeeping:
   - renew before the 7 days (WorkManager);
   - re-register on app start, on a version change and about every 30 days, as Mastodon does;
   - on a new token or a changed server key, destroy and create (the URL cannot be updated);
   - carry the account in the subtype.
4. Keep the foreground service as the fallback and as a user choice: no Play services, no VAPID key on the server, registration refused, or the user does not want Google in the path.
5. Build a relay only if the experiment fails at Google's side. It is not expected to.

COST

- **Money and hosting:** none. No Firebase project, no Google library in the APK.
- **Work:** one new piece in the core (decryption and subscription lifecycle) and one on Android (registration and receivers).
- **Privacy:** Google sees the timing, size and sending server of each signal, not the content. The user should be told and should be able to choose.

WHAT COULD GO WRONG

- **The device-side registration is not a documented Google API.** It is the path Chrome uses, and Mastodon has shipped it for five months. Google could change it, which is why the fallback stays.
- **Whether `Urgency: high` wakes a dozing phone for a native app is not established.** Mastodon's server sends `normal` (`app/workers/web/push_notification_worker.rb` line 10), so its app proves nothing here. This is the real open risk for "mail arrives promptly overnight".
- **Other unknowns:** behaviour after force-stop, on vendor battery savers, and whether Play services rotates the token on its own.

THE ONE EXPERIMENT

No public host is needed: the local Stalwart can reach Google.

- **Where:** the existing emulator `kweb35` (Google APIs image, which has Play services) against the local Stalwart. Afterwards, the Doze part on the real phone.
- **What to build:** a throwaway debug screen or tiny test app that
  1. reads `applicationServerKey` from the session;
  2. sends the register intent as Mastodon does and logs the `registration_id`;
  3. creates the PushSubscription with keys and `emailPush` at urgency `high`;
  4. logs every `c2dm.intent.RECEIVE` broadcast (extras, `rawData` length) and decrypts it.
- **What to look for:**
  1. A token comes back, not `SERVICE_NOT_AVAILABLE`.
  2. The Stalwart log shows `push-subscription.success` for the `fcm.googleapis.com` URL. A 401 or 403 means the VAPID header is the problem; a 410 means the token is.
  3. The PushVerification arrives in `rawData`, decrypts, and the written-back code is accepted.
  4. Seeding a mail produces an EmailPush (note its wire shape) and a StateChange.
  5. With the app swiped away, then with `adb shell dumpsys deviceidle force-idle`, measure how long EmailPush (`high`) and StateChange (`normal`) each take.
- **Pass:** steps 1 to 4 work, and EmailPush arrives within seconds in forced idle.
- **If only step 5 fails for normal urgency:** the route is still good for new mail, and removals and calendar alerts need the local workarounds above.

NOTE ON THE RESTART

The earlier agents' clones are still in the scratchpad and were used for this check: the session scratchpad (gone)/mastodon, the session scratchpad (gone)/efcm and the session scratchpad (gone)/relay-src. The folder the session scratchpad (gone)/stalwart-src is empty; the Stalwart source is under relay-src/stalwart. The repository was not changed. My probe script is the session scratchpad (gone)/verdict/probe.py.

## Report: the direct route

# Web Push straight from Stalwart to a native Android app through FCM, no relay

## Answer: works, with caveats

A native app can register with Google Play services using the server's VAPID public key as the sender, hand the server `https://fcm.googleapis.com/fcm/send/<token>`, and receive the encrypted body in a broadcast. No relay, no Firebase project, no `google-services.json`, no Firebase or Play services library is needed. The official Mastodon app has shipped exactly this since June 2026, and UnifiedPush publishes a library that does the same.

The caveats:
- The device-side registration is not a documented Google API. It is the path Chrome uses, reverse-engineered.
- It only works on devices with Play services.
- **Stalwart sends StateChange and CalendarAlert with `Urgency: normal`.** Only the EmailPush extension can be set to `high`, so only that is likely to get through Doze promptly.

Nothing was tried on a device, and nothing about this project was sent anywhere.

## The mechanism (Mastodon Android, commit 9ae49ec, 2026-11-11)

Repository: github.com/mastodon/mastodon-android. The switch was commit 0c3cab991 (2026-06-09), "Make web push notifications go directly to FCM removing the need for relay".

**Registration** (`mastodon/src/main/java/org/joinmastodon/android/api/PushSubscriptionManager.java`, lines 118-136):
```java
Intent intent=new Intent("com.google.android.c2dm.intent.REGISTER");
intent.setPackage("com.google.android.gms");
intent.putExtra("app", PendingIntent.getBroadcast(context, 0, new Intent(), PendingIntent.FLAG_IMMUTABLE));
String subtype="wp:https://"+session.domain+"/#"+session.pushAccountID;   // random UUID per account
intent.putExtra("sender", sender);          // server VAPID public key
intent.putExtra("subscription", sender);
intent.putExtra("X-subscription", sender);
intent.putExtra("subtype", subtype);
intent.putExtra("X-subtype", subtype);
intent.putExtra("scope", "GCM");
intent.putExtra("kid", "|ID|"+accountID+"|");
context.startService(intent);               // wrapped in try/catch (commit 8475fd7dd, degoogled devices)
```
- The sender must be base64url with **no `=` padding**; otherwise the reply is `error=SERVICE_NOT_AVAILABLE` (the app's author, github.com/mastodon/webpush-fcm-relay/issues/9, 2026-06-08). Stalwart's `applicationServerKey` is already unpadded (our notes).
- Unregistering is the same intent with `delete=1` (lines 142-166).

**Token reply** (`RegistrationReceiver`, lines 441-486):
- It arrives as broadcast `com.google.android.c2dm.intent.REGISTRATION` with extra `registration_id` = `<kid>:<token>`. The `kid` echo is how the app matches the reply to an account.
- On unregister the token equals the package name.

**Endpoint given to the server** (line 222): `"https://fcm.googleapis.com/fcm/send/"+session.pushToken`, together with the app's own P-256 public key (65-byte uncompressed, base64url) and a 16-byte auth secret, both generated in the app (lines 197-210).

**Manifest** (`mastodon/src/main/AndroidManifest.xml`, lines 10-15 and 99-109):
- `uses-permission com.google.android.c2dm.permission.RECEIVE`.
- Two exported receivers guarded by `android:permission="com.google.android.c2dm.permission.SEND"`, one for `...c2dm.intent.RECEIVE` and one for `...c2dm.intent.REGISTRATION`.
- `mastodon/build.gradle` has no Firebase or `com.google.android.gms` dependency.

**Receiving** (`PushNotificationReceiver.java`, lines 65-127):
- Extra `subtype` is the `wp:...` string and selects the account.
- Extra `rawData` (byte[]) is the POST body.
- For aes128gcm there are no other extras: salt (16), record size (4), key id length (1) and the server key are parsed from the body header.
- Extras `encryption` and `crypto-key` appear only for the old aesgcm draft.

**Decryption** (`PushSubscriptionManager.java`, lines 356-408): RFC 8291 aes128gcm, done in plain Java (ECDH, HMAC-SHA256 HKDF with "WebPush: info", AES-128-GCM, trailing 0x02 padding byte), single record only. This matches the steps in our notes, so the Rust core can do the same.

**Token change:**
- There is no token-refresh callback on this route.
- Mastodon re-registers on every app start when the token is missing, the app version changed, or 30 days have passed (`TOKEN_REFRESH_INTERVAL`, lines 62 and 100; called from `MastodonApp.java:33`), then re-creates the server subscription.
- For Stalwart the subscription URL is immutable (our notes), so a new token means destroy and create.

**Wake-up when not running:** the message is a manifest-declared broadcast from Play services, so the process is started for it. A force-stopped app gets nothing, as with any FCM app (from memory, unverified).

## Is it supported by Google?

- **Server side: documented.** Chrome's "Web Push Interoperability Wins" (developer.chrome.com/blog/web-push-interop-wins, undated) says "FCM / GCM and Chrome now support the standard Web Push Protocol" with VAPID, and shows the endpoint `https://fcm.googleapis.com/fcm/send/ABCD1234`.
- **2024 legacy shutdown:** a second-hand statement in firebase-talk (groups.google.com/g/firebase-talk/c/3C2Vq9pIWr4, 2024-06-04) says "the Web Push endpoints will not be deprecated". I found no first-hand Google statement.
- **Device side: tolerated, not documented.** The only write-up is a gist by microG's author (gist.github.com/mar-v-in/2a054e3a4c0a508656549fc7d0aaeb74, section WebPush).
- **It is Chrome's own path.** Chromium's `components/gcm_driver/android/java/src/org/chromium/components/gcm_driver/GoogleCloudMessagingV2.java` uses the same `c2dm.intent.REGISTER` with `sender`, `subscription` and `subtype`; its header comment calls it "Temporary code for sending subtypes". Breaking it would break web push in Chrome on Android.
- **UnifiedPush's maintainer** calls it "less known, and not (well?) documented by Google" (unifiedpush.org/news/20250131_push_for_decentralized). He judges the chance of the intent route being removed in favour of the newer Messenger IPC as "very low", and implemented the Messenger route in his library anyway (same relay issue, 2026-06-09).
- **Play Store or Google terms:** not found; I found nothing for or against.

## Does it still work?

- Mastodon's push file was last touched 2026-11-04 (ba290a057, a re-registration fix), with other fixes in June and July 2026. All are app-side bugs, not Google breakage.
- UnifiedPush's notes (codeberg.org/UnifiedPush/fcm-distributor, `Play-IPC.md`) record that registration "fails without sender at this moment, 01-2026", so both old and new extra names must be sent. The same notes say `wp:<uuid>` is a legacy format and Chrome now uses `wp:https://URL/#UUIDv4`, which is what Mastodon sends.
- The UnifiedPush tracker for the library has two issues, neither about delivery. A search of Mastodon's tracker for push problems since June 2026 found one unrelated feature request.

## The ready-made alternative

`org.unifiedpush.android:embedded-fcm-distributor` 3.1.0 (Maven Central, 2026-06-23; source codeberg.org/UnifiedPush/android-embedded_fcm_distributor, commit 2fe24cc, 2026-11-04, Apache 2.0):
- "It doesn't contain any Google proprietary blobs."
- "If your application supports VAPID, you have nothing special to do."
- It uses the same extras (`c2m/C2mRequests.kt`), the same endpoint constant (`Constants.kt`), and the Messenger IPC, and it acknowledges each message to Play services (`impl/FirebaseReceiver.kt`), which Mastodon does not do.
- Its gateway ("rewrite proxy") is only for servers that send without VAPID. Stalwart sends VAPID, so none is needed.
- Used with the UnifiedPush connector, the same code would also serve phones without Play services through ntfy or another distributor. That distributor's public server is then the only "relay", and it is the user's choice.

## What other apps do

Checked in their build files today:
- Tusky: UnifiedPush connector only, no FCM.
- Element X: UnifiedPush connector 3.3.5 plus Firebase.
- Delta Chat: `firebase-messaging` 24.1.2 in the Play flavour, with its own notification server.
- Thunderbird/K-9: no push library.
- Conversations and FairEmail: not checked.

## What Stalwart sends (source at tag v1.1.0, commit e7cde9b, 2026-11-09)

In `crates/services/src/state_manager/http.rs` and `crates/common/src/network/webpush.rs`:
- Headers: `TTL: 86400`, `Urgency: <value>`, `Content-Encoding: aes128gcm` when keys were given, and `Authorization: vapid t=<ES256 JWT>, k=<key>`.
- The JWT has `aud` set to the endpoint's origin and a 12-hour expiry. `sub` is included only if `webPushContact` is set.
- No `Topic` header, so nothing is collapsed at FCM.
- **PushVerification goes through the same function**, so it arrives encrypted over FCM like any other message. The app decrypts it and writes the code back. This removes the "needs a public https listener" obstacle from our notes.
- **Urgency is hard-coded `normal` for PushVerification, StateChange and CalendarAlert.** Only EmailPush uses the per-account `urgency` from the subscription (`high` is allowed).
- Any non-2xx answer is retried up to the attempt limit and then the pending batch is dropped. The subscription is not removed on 404 or 410.

## What each side needs

**App:**
- The manifest permission and two receivers.
- The register intent with the session's `applicationServerKey`.
- A P-256 key pair and auth secret, stored.
- RFC 8291 decryption (core or Kotlin).
- `PushSubscription/set` with the FCM URL, the keys and an `emailPush` entry with urgency `high`; then the verification code written back.
- Renewal before the 7-day expiry, which needs the app to run (WorkManager).
- Re-register on start, on version change, and about every 30 days.
- A fallback to the current foreground service when Play services is absent or registration fails.

**Server:**
- Stalwart 1.1 with a VAPID key (set by default per our notes) and outbound https to fcm.googleapis.com.
- Setting `webPushContact` is prudent; whether FCM insists on `sub` was not found.

## Risks

- **Undocumented client API.** It could change without notice; the fallback must stay.
- **Doze.** Google states that normal priority is "batched together and delivered opportunistically when the device is in Doze mode" (firebase.blog/posts/2025/04/fcm-on-android) and that high priority is attempted immediately. I found no Google statement that `Urgency: high` maps to FCM high priority for a native app. Mastodon's server sends `Urgency: normal` (`app/workers/web/push_notification_worker.rb`, line 10), so its app is no evidence for prompt delivery in Doze.
- **Calendar alerts and "read elsewhere" removals** ride on normal urgency and may be late on an idle phone.
- **Google is in the path.** It sees timing and size, not content. This is a privacy change from today's direct connection.
- **Key rotation.** The token is bound to the server's VAPID key; if the admin rotates `webPushKey`, pushes fail until the app re-registers.
- **Subscription quota.** Stalwart allows 15 subscriptions per account; stale ones must be destroyed.
- **Limits not established.** FCM's payload limit for this route (4096 bytes from memory; Stalwart caps at 4 KB), rate limits, and whether high-priority messages that show no notification get downgraded: not found.

## Unverified until tried on a device

- That FCM accepts Stalwart's VAPID header as sent, with or without `sub`, and that the status is 201.
- That PushVerification really arrives through `rawData` and decrypts.
- Whether EmailPush with urgency `high` wakes a dozing phone, and how late normal-urgency StateChange is.
- The wire shape of an EmailPush body.
- Behaviour after reboot before first unlock, after force-stop, and on vendor battery savers.
- Whether Play services ever rotates the token on its own.
- Whether microG accepts `wp:` registrations.

Sources were cloned into the session scratchpad (gone) (folders `mastodon`, `efcm`, `fossfcm`, `stalwart-src`). The repository was not changed.

## Report: relay options

REPORT: how a relay between Stalwart Web Push and FCM would be set up (question 2)

HEADLINE (changes the question)
A relay is very likely NOT needed. Google's FCM endpoint accepts standard Web Push itself, for a native Android app, as long as the request carries a VAPID Authorization header, and Stalwart always sends one. The app asks Play services for a token using the mail server's VAPID public key as the "sender"; the push URL is then https://fcm.googleapis.com/fcm/send/<token>, which Stalwart can post to directly. No Firebase project, no google-services.json, no service-account key, no host of ours, nothing to run or pay for. This is what UnifiedPush's current "embedded FCM distributor" does. It was read from source but NOT tried here (no device run, no post from a Stalwart to FCM). A relay is only needed for a server that has no VAPID key, or if the direct route fails a real test.

Sources for the headline
- UnifiedPush common-proxies README (github.com/UnifiedPush/common-proxies, commit 3a62b28, 2025-01-29): "Webpush messages can be directly send to FCM servers (without this proxy) if the request contains a VAPID authorization."
- UnifiedPush android-embedded_fcm_distributor (commit 43c6786, 2025-05-26, library 3.0.0), EmbeddedDistributorReceiver.kt: "Google FCM servers can handle webpush requests out of the box, but they require a VAPID authorization. If your application supports VAPID, you have nothing special to do." Constants.kt: ENDPOINT = "https://fcm.googleapis.com/fcm/send/%s". Utils.kt registerFCM: broadcast "com.google.iid.TOKEN_REQUEST" to package com.google.android.gms with extras scope=GCM, sender/subscription/X-subscription = the VAPID public key, subtype/X-subtype = "wp:<id>", kid, app = a PendingIntent. FirebaseReceiver.kt: token arrives in "com.google.android.c2dm.intent.REGISTRATION" (extra registration_id), messages in "com.google.android.c2dm.intent.RECEIVE" with the raw POST body in the byte-array extra "rawData". build.gradle has no Firebase or Play services dependency; https://unifiedpush.org/kdoc/embedded_fcm_distributor/ says the library "doesn't contain any Google proprietary blobs".
- Origin of the method: microG author's gist https://gist.github.com/mar-v-in/2a054e3a4c0a508656549fc7d0aaeb74 (last active 2025-12-25), WebPush section. It is not a documented Google API for Android apps (it is the mechanism browsers use). Google could change it: that is the main risk.
- Stalwart source (github.com/stalwartlabs/stalwart, commit e7cde9b, 2026-11-09):
  - crates/services/src/state_manager/http.rs http_request: POST to the subscription url with headers "TTL: 86400", "Urgency: <value>", Authorization (VAPID, when a key is configured), and when keys were given "Content-Encoding: aes128gcm", Content-Type application/octet-stream; without keys the body is plain application/json. Any 2xx is success; any other status is a failure that is retried. StateChange, CalendarAlert and PushVerification go with Urgency normal; EmailPush uses the urgency the client set in emailPush.
  - crates/common/src/network/webpush.rs: header is "vapid t=<ES256 JWT>, k=<public key>", claims aud = origin of the endpoint, exp = now + 12 h, sub only if a contact is set. This answers the open "k part" question in dev/notes/stalwart-webpush.md.
  - crates/services/src/state_manager/ece.rs: WEBPUSH_MAX_BODY_SIZE = 4096, one record of 4096.
  - push.rs: after maxPushAttempts the pending items are dropped and the counter reset; there is no code that deletes a subscription on 404 or 410. So "answer 410 so the server drops the subscription" does NOT work with Stalwart: a dead subscription keeps being tried until it expires (7 days at most) or the app destroys it.
  - "vapid" is an Option in the server: a Stalwart with no webPushKey sends no Authorization header, and FCM would refuse it.

OPTIONS

| Option | What it is | Input side | Needs | Who runs it | Cost | What it can see |
|---|---|---|---|---|---|---|
| 0. Direct to FCM (no relay) | App registers with Play services using the server's applicationServerKey; Stalwart posts to fcm.googleapis.com/fcm/send/<token> | RFC 8030 + aes128gcm + VAPID unchanged | Play services on the phone; a VAPID key on the Stalwart (ours had one). No Firebase project | Nobody | 0 | Google: token, timing, size, the mail server's IP and VAPID key. Body stays encrypted |
| 1. UnifiedPush common-proxies "webpushfcm" (/wpfcm?t=<token>) | Go rewrite proxy: copies body, TTL, Urgency, Content-Encoding, replaces Authorization with its own VAPID, posts to fcm.googleapis.com/fcm/send/<token>, returns FCM's status | RFC 8030 posts unchanged; no client registration at the proxy; the app must register at Play services with the PROXY's VAPID key | Public https host, one VAPID private key file (common-proxies -vapid). No Firebase project, no database | App maker (UnifiedPush runs a public one at fcm.distributor.unifiedpush.org and asks "Please host your own gateway if possible") | Small VM or container | Token, timing, size, sender IP. Body untouched (rewrite/wp_fcm.go) |
| 2. common-proxies "/FCM" (old) | Same proxy, FCM HTTP v1 with a service-account key; body base64 in data "b", split in two messages above 3800 chars | Any POST with ?token=&instance=&v2 | Firebase project + service-account JSON, Firebase library in the app | App maker | VM | Same. Marked deprecated in rewrite/fcm.go ("please use wp_fcm instead") |
| 3. mastodon/webpush-fcm-relay (commit 1cbc6e9, 2026-07-28) | Go service the official Mastodon apps use | POST /relay-to/fcm/<device token>[/extra]; accepts Content-Encoding aesgcm (needs Crypto-Key and Encryption headers) and aes128gcm (sets data "rfc"="1"); TTL, Topic, Urgency read; VAPID ignored; answers 201 at once and sends from a queue | Firebase project + credentials file; no database; pulls in Datadog tracing; always adds a visible Notification title (a trumpet emoji) meant for iOS | App maker | VM | Token, timing, size. Body untouched, re-encoded Z85 in data "p". On NotRegistered it calls an "Unsubscribe-URL" header that only Mastodon sends |
| 4. DagAgren/toot-relay (commit eaded0e, 2025-03-01) | Same idea for Apple APNs | /relay-to/<environment>/<device token>; only aesgcm enabled | Apple push certificate | App maker | VM | Not usable for Android |
| 5. Sygnal (matrix-org/sygnal 4700184, 2025-07-21; now element-hq/sygnal) | Matrix push gateway | Matrix push gateway API (JSON), NOT Web Push | Firebase service account (api_version v1) | App maker | VM | Does not fit: it would need a second converter in front, and Matrix payloads are not end-to-end encrypted by it |
| 6. ntfy with Firebase | Self-hosted ntfy forwarding to FCM topics | Plain POST to a topic; no VAPID needed | docs.ntfy.sh/config: FCM "only works if you modify and build your own Android .apk"; firebase key file | Each user, plus the ntfy app | VM | Only useful as a UnifiedPush distributor (the planned optional route), not as our FCM relay. server_firebase.go caps at 4000 bytes and sends only a "poll_request" when the topic is not world-readable |
| 7. Own minimal relay | See below | RFC 8030 unchanged | Firebase project, service-account key as a secret, public https | App maker | Free tier | Token, timing, size, sender IP |

Not checked: Mozilla autopush's FCM bridge (from memory it needs client registration and a Firebase project; unverified).

WHO RUNS IT, AND WHY "EACH ADMIN RUNS THEIR OWN" FAILS FOR FCM HTTP v1
An FCM registration token is bound to one Firebase project (the sender); a send from another project fails with SENDER_ID_MISMATCH 403 (https://firebase.google.com/docs/reference/fcm/rest/v1/ErrorCode). The project is compiled into the app through google-services.json. So a published app can only be reached by the maker's project, and the service-account key cannot be given to admins (it would let anyone push to every user of the app). Each admin would have to build their own APK. With the Web Push route (options 0 and 1) this problem goes away: the "sender" is a VAPID public key chosen at registration time, so each mail server's own key works with one published app.

RECOMMENDED DESIGN

A. First choice: direct, no relay (to be proven on a device)
1. App reads applicationServerKey from the JMAP session ("urn:ietf:params:jmap:webpush-vapid").
2. App to Play services: broadcast com.google.iid.TOKEN_REQUEST (package com.google.android.gms; extras as listed above, sender = that key, subtype "wp:<uuid per account>"). Answer: com.google.android.c2dm.intent.REGISTRATION with registration_id. Either use org.unifiedpush.android:embedded-fcm-distributor 3.0.0 or copy its roughly 150 lines.
3. App generates a P-256 key pair and a 16-byte auth secret, then PushSubscription/set create {deviceClientId, url: "https://fcm.googleapis.com/fcm/send/<token>", keys: {p256dh, auth}, types}.
4. Stalwart to FCM: POST /fcm/send/<token>, TTL: 86400, Urgency: normal, Content-Encoding: aes128gcm, Authorization: vapid t=<jwt aud https://fcm.googleapis.com>, k=<key>, body = encrypted PushVerification. Expected answer 201 (RFC 8030; FCM's actual status unverified).
5. Play services to app: com.google.android.c2dm.intent.RECEIVE, extra rawData = the exact body. App (Rust core) decrypts per RFC 8291, reads verificationCode, PushSubscription/set update {verificationCode}.
6. Later posts carry StateChange (or EmailPush, CalendarAlert); the app decrypts and syncs over JMAP.
7. Renew expires before 7 days; on a new token, or a changed applicationServerKey, destroy the old subscription and create a new one (the url cannot be updated).
8. No Play services, registration refused, or no VAPID key on the server: keep the existing connection and foreground service.

B. Fallback if A fails: our own small relay (only then)
- Stalwart to relay: POST https://push.<our domain>/push/<fcm token>, headers TTL, Urgency, Content-Encoding: aes128gcm, Authorization: vapid ..., body up to 4096 bytes. Relay answers 201 on FCM success; 404 or 410 for UNREGISTERED (harmless but Stalwart will not drop the subscription); 413 above the limit; 429 or 503 otherwise so Stalwart retries.
- Relay to Google token endpoint: signed JWT (RS256, scope https://www.googleapis.com/auth/firebase.messaging) to https://oauth2.googleapis.com/token, answer access_token valid about 1 hour, cached (standard service-account flow; the scope string is in common-proxies rewrite/fcm.go).
- Relay to FCM: POST https://fcm.googleapis.com/v1/projects/<id>/messages:send, Authorization: Bearer <token>, body {"message":{"token":"<fcm token>","data":{"b":"<base64 body>"},"android":{"priority":"high" only when Urgency is high, else "normal","ttl":"<TTL>s"}}}. Answer 200 {"name":...}. Errors: UNREGISTERED 404, INVALID_ARGUMENT 400, SENDER_ID_MISMATCH 403, QUOTA_EXCEEDED 429, UNAVAILABLE 503, INTERNAL 500.
- Size: FCM's limit is 4096 bytes for the whole data payload. Base64 turns 4096 bytes into about 5460, so the app must hold the server to about 2.9 KB, or the relay splits in two as common-proxies does at 3800 characters. A StateChange is far below this; EmailPush is what could reach it.
- Abuse: the token in the URL is the only credential (same as every Web Push service). Add body and rate limits per token and per source IP; optionally verify the VAPID JWT and bind a token to the first key seen. No database needed.
- App side for B: Firebase Messaging library, google-services.json from our project, FirebaseMessagingService.onNewToken (re-create the PushSubscription) and onMessageReceived (base64-decode, decrypt in the core), same fallback as A.8. The app would then no longer be free of Google libraries.

FCM LIMITS (Google pages, fetched 2026-11-14)
- 4096 bytes per message (ErrorCode page, INVALID_ARGUMENT); TTL 0 to 2,419,200 s, default and maximum 4 weeks (projects.messages reference).
- Priority (https://firebase.google.com/docs/cloud-messaging/android/message-priority): normal is delivered at once when the device is awake and "may be delayed" in Doze; high wakes the device, but "High priority messages on Android are meant for time sensitive, user visible content" and FCM looks at 7 days of behaviour and may deprioritise an app whose high-priority messages show no notification. Consequence for us: Stalwart sends StateChange with Urgency normal, so in Doze a new-mail push can wait for the next maintenance window. Only EmailPush lets the client choose urgency high. How FCM maps the Web Push Urgency header for an Android token: not found.
- Quotas (https://firebase.google.com/docs/cloud-messaging/throttling-and-quotas): 600k messages per minute per project; 240 per minute and 5,000 per hour to one device.

COST
- FCM: listed among the "no-cost" products on https://firebase.google.com/pricing; the Spark plan needs no payment method.
- Cloudflare Workers Free (https://developers.cloudflare.com/workers/platform/limits/): 100,000 requests per day, 10 ms CPU per request (network waiting not counted), 50 subrequests per request, 128 MB. The RS256 signature for the Google token, once an hour, is the only CPU-heavy step; whether it fits in 10 ms was not measured.
- Cloud Run (https://cloud.google.com/run/pricing): 2 million requests, 180,000 vCPU-seconds and 360,000 GiB-seconds free per month for request-based billing in Tier 1 regions; needs a billing account.
- Option 0 costs nothing and has no host.

WHAT A RELAY OPERATOR LEARNS, AND THE PRIVACY NOTE
The relay (or, in option 0, only Google) sees: the device token, the time and size of each push, the mail server's IP address, and the server's VAPID public key. It never sees mail content, addresses or account names: the body is encrypted for a key that exists only on the phone. Note for users, option 0: "For push without a permanent notification the app uses Google's push service. Your mail server sends Google a short encrypted signal that something changed; Google can see when and from which server, not what. You can turn this off and use the app's own connection instead." With a relay, add the maker's host as a second party that sees the same metadata and keeps no log beyond what is stated.

UNVERIFIED
- The whole direct route end to end: that current Play services still issue a "wp:" token to an app with our target SDK 36, that FCM accepts Stalwart's exact header (JWT has no sub unless webPushContact is set; Chrome's push service documents sub as expected, FCM's rule here was not found), the status FCM returns, and delivery in Doze with Urgency normal. Needs one test: a Stalwart on a public host, or a curl with a hand-made VAPID header, plus an emulator image with Play services.
- Whether a fresh Stalwart install has a VAPID key by default (the local test server had webPushKey set; the code treats it as optional).
- Token lifetime and refresh for "wp:" registrations (the gist and library do not say; the Firebase onNewToken callback does not exist on this route).
- Whether Stalwart's push client follows FCM redirects or needs any (policy is "SameOrganization", 4 at most; not examined further).
- The gist text was summarised by the fetch tool, not read raw; the library source confirms the same extras.
- Sygnal, ntfy and Mozilla autopush details beyond the lines quoted are from their READMEs or memory.

Sources cloned to the session scratchpad (gone)/relay-src (public repositories only; nothing about this project was sent anywhere; the repository was not changed).
