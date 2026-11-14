REPORT: optional UnifiedPush transport. Scout against the local Stalwart 1.1.0 (http://127.0.0.1:18080), user tester@kweb.test.

HEADLINE
1. I could NOT capture a single POST from the server (no PushVerification, no StateChange, no headers). The server accepts a subscription only for an https URL on a public DNS name. When it later tries to deliver, its resolver refuses any name that resolves to loopback or a private address. So no local listener can ever receive a push from this server. Everything below about POST shape is marked VERIFIED (observed) or UNVERIFIED (from binary strings or RFCs).
2. Consequence for the product: a user's Stalwart can only push to a PUBLIC https endpoint. A ntfy or other distributor reachable only on the LAN or a VPN will not work through JMAP PushSubscription.

SESSION (VERIFIED)
Session capabilities include "urn:ietf:params:jmap:webpush-vapid": {"applicationServerKey": "BFK3F4tWa25zdwbTkgyC9L_yRwldD0fr2vMBoD98bmgryXdluemPzdn7FJcwaSoKYDZq36hioQ_e7iJdeu0pat0"}. It is a 65-byte uncompressed P-256 point, base64url, no padding. This is the server's VAPID public key.
Also present: "urn:ietf:params:jmap:emailpush": {}, and "urn:ietf:params:jmap:websocket": {"url":"ws://127.0.0.1:18080/jmap/ws","supportsPush":true}.
eventSourceUrl: http://127.0.0.1:18080/jmap/eventsource/?types={types}&closeafter={closeafter}&ping={ping}
Server settings, admin object "Jmap" (read with stalwart-cli): pushThrottle 1s, pushAttemptWait 1m, maxPushAttempts 3, pushRetryWait 1s, pushRequestTimeout 10s, pushVerifyTimeout 1m, maxSubscriptions 15 (the default), maxPushSize 4 KB ("EmailPush objects are truncated to fit"), webPushContact (VAPID sub claim, unset), webPushKey (the PKCS#8 PEM VAPID signing key, set), pushShardsTotal 1.

URL ACCEPTANCE (VERIFIED), PushSubscription/set create. Method call shape:
["PushSubscription/set",{"create":{"a":{"deviceClientId":"probe-1","url":"<URL>"}}},"0"]
Responses are notCreated:{"a":{"type":"invalidProperties","description":"...","properties":["url"]}} with these descriptions:
- http://127.0.0.1:19001/p1: "URL scheme is not permitted"
- http://example.com/p: "URL scheme is not permitted" (any http is refused)
- ftp://x/p: "URL scheme is not permitted"
- https://127.0.0.1:19001/p, https://[::1]:19001/p, https://192.168.1.5/p: "Destination is a local or reserved IP address"
- https://localhost:19001/p and https://example.invalid/p: "Host is not a public domain name"
- https://foo.localtest.me:19001/p3 (name that resolves to ::1 and 127.0.0.1) and https://192.168.1.115.nip.io:19001/p4 (resolves to the LAN IP): ACCEPTED at create. The check at create is only on the host text (scheme https, not an IP literal, a public-looking domain name). A custom port is accepted.
So the local server does not accept plain http or loopback at all, and no JMAP setting turns this off. The Jmap singleton has no such field.

DELIVERY ATTEMPT (VERIFIED from the server log after I raised the Tracer to Trace and ran the admin Action ReloadSettings; the log needs a reload to change level):
- At create, the server logs DEBUG push-subscription.error details="Failed to verify push subscription" or "Push subscription verification deferred", reason="Too many requests". This appears when several creates happen within about a second, so a throttle defers the verification.
- About 60 s later (pushAttemptWait) it tries the POST and logs: details="HTTP POST failed", url="https://foo.localtest.me:19001/p3", reason="error sending request for url (...): client error (Connect): dns error: error resolving DNS: Destination is a local or reserved IP address".
- My listener (TLS, dual-stack, port 19001) logged zero TCP connections.
- A second guard at send time is built into the resolver. A real public https host is needed.
- I did not try tricks to get around the guard. A self-signed certificate would likely have been rejected anyway.

PushSubscription/get and set (VERIFIED)
create response: {"created":{"a":{"id":"b","keys":null,"expires":"2026-11-21T10:00:37Z"}}}. "keys" is null even when keys were sent. Ids are short strings and get reused after destroy.
get default properties: {"id","deviceClientId","verificationCode":null,"expires","types":[...]}.
- Requesting "url" or "keys" in get: {"type":"forbidden","description":"The 'url' and 'keys' properties are not readable"} as a method-level error.
- verificationCode is always null in get. The code can only be learned from the POST to the endpoint.
- PushSubscription/changes and PushSubscription/query: unknownMethod.
- types=null at create stores all 23: CalendarAlert, ParticipantIdentity, ShareNotification, Principal, FileNode, ContactCard, AddressBook, CalendarEventNotification, CalendarEvent, Calendar, SieveScript, Quota, MDN, VacationResponse, SearchSnippet, PushSubscription, Core, Identity, Thread, Mailbox, EmailSubmission, EmailDelivery, Email.
- types=[] stores [] and types=["EmailDelivery","CalendarAlert"] reads back ["CalendarAlert","EmailDelivery"].
- Unknown type ["Email","Bogus"]: invalidProperties "Invalid data type." properties ["types"].
- keys {"p256dh":b64url(65-byte uncompressed point),"auth":b64url(16 bytes)}: accepted, with or without = padding.
- auth of 8 bytes: invalidProperties "Invalid auth secret, expected 16 octets." properties ["keys"].
- Bad p256dh: "Invalid P-256 ECDH public key." properties ["keys"].
- Missing deviceClientId was accepted.
- expires: default is now+7 days. A value beyond 7 days is silently clamped to now+7 days, on create and on update. A requested near time (2026-11-15T00:00:00Z) is kept. A past time (2020-01-01T00:00:00Z) is accepted at create, but the subscription is then absent from get (notFound ["h"]), so expired subscriptions are filtered out.
- update {"b":{"expires":...}} returns "updated":{"b":null}. This is the renewal call.
- update of "types" and "keys":null succeeded.
- update url: notUpdated invalidProperties "Field could not be set." properties ["url"]. The url is immutable.
- update verificationCode wrong: notUpdated invalidProperties "Verification code does not match." properties ["verificationCode"]. A correct code could not be tested.
- destroy of an unknown id: notDestroyed {"type":"notFound"}.
- 16th subscription: {"type":"overQuota","description":"There are too many subscriptions, please delete some before adding a new one."}.
- Whether the server itself extends or drops a subscription on its own: not observed. I only saw filtering at expiry. The client must renew before the 7 days end.

EmailPush extension (VERIFIED for validation only; delivery not testable)
Create accepts an extra property "emailPush": {"<accountId>": {"properties":[...email properties...], "urgency":"very-low|low|normal|high"}}. Validation errors:
- bad urgency: invalidProperties "Invalid urgency value." properties ["emailPush"]
- unknown email property ("nope"): "Unknown email property."
- another account id: {"type":"forbidden","description":"No access to one of the accounts in the emailPush map."}
- Accepted: an empty {} config, and properties such as id, threadId, mailboxIds, keywords, size, receivedAt, sentAt, subject, from, to, cc, bcc, sender, replyTo, preview, hasAttachment, messageId, inReplyTo, references, bodyValues, textBody, blobId, header:List-Id:asText, attachments, bodyStructure.
- It was accepted even without urn:ietf:params:jmap:emailpush in "using".
- Binary strings: "EmailPush objects are truncated to fit" maxPushSize, and "Failed to build EmailPush notification object". The wire shape of an EmailPush POST is UNVERIFIED.

WHAT A POST LOOKS LIKE (UNVERIFIED; from binary strings, the RFCs and the EventSource)
- The PushVerification body is a literal string in the binary: {"@type":"PushVerification","pushSubscriptionId":"<id>","verificationCode":"<code>"}.
- StateChange: the same JSON I captured on the EventSource (VERIFIED there), after Email/set created an Inbox mail:
  event: state
  data: {"@type":"StateChange","changed":{"c":{"Thread":"snu","Mailbox":"snu","EmailDelivery":"snu","Email":"snu"}}}
  Keys are account ids, values are type to state. EmailDelivery appears for a JMAP-created Inbox mail too. The push POST body is presumably this JSON, encrypted when keys were given and plain otherwise (UNVERIFIED).
- Header strings found in the binary: "Content-Encoding: aes128gcm", "Urgency", the decimal "86400" (presumably TTL), "vapid t=" (Authorization: vapid t=<JWT signed ES256 with webPushKey>, presumably k=<applicationServerKey>; the k part is unconfirmed), and an "Invalid AES-GCM key length" error. Log events are push-subscription.success, .error and .not-found.
- CalendarAlert: it is an accepted type and the jmap-client 0.4.3 EventSource parser has a CalendarAlert variant, so calendar alerts are expected to go over push. Not tested.

USING THE ARTIFACT IN THE CORE
The jmap-client 0.4.3 already in Cargo.lock has push_subscription_create, push_subscription_verify, push_subscription_update_types, push_subscription_destroy, a Keys::new(p256dh, auth) setter and expires as an i64. The core has no PushSubscription code today. The current push path is the EventSource loop in core/src/push/run.rs, plus a foreground service (PushService, specialUse) in the Android manifest. The repository has no UnifiedPush or ntfy reference.

UNIFIEDPUSH ANDROID (from my own knowledge, not checked online; versions NOT certain)
- Library: Maven Central, group org.unifiedpush.android, artifact "connector" (plus optional "connector-ui"). I believe the current line is 3.x (3.0.x). Exact latest patch: not sure. I believe the 2.x line (2.5.x, a MessagingReceiver BroadcastReceiver) is the older API. The minimum Android API is low (about 21); our minSdk 29 is fine.
- Declare a service that extends org.unifiedpush.android.connector.PushService, exported, with an intent-filter for action "org.unifiedpush.android.connector.PUSH_EVENT".
- Callbacks (3.x): onNewEndpoint(endpoint: PushEndpoint, instance: String), onMessage(message: PushMessage, instance: String), onRegistrationFailed(reason: FailedReason, instance), onUnregistered(instance).
- PushEndpoint carries url, pubKeySet (PubKeySet with pubKey and auth, base64url) and temporary. PushMessage carries content: ByteArray and decrypted: Boolean.
- Registration: pick a distributor with UnifiedPush.getDistributors, tryUseCurrentOrDefaultDistributor or saveDistributor, then UnifiedPush.register(context, instance, messageForDistributor?, vapid?). The vapid argument takes the server's applicationServerKey, for push servers that require it. UnifiedPush.unregister(context, instance) removes it.
- Keys: in 3.x the connector generates and stores the web push key pair and auth secret itself, and returns the public part in pubKeySet. The app puts p256dh and auth from pubKeySet into PushSubscription/set "keys". The connector then decrypts RFC 8291 bodies and hands plaintext to onMessage with decrypted=true. If the app does not use the connector's decryption, content is the raw aes128gcm record. Which side holds the private key is therefore a design choice; with the connector it stays inside the library.
- Distributor behaviour: ntfy and similar pass the raw POST body through. Whether a given distributor forwards the Content-Encoding header is not something I can state.

DECRYPTING RFC 8291 IN THE RUST CORE (if the core does it)
- Cargo.lock has: sha2 0.10.9, crypto-common 0.1.7, subtle 2.6.1, ring 0.17.14, aws-lc-rs 1.19.0 and aws-lc-sys 0.47.0. It does NOT have p256, hkdf, hmac, aes, aes-gcm, ecdsa, elliptic-curve, ece or web-push. I looked each name up in Cargo.lock.
- ring 0.17 can do hkdf and AES-128-GCM, but its agreement API only supports ephemeral private keys, so it cannot do ECDH with a stored key. The app's key is long-lived, so ring alone is not enough for the ECDH step.
- Crates that would do it: p256 (features "ecdh" and "arithmetic") for the ECDH, plus hkdf and aes-gcm from RustCrypto (sha2 is already there). The ece crate needs OpenSSL, so it is the poor fit for Android.
- Steps: the body is salt(16) | rs(4, big-endian) | idlen(1) | keyid (the 65-byte server public key) | ciphertext. ECDH(ua_private, as_public) gives the secret. PRK_key = HKDF-Extract(auth_secret, secret). IKM = HKDF-Expand(PRK_key, "WebPush: info\0" || ua_public || as_public, 32). Then PRK = HKDF-Extract(salt, IKM), CEK = HKDF-Expand(PRK, "Content-Encoding: aes128gcm\0", 16), NONCE = HKDF-Expand(PRK, "Content-Encoding: nonce\0", 12) (XORed with the record counter). Decrypt each record with AES-128-GCM and strip the padding delimiter (0x02 on the last record).
- The VAPID Authorization header would only matter to a push server. The app does not need to verify it.

OPEN QUESTIONS FOR THE LEAD
1. Real wire format of the StateChange POST (encrypted or plain without keys, TTL value, whether the Urgency header carries a value). It can only be observed against a Stalwart that can reach a public https host, or by reading the Stalwart source.
2. Whether the subscription is active before verification. Logs suggest verification is attempted first and retried after 60 s, up to maxPushAttempts 3.
3. Whether the server ever extends or drops a subscription itself.
4. Whether CalendarAlert pushes really arrive.
5. Which side decrypts (connector or core), and whether ntfy or other distributors keep the encrypted body intact.

LEFT BEHIND ON THE LOCAL SERVER
- Subscriptions: none (all destroyed).
- Tracer jolskymeaaaa: I set its level to Trace and back to Info, and ran ReloadSettings twice (Action ids bvpqmkd, bvpqmo9). While at Trace, the log files under ~/tools/stalwart-test/logs/stalwart.log.2026-11-14 recorded full HTTP request headers, including the tester Basic Authorization header (base64) and request and response bodies. The log is local only; I did not print the password. Consider that entry sensitive.
- About 5 test mails with subject "push probe <timestamp>" and a few from dev/seed_mail.py "one" in tester's Inbox. The seed script also hit "429 Too Many Requests" several times: another agent was using the server at the same time (accounts second, filesq and files2 were active).
- One curl to https://1.1.1.1 for a connectivity check; no mail server was contacted.
- My local listener is stopped. Scripts, a self-signed cert and logs are in /tmp/claude-1000/-mnt-C-Users-kaank-Desktop-kweb-mail-for-stalwart/1ec96980-4871-4ff3-b43b-06ef4006739f/scratchpad/up/ (lib.py, listener.py, p1-p8.py, clean.py, mk.py). The repository was not edited.