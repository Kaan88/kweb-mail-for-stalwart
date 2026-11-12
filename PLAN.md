# Plan

Written 2026-11-12. Nothing is built yet. This file is the agreed starting point; change it when a decision changes.

## Goal

A simple native Android mail client for a Stalwart server, with a Rust core and a Kotlin interface.

The first priority is notifications:

- New mail shows a notification within a few seconds, with the screen off.
- A notification disappears by itself when the mail is read, archived or deleted on any other device.
- The app can show the user, at any time, whether delivery is working.

Design and scope were agreed from [design/mockups.html](design/mockups.html).

## Decisions

| Decision | Choice | Why |
|---|---|---|
| Core language | Rust | Chosen by Kaan. Sync, cache and notification rules are testable on the desktop. |
| Interface | Kotlin, Jetpack Compose | Android's first-class toolkit; services and notifications are Kotlin APIs. |
| Bridge | UniFFI | Generates the Kotlin bindings from the Rust interface. |
| Server | Stalwart 1.1 or later, JMAP only | No IMAP or SMTP code. |
| Push transport | The app's own JMAP EventSource connection, held by a foreground service | No relay, no Google services, about 1 s delay. Same design as Postern. |
| Sign-in | OAuth 2.0 authorization code with PKCE in the browser | Passkeys work; no password is stored. Ported from Postern. |
| Accounts | One in phase 1 | Keeps the first version small. The data model keys everything by account so more can follow. |
| Distribution | Signed APK, sideloaded | No Play Store review, which also avoids its foreground-service rules. |
| Branding | Ward: crimson `#c8234a` (dark `#f0577a`), ink `#1b1d2a` (dark `#eceef4`), tint `#fde4ea` (dark `#4a1424`) | Files in [branding/ward](branding/ward). |

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
| Phone restarted, app not opened | Notifications work again |
| App swiped out of recents | Notifications keep working |
| Phone idle overnight | Morning mail arrives within 5 s; nothing stale is shown |
| Access token expires | Renewed without a missed notification |
| Notification swiped away, other mail arrives | The swiped one does not come back |
| Mark read / Archive / Reply from the notification | Applied on the server; notification goes |
| Calendar reminder with the app closed | Shown on time; Snooze works |

### M3: reading

Inbox, threads, message view (HTML in a sandboxed WebView, remote images off by default), folders, search, swipe actions, offline reading of recent mail.

### M4: sending

Compose, reply, forward, attachments, drafts on the server, sender identities, undo send, recipient completion from the address book, quota in Settings.

**Release 0.1.0** after M4: phase 1 of the scope.

### M5: calendar

Agenda and day views, create and edit events, repeat rules, invitations, tasks.

### M6: contacts and away message

Contact list, view, edit. Vacation response switch.

### M7: sharing, files, second push route

Shared mailboxes and calendars, file storage, and UnifiedPush as an optional transport.

## Scope

| Phase | Included |
|---|---|
| 1 (M0–M4) | Browser sign-in, mail, sending, push with read-elsewhere removal, reminders, quota |
| 2 (M5–M6) | Calendar, contacts, away message |
| 3 (M7) | Shared mailboxes and calendars, files, UnifiedPush |
| Not planned | Sieve filter editor, S/MIME or PGP key handling in the app |

The capability-by-capability table is in the mockups page.

## Reused from Postern

Postern (MIT, same author) already has, in Rust: the OAuth flow with client registration and token rotation, the EventSource watcher with re-arm and backoff, and calendar alert fetching. These are ported into `core/`, not rewritten.

## Risks

| Risk | Handling |
|---|---|
| The phone's maker kills background apps despite the exemption | Delivery check detects it and says which setting to change. If it cannot be solved on that phone, UnifiedPush moves up from M7. |
| `jmap-client` has no calendar, contact or file types | Hand-written types in `core/src/jmap`, as Postern does for reminders. |
| UniFFI async and callbacks across the bridge | Proven in M0 before anything depends on it. |
| Stalwart changes behaviour between releases | The mock server tests pin what the core expects; the `cli` runs against the real server before each release. |

## Open questions

1. Which phone and Android version is the target? It sets the minimum SDK (proposed: Android 10, API 29) and how much the battery risk matters.
2. App name and package id. Proposed: "kweb mail", `ovh.kweb.mail`.
3. Licence, if the repository is ever made public.
