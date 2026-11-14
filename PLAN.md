# Plan

Written 2026-11-12. Nothing is built yet. This file is the agreed starting point; change it when a decision changes.

## Goal

A simple native Android mail client for a Stalwart server, with a Rust core and a Kotlin interface.

The first priority is notifications:

- New mail shows a notification within a few seconds, with the screen off.
- A notification disappears by itself when the mail is read, archived or deleted on any other device.
- The app can show the user, at any time, whether delivery is working.

Design and scope were agreed from [design/mockups.html](design/mockups.html); the look moved to Material You later (see Decisions).

## Decisions

| Decision | Choice | Why |
|---|---|---|
| Core language | Rust | Chosen by Kaan. Sync, cache and notification rules are testable on the desktop. |
| Interface | Kotlin, Jetpack Compose | Android's first-class toolkit; services and notifications are Kotlin APIs. |
| Bridge | UniFFI | Generates the Kotlin bindings from the Rust interface. |
| Name | "kweb mail for Stalwart"; "kweb mail" under the icon | It says what the app works with. "Stalwart" is only used descriptively: no Stalwart logo, and the README says the project is independent. |
| Server | Stalwart 1.1 or later, JMAP only | No IMAP, POP3 or SMTP code. Other JMAP servers are not tested, so the core may rely on Stalwart's own behaviour (server-fired calendar alerts, its OAuth client registration). |
| Push transport | The app's own JMAP EventSource connection, held by a foreground service | No relay, no Google services, about 1 s delay. Same design as Postern. |
| Sign-in | OAuth 2.0 authorization code with PKCE in the browser | Passkeys work; no password is stored. Ported from Postern. |
| Accounts | One in phase 1 | Keeps the first version small. The data model keys everything by account so more can follow. |
| Distribution | Signed APK, sideloaded | No Play Store review, which also avoids its foreground-service rules. |
| Branding | Ward: crimson `#c8234a` (dark `#f0577a`), ink `#1b1d2a` (dark `#eceef4`), tint `#fde4ea` (dark `#4a1424`) | Files in [branding/ward](branding/ward). Since 2026-11-14 this is the icon and the name only; the app's colours are the next row. |
| Look | Material You (Material 3 Expressive). The colours are the user's choice: the phone's wallpaper palette (dynamic colour, Android 12 and later, the default there) or a built-in theme: Ward crimson (the default on older phones), Lavender, Moss, Peach. Light or dark follows the system. | Owner's decision, 2026-11-14. Mock-ups: [design/mockups-material.html](design/mockups-material.html), which holds the palettes the app uses (`ui/theme/Palettes.kt`). [design/mockups.html](design/mockups.html) is the earlier fixed-crimson design. Built so far: colour, the theme picker (in the folder drawer), and the mock-ups' style on the mail list, thread, message, search, Delivery check and sign-in screens (grouped rows with large outer corners, square avatars for unread mail, the tall search pill, tonal bars, the verdict card on Delivery check; see [design/m3-material.png](design/m3-material.png) and, next to the mock-up, [design/m3-material-compare.png](design/m3-material-compare.png)). The app's own green and amber status colours are gone: a passed check is the tertiary container, a failed one the error container. The connected reply buttons, the Compose button and the bottom bar come with M4 and M5. |

## Layout of the repository

```
core/                  Rust crate (kweb-core)
  src/auth             OAuth, token renewal
  src/jmap             requests; jmap-client for mail, own types for calendars and contacts
  src/store            SQLite cache: mailboxes, messages, state strings, outbox, shown notifications
  src/sync             Email/changes, Mailbox/changes, full resync when the server cannot give changes
  src/push             EventSource stream, re-arm, backoff
  src/notify           decides which notifications should exist
  src/actions          mark read, archive, delete, send; queued when offline
  src/ffi.rs           the UniFFI interface
  tests/               against a mock JMAP server
cli/                   small desktop program that drives the core against a real server
android/               Gradle project
  app/                 Compose screens, services, receivers
design/                mockups
branding/              Ward assets
```

The Kotlin side never talks to the server. It calls the core and renders what the core returns.

## The notification rule

The core holds one rule: **the notifications on screen must equal the unread Inbox mail that arrived after sign-in and that the user has not swiped away.**

Each time the rule runs, the core compares that set with what it last told Android to show, and returns two lists: post these, cancel these.

It runs when:

1. the server pushes a state change (new mail, or a change made on another device),
2. the connection is re-established,
3. the 15-minute background check fires,
4. the app comes to the foreground,
5. the phone finishes booting.

Details that make it hold:

- The shown set is stored in SQLite, so a killed process does not forget what is on screen.
- A notification's Android id is derived from the message id, so posting twice updates instead of duplicating.
- A swipe-away is recorded, so that mail is not posted again.
- Removal is silent. Only a genuinely new message makes a sound.
- Actions (Archive, Mark read, Reply) go through the core, also when the app was not running.

Android pieces:

| Piece | Job |
|---|---|
| Foreground service | Holds the EventSource connection. Shows the silent "Connected" entry. |
| WorkManager job, every 15 min | Runs the rule and restarts the service if it was killed. |
| Boot receiver | Starts the service after a restart. |
| Action receiver | Handles notification buttons and swipes. |
| Channels | Mail, Reminders, Connection (silent). |

To check at milestone 2: the foreground service type (`specialUse` is the plan, because Android 15 limits `dataSync` services to 6 hours a day), and how the target phone's maker treats exempted apps.

## Milestones

Each one ends with something that can be run and checked.

### M0: toolchain

- Rust with Android targets, NDK, cargo-ndk, UniFFI, Gradle.
- A Compose screen that shows a string returned by the Rust core, on an emulator and on the phone.
- CI that builds the core, runs its tests and builds a debug APK.

### M1: core sync

- Sign-in, session, token renewal.
- Mailboxes and message headers synced into SQLite, kept current with `/changes`.
- `cli` shows the Inbox and follows live changes.
- Tests against a mock JMAP server, including a lost connection and an expired token.

### M2: notifications

The priority milestone. It ships with only a bare message list so notifications have something to open.

- Push service, the notification rule, actions, calendar and task reminders, the Delivery check screen.

Done when all of these pass on the real phone:

| Test | Expected |
|---|---|
| Mail arrives, screen off | Notification within 5 s |
| That mail is read on the desktop | Notification gone within 5 s, no sound |
| Mail archived or deleted elsewhere | Notification gone within 5 s |
| Airplane mode for 10 min, three mails arrive, one is read elsewhere, back online | Exactly the two unread ones show, within 30 s |
| Phone restarted, app not opened | Notifications work again (verified on the phone 2026-11-14) |
| App swiped out of recents | Notifications keep working |
| Phone idle overnight | Morning mail arrives within 5 s; nothing stale is shown |
| Access token expires | Renewed without a missed notification |
| Notification swiped away, other mail arrives | The swiped one does not come back |
| Mark read / Archive / Reply from the notification | Applied on the server; notification goes |
| Calendar reminder with the app closed | Shown on time; Snooze works |

### M3: reading

Inbox, threads, message view (HTML in a sandboxed WebView, remote images off by default), folders, search, swipe actions, offline reading of recent mail.

Built 2026-11-14 (not committed at the time of writing). How it was decided:

- **Lists are conversations.** A row is a thread's mail in that folder; the thread screen shows the whole thread, replies in Sent included (`Thread/get`, then the headers that are missing).
- **Bodies come from `Email/get` body values, not from the raw message.** The server has parsed the MIME already and the request carries no attachments. The raw message's blob id is stored with the body, so M9 can fetch the RFC 5322 bytes when it needs them (this replaces "keep the raw bytes" in M9 stage 0).
- **Offline reading:** the bodies of the newest 100 Inbox mails are fetched after a notification pass, beside the live loop and never in its way, plus every message that was opened. Attachments and inline pictures are fetched when shown and kept in the app's cache directory.
- **Each folder has a window:** the cache holds it without gaps back to a date, and the list shows only that part; "load older" moves the window. Without it, an old mail fetched by a search or a thread would appear in the list with a gap above it.
- **HTML is not cleaned in the core.** The WebView has scripts off, no file or content access, a content security policy, and every request goes through one policy: `cid:` pictures from the mail, remote content only after "Show" for that message (network loads are blocked in the view until then), links open in the browser on a tap only.
- **New actions:** star, move to a folder (also Spam and Undo of a swipe). A change the server refuses for good is dropped from the queue and the message is shown again as the server has it.
- **Search** is the server's (`text` filter); offline it falls back to the cached senders, subjects and previews and says so.
- **Left for M4:** Compose and reply buttons, Settings. Not done in M3: marking the matched words in search results, a per-sender "always show images".

Checked on the emulator against the local Stalwart: list, folders drawer, a four-mail thread, HTML with a remote image (hidden, then shown), an inline picture, swipe to archive with Undo, search on the server, reading a prefetched mail with the server cut off. Not checked there: opening and saving an attachment, Move to, load older on a large folder, the dark theme, a notification tap opening its thread. Not checked on the real phone at all.

### M4: sending

Compose, reply, forward, attachments, drafts on the server, sender identities, undo send, recipient completion from the address book, quota in Settings.

**Release 0.1.0** after M4: phase 1 of the scope.

### M5: calendar

Agenda and day views, create and edit events, repeat rules, invitations, tasks.

### M6: contacts and away message

Contact list, view, edit. Vacation response switch.

### M7: sharing, files, second push route, AI assistant

Shared mailboxes and calendars, file storage, and UnifiedPush as an optional transport. And an AI assistant.

**AI assistant.** Researched 2026-11-14 from the owner's Thunderbird extension (AI Assistant 1.2.3), Bulwark's AI Assistant plugin and Stalwart 1.1's docs and source.

- **Where the model runs.** Stalwart's AI is server-internal: an `AiModel` object (managed over JMAP as `x:AiModel/*`) feeds only the spam classifier and the Sieve function `llm_prompt`. No documented JMAP or HTTP call lets a client send a prompt to it, and it is an Enterprise feature. So the app calls a provider itself, from the Rust core, as Bulwark and the Thunderbird extension do. A "use my server's model" mode is possible only if Stalwart adds an endpoint. Not verified: that none exists (absence in the docs is not proof).
- **Providers.** Two adapters, both streaming with tool calling: OpenAI-compatible (`POST {baseUrl}/chat/completions`, key optional so Ollama, LM Studio and vLLM work over the LAN or a VPN; hide `<think>` output; retry without tools if a 400 mentions tools) and Anthropic Messages (`/messages`). Base URL, key and model are set by the user. Keys go through the existing secret-store callback (Android Keystore), never plain settings or logs. The extension's "Claude on this computer" mode has no phone equivalent and is left out.
- **Privacy (from Bulwark).** Nothing is sent until the user taps. A first-use confirmation names the service, host and model. Per-feature switches. Hard caps: about 60,000 characters and 8 pictures per request, 20 messages per thread summary, about 12,000 characters per mail body. Remote images are never loaded.
- **Safety (from both).** Mail and tool results are wrapped in tags and declared to be data, not instructions. The assistant never sends or deletes on its own: a draft opens in the composer, and any change asks first (an approval card showing what will happen) with Undo. The extension's four risk classes (read, ui, change, confirm) and its cap of 30 agent steps carry over.
- **Stages.** (1) Summarize a message or thread, draft a reply, and a compose sheet to rewrite, shorten, change tone, fix spelling and translate. (2) Reply ideas, to-do and date extraction, explain a selection. (3) A chat screen whose model looks things up through tools over the JMAP code already in the core (search, read, list folders, calendar). (4) Actions (archive, move, mark, tag, add events) with ask-first as the default.
- **Left out.** Bulwark's files area, memory notes, admin-lock config and plugin hooks; the extension's panel docking, Thunderbird calendar and address-book wrappers, and Claude CLI bridge.
- **Stalwart Enterprise.** Checked in the v1.1.0 source (`crates/common/src/enterprise/config.rs`): with no licence key the server returns no Enterprise edition, except in a build made with the compile-time flag `--cfg stalwart_tests`, which grants a one-year licence for 100 accounts on any hostname. It is not a runtime setting, so the official release binary does not have it; a dev server for this app would be built from source with that flag (the licence's section 4.5 allows copying the source for development and testing). Without it, the official route is the 30-day trial. M7 does not depend on either. Owner's decision (2026-11-14): when the AI work starts, build Stalwart from source with that flag as a second local test server (separate data and port, next to the normal one) and port the Thunderbird extension's provider layer, prompts and tools first; a real Enterprise licence comes later and is tested for real then.

### M8: filters (a simplified Sieve editor)

Researched 2026-11-14 against Bulwark's source (`lib/sieve/`, `stores/filter-store.ts`, `components/settings/filter-settings.tsx` in bulwarkmail/webmail) and RFC 9661.

- JMAP only (`urn:ietf:params:jmap:sieve`: `SieveScript/get`, `/set`, `/validate`, script text through blobs). No ManageSieve. The section is hidden when the server does not advertise the capability.
- A visual rule builder, as in Bulwark. First version: conditions From (address or domain), To/Cc, Subject and one custom header, match all or any; actions Move to folder, Mark read, Star, Delete (discard) and Stop. Left out at first: regex, body, size and attachment tests, wildcard matches, activation periods, drag and drop (up/down buttons instead).
- Rules are the source of truth, stored as JSON in a versioned metadata comment at the top of one script, followed by the generated Sieve (Bulwark's approach). The builder never parses arbitrary Sieve. A script without valid metadata is foreign: shown read-only or as raw text, never overwritten silently, with a warning that activating ours would deactivate it (JMAP allows one active script). One managed script, named `kweb-filters`.
- Always `SieveScript/validate` before saving and show the server's error text; save per change and restore the old state on failure; delete with Undo. Gate constructs on the session's Sieve capability (extensions, `maxNumberRedirects`: forward is hidden at 0; script size). Move rules get a spam guard unless "include spam" is on.
- "Create a rule from this message" on the message screen (sender, domain, List-Id) is the mobile-friendly way in; new rules go on top.
- The away message is its own simple screen on `VacationResponse` (M6). Stalwart stores it as a reserved script named `vacation`, and enabling it displaces the user's active script, so the editor treats that script as read-only and warns about it.
- A raw editor is an advanced option, read-only with copy at first; saving by hand marks the script hand-edited and turns the builder off until a confirmed reset.
- The core gets: list scripts, get and put text, validate, activate, deactivate, delete, rename, and the rule list with its Sieve generator, all tested against the mock server and the local Stalwart. Stalwart's error names (`invalidScript`, `scriptIsActive`) differ from the RFC's (`invalidSieve`, `sieveIsActive`): accept both and check on the test server.
- Stalwart's limits are low (one redirect and three outgoing messages per run, a script size cap): check the generated rules against them.

### M9: optional S/MIME and OpenPGP

For people who use them; the owner does not. Off by default and independent of the server: it works over plain JMAP (`Email/get`, blob download), so nothing here needs Stalwart's help. Stalwart's own encryption is at rest only: the user uploads a public key, the server encrypts incoming mail with it, and it never holds a private key. Researched 2026-11-14; the sizes are estimates, not sourced.

| Stage | What | Rough size |
|---|---|---|
| 0 | Fetch the raw RFC 5322 bytes by the blob id M3 stores with each body, and parse MIME with exact signed byte ranges. A spike proves the chosen crates build with cargo-ndk for arm64 and x86_64. | with M3 |
| 1 | Read-only verify: `multipart/signed` (PGP and S/MIME) and a small status chip; a keyring of public keys and certificates in the core database. Likely stack: rPGP (`pgp` crate) for OpenPGP; RustCrypto `cms`/`x509` crates for S/MIME. | 3 to 4 weeks |
| 2 | Decrypt: import private keys (armored PGP, PKCS#12) through the file picker, wrapped by an Android Keystore key, held in Rust only for the call and zeroized after. Detect mail Stalwart encrypted at rest and say that its bodies cannot be searched on the server. | 2 to 3 weeks |
| 3 | Compose: sign and encrypt, including to the sender's own key so Sent stays readable; avoid double encryption with Stalwart's gateway. | 3 to 4 weeks |
| 4 | Key discovery and trust: Autocrypt headers, WKD, trust on first use with warnings when a fingerprint changes; show where a key came from. | 2 to 3 weeks |
| 5 | Only if asked for: hardware tokens, certificate chain validation against the system store with revocation. | |

Not planned: depending on OpenKeychain (maintenance mode, GPL). To check before starting: rPGP's detached-signature support and Android build, and the `cms` crate's maturity with old `.p12` algorithms.

## Scope

| Phase | Included |
|---|---|
| 1 (M0–M4) | Browser sign-in, mail, sending, push with read-elsewhere removal, reminders, quota |
| 2 (M5–M6) | Calendar, contacts, away message |
| 3 (M7) | Shared mailboxes and calendars, files, UnifiedPush, AI assistant (own provider keys) |
| 4 (M8–M9) | A simplified Sieve filter editor; optional S/MIME and OpenPGP |
| Not planned | IMAP, POP3, SMTP or ManageSieve (JMAP only, Stalwart only); a full Sieve script editor; OpenKeychain |

The capability-by-capability table is in the mockups page.

## Reused from Postern

Postern (MIT, same author) already has, in Rust: the OAuth flow with client registration and token rotation, the EventSource watcher with re-arm and backoff, and calendar alert fetching. These are ported into `core/`, not rewritten.

## Risks

| Risk | Handling |
|---|---|
| The phone's maker kills background apps despite the exemption | Delivery check detects it and says which setting to change. If it cannot be solved on that phone, UnifiedPush moves up from M7. |
| `jmap-client` has no calendar, contact or file types | Hand-written types in `core/src/jmap`, as Postern does for reminders. |
| UniFFI async and callbacks across the bridge | Proven in M0 before anything depends on it. |
| Hand-written Sieve or scripts from other tools collide with the filter editor | The builder owns one script with a metadata comment and treats anything else as foreign, read-only (M8). |
| Rust crates for PGP and S/MIME are less proven on Android | A build-and-round-trip spike before M9 stage 1 (Thunderbird, GnuPG and OpenSSL as counterparts). |
| Stalwart changes behaviour between releases | The mock server tests pin what the core expects; the `cli` runs against the real server before each release. |

## Open questions

1. Which phone and Android version is the target? It sets the minimum SDK (proposed: Android 10, API 29) and how much the battery risk matters.
2. Package id. Proposed: `ovh.kweb.mail`.
3. ~~Licence.~~ Settled 2026-11-13: MIT, as Postern. The repository is public.
