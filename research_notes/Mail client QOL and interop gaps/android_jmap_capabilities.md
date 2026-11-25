# Android platform integrations and JMAP/Stalwart capabilities for a native Stalwart mail client (as of 2026-11-25)

## Android: notifications, conversation features, Live Updates (Android 11-17)

### Takeaway
Conversation-style notifications need MessagingStyle plus a long-lived sharing shortcut, but Google says they are meant for real-time chat, not email, so mail apps should use them selectively. Android 16 progress-centric notifications and Live Updates are restricted to ongoing user-initiated activities (for a mail app: send-with-undo, upload or download progress). Notification cooldown is a user-level system setting with no known app API.

### Cited Findings
- A notification counts as a conversation notification if it uses `MessagingStyle`, is tied to a valid long-lived dynamic or cached sharing shortcut (`setShortcutId()` or `setShortcutInfo()`), and the user has not demoted the conversation. Conversation notifications get the conversation space, priority marking, bubbles, and per-conversation silence and sound. — [Android conversations guide](https://developer.android.com/develop/ui/views/notifications/conversations)
- The shortcut must be long-lived and carry `Person` data. `LocusIdCompat` is recommended for ranking and last-interaction time. — [Android conversations guide](https://developer.android.com/develop/ui/views/notifications/conversations)
- Google states that conversation notifications are intended for real-time conversations (SMS, chat, calls) and that "users don't have that expectation with emails". — [Android conversations guide](https://developer.android.com/develop/ui/views/notifications/conversations)
- Do not cancel a conversation notification before the user could see the message, because the associated bubble is removed too. Provide 104dp avatars and a `data` URI for MIME-related message metadata. — [Android conversations guide](https://developer.android.com/develop/ui/views/notifications/conversations)
- Long-lived sharing shortcuts also appear in the sharesheet's Direct Share row (the same shortcuts serve both). — [Android conversations guide](https://developer.android.com/develop/ui/views/notifications/conversations); [Direct share targets](https://developer.android.com/develop/ui/compose/sharing/direct-share-targets)
- Android 12 conversation widgets show conversation status via `ConversationStatus`, via `setIsConversation()` shortcuts. — [Android conversations guide](https://developer.android.com/develop/ui/views/notifications/conversations)
- Notification cooldown (Android 15 preview, kept in Android 16) lowers volume of successive notifications from the same app; it is a user setting. Calls, alarms and priority conversations are exempt. Coverage found no app-facing API or opt-out. — [Android Central on Android 16](https://www.androidcentral.com/apps-software/android-16-has-a-new-trick-to-put-a-damper-on-annoying-app-pings); [9to5Google](https://9to5google.com/2024/02/16/android-15-notification-cooldown/)
- Android 16 added `Notification.ProgressStyle` (progress-centric notifications) for user-initiated start-to-end journeys (rideshare, delivery, navigation) with top ranking in the drawer. Live Updates are the promoted form (`POST_PROMOTED_NOTIFICATIONS`); Google says to use them only for ongoing, user-initiated, time-sensitive activities, not for ambient information or shortcuts to features. — [Android 16 progress-centric notifications](https://developer.android.com/about/versions/16/features/progress-centric-notifications); [Live Updates guide](https://developer.android.com/develop/ui/compose/notifications/live-update); search summary at the same pages
- Android 17 adds semantic colour API for Live Updates (`Notification.createSemanticStyleAnnotation`, `setRequestPromotedOngoing(true)`), and a `MetricStyle` template. — [Android 17 features](https://developer.android.com/about/versions/17/features); [Android 17 search summary](https://developer.android.com/about/versions/17/behavior-changes-17)
- Android 17 adds `AlarmManager.setExactAndAllowWhileIdle` with an `OnAlarmListener` instead of a `PendingIntent`, aimed at apps that hold wakelocks for sockets (relevant to an always-on JMAP connection and scheduled work). — [Android 17 features](https://developer.android.com/about/versions/17/features)

### Inferences
- A "conversation" notification per email thread is possible technically but goes against Google's stated intent; a reasonable use is only for known-person threads or opt-in.
- Send-with-Undo, large downloads and resync progress are plausible Live Update candidates; new-mail alerts are not (ambient, not user-initiated).

### Gaps
- No source fetched for Direct Reply (`RemoteInput`), `VISIBILITY_PRIVATE` public versions, notification bundling and summary groups, Wear OS bridging, or smart-reply/suggested actions. These are long-standing APIs but not verified in this research pass.
- Whether cooldown applies to a mail app's notifications and whether channel settings interact with it was not found in developer docs.

## Android: home screen, adaptive layouts, input, edge-to-edge, widgets

### Takeaway
Targeting API 36 forces edge-to-edge and predictive back, and removes large-screen orientation/resizability opt-outs on 600dp+ screens (no opt-out at all when targeting 37). Glance 1.2.0 is stable, with generated widget previews. `NavigableListDetailPaneScaffold` is the supported two-pane list-detail building block.

### Cited Findings
- Apps targeting Android 16 (API 36) cannot opt out of edge-to-edge (`windowOptOutEdgeToEdgeEnforcement` is deprecated and disabled), get predictive back animations by default (temporary opt-out via `android:enableOnBackInvokedCallback=false`), and `onBackPressed` is no longer called. — [Android 16 behavior changes](https://developer.android.com/about/versions/16/behavior-changes-16)
- For apps targeting API 36, orientation and resizability restrictions are ignored on displays with smallest width 600dp or more; apps targeting Android 17 cannot opt out of those changes. — [Android 16 behavior changes](https://developer.android.com/about/versions/16/behavior-changes-16); [Android 17 behavior changes](https://developer.android.com/about/versions/17/behavior-changes-17)
- Jetpack Glance 1.2.0 stable was released 2026-08-26; generated widget previews via `GlanceAppWidget.providePreview` and `GlanceAppWidgetManager.setWidgetPreview` work on Android 15+ (compileSdk 35+), with a `previewImage` fallback; `setWidgetPreview` is rate-limited (about 2 calls per hour by default). — [Glance release notes](https://developer.android.com/jetpack/androidx/releases/glance); [Generated previews](https://developer.android.com/develop/ui/compose/glance/generated-previews)
- `NavigableListDetailPaneScaffold` (artifacts `adaptive`, `adaptive-layout`, `adaptive-navigation`) gives list, detail and optional extra pane: side-by-side on large windows, one pane on small; default back behavior `PopUntilScaffoldValueChange`. — [List-detail guide](https://developer.android.com/develop/adaptive-apps/guides/list-detail)
- Keyboard support in Compose: `onKeyEvent` modifier for shortcuts; Google's large-screen codelab includes a Keyboard Shortcuts Helper, keyboard navigation and scroll shortcuts; Device Manager has Desktop virtual devices for testing. — [Compose keyboard commands](https://developer.android.com/develop/ui/compose/touch-input/keyboard-input/commands); [Keyboard and mouse codelab](https://developer.android.com/codelabs/large-screens/add-keyboard-and-mouse-support-with-compose)
- Android 17 adds Handoff (`Activity.setHandoffEnabled()`, `onHandoffActivityDataRequested()`) to continue an activity on another nearby Android device, per-activity. — [Android 17 features](https://developer.android.com/about/versions/17/features)

### Inferences
- Handoff could hand a half-written draft or an open thread to a tablet; it requires the app on both devices and state passed through `HandoffActivityData`.
- A Glance unread-count/inbox widget is cheap given Glance 1.2 stable; the widget must read from the local cache, not the network.

### Gaps
- No sourced pages for app shortcuts (static/dynamic launcher shortcuts), Quick Settings tiles, drag-and-drop of attachments, stylus/handwriting, or Material 3 Expressive specifics. Not fetched.

## Android: system integrations (passkeys, OTP, contacts, assistants, Play policy)

### Takeaway
There is no documented path for a mail app to feed email OTP codes to the keyboard; Android 17 adds a permission-free contact picker; AppFunctions (Android 16) lets an app expose actions to agents, but Gemini access for third-party apps was still in private preview as of May 2026. Exact alarms are a restricted permission on Play, with calendar and alarm apps as the explicit qualifying categories.

### Cited Findings
- Android 17 adds a system Contact Picker (API 37+) that grants read access to only the selected contacts and fields, as an alternative to `READ_CONTACTS`; useful for the recipient field. — [Android 17 features](https://developer.android.com/about/versions/17/features)
- AppFunctions: Android 16+ platform API plus Jetpack library; apps declare functions by annotation, a generated XML schema is indexed by the OS; callers need `EXECUTE_APP_FUNCTIONS`. As of May 2026 Gemini integration was in private preview with trusted testers; Gemini was reported using AppFunctions for Google/OEM Calendar, Notes and Tasks. — [AppFunctions overview](https://developer.android.com/ai/appfunctions); [9to5Google, Feb 2026](https://9to5google.com/2026/02/25/android-appfunctions-gemini/)
- Autofill: the Jetpack Autofill library 1.2.0-alpha01 added an "Email OTP" hint constant; the documented OTP hint for apps is `AUTOFILL_HINT_SMS_OTP`. Search found no documented mechanism for a mail app to supply OTP suggestions to the keyboard, and a 1Password feature request describes email OTP autofill as unsolved. — [Autofill release notes](https://developer.android.com/jetpack/androidx/releases/autofill); [Autofill optimisation](https://developer.android.com/guide/topics/text/autofill-optimize); [1Password request](https://www.1password.community/discussions/1password/feature-request-autofill-for-otp-codes-delivered-by-email/170270)
- April 2026 Android Developers post: Google issues cryptographically verified email credentials through Credential Manager (a sign-in feature for apps, not a mail-reading feature). — [Android Developers Blog](https://android-developers.googleblog.com/2026/04/streamline-auth-credential-manager-verified-email.html)
- Exact alarms: Google Play treats exact-alarm permissions as restricted; the core function must need exact timing. Calendar and alarm/timer apps are the qualifying categories; `USE_EXACT_ALARM` is install-time and not revocable; other apps use `SCHEDULE_EXACT_ALARM` (user-granted). Lists of qualifying categories vary by secondary source. — [Play policy announcement](https://support.google.com/googleplay/android-developer/answer/14554661); [Android 14 exact alarm change](https://developer.android.com/about/versions/14/changes/schedule-exact-alarms)
- Android 17 platform: Advanced Protection Mode (`AdvancedProtectionManager` API to detect), Encrypted Client Hello platform support (`<domainEncryption>` in network security config), hybrid PQC APK signing. — [Android 17 features](https://developer.android.com/about/versions/17/features)

### Inferences
- Because scheduled send in this app is server-side (held until its time), it does not need exact alarms; a calendar-reminder feature would be the only qualifying exact-alarm use.
- An email-code copy action in notifications has to be built in-app (parse the body, offer a Copy action); no system hook was found.
- AppFunctions could expose "search mail", "compose draft", "mark read" but no verified third-party Gemini consumer exists yet.

### Gaps
- Not sourced: Photo Picker/document picker, per-app language, themed icons, Circle to Search, contacts provider sync (already built in the project), foreground service Play policy details, Credential Manager passkey sign-in (already working in the app), Wear OS.
- Play's current permissions policy page text was not fetched; only the 2022 announcement and secondary sources.

## JMAP RFCs and drafts: status and client features

### Takeaway
Core mail, WebSocket, sharing, quotas, blob, Sieve, MDN, S/MIME verification, Contacts, JSContact and VAPID Web Push are published RFCs. Calendars and JSCalendar 2.0 are still drafts; snooze is an expired draft.

### Cited Findings
- Published RFCs: RFC 8620 (core, push, resync); RFC 8621 (Mail); RFC 8887 (WebSocket); RFC 9007 (MDN, read receipts); RFC 9404 (Blob management); RFC 9425 (Quotas); RFC 9661 (Sieve scripts); RFC 9670 (Sharing); RFC 9219 (S/MIME signature verification status); RFC 9610 (Contacts); RFC 9553 (JSContact); RFC 9749 (VAPID for JMAP Push). — [jmap.io spec index](https://jmap.io/spec/index.html)
- In progress: JMAP Calendars (draft-ietf-jmap-calendars; revisions up to -32 seen) and JSCalendar 2.0. — [jmap.io spec index](https://jmap.io/spec/index.html); [datatracker, JMAP Calendars](https://datatracker.ietf.org/doc/draft-ietf-jmap-calendars)
- Calendars draft defines Calendar, CalendarEvent, CalendarEventNotification and ParticipantIdentity types; `Principal/getAvailability` belongs to the principals capability. — [draft-ietf-jmap-calendars](https://datatracker.ietf.org/doc/draft-ietf-jmap-calendars)
- Snooze: the IETF EXTRA draft ("Snoozing Email with IMAP, JMAP, and Sieve", capability `urn:ietf:params:jmap:mail:snooze`, a snoozed mailbox role plus an Email property) is an expired Internet-Draft. A `$snoozed` keyword was not found. A MailMaint draft (`draft-ietf-mailmaint-messageflag-mailboxattribute`, rev -14, Jan 2026) registers further keywords and mailbox attributes including a snoozed mailbox attribute. — [email-snooze draft](https://datatracker.ietf.org/doc/draft-ietf-extra-email-snooze/); [MailMaint draft](https://datatracker.ietf.org/doc/draft-ietf-mailmaint-messageflag-mailboxattribute/)
- Client-side features each enables (from the spec index descriptions): MDN read receipts (9007), quota display and sync (9425), inline blob creation and reverse lookup (9404), Sieve management and validation (9661), sharing of mailboxes/calendars/address books (9670), push with fast resync (8620), higher-performance transport (8887). — [jmap.io spec index](https://jmap.io/spec/index.html)

### Inferences
- Snooze cannot be built on a standard: a client-side approach (local reminder plus a folder or label) or Stalwart's own mechanism, if any, is needed; Stalwart support for the draft was not found.
- `SearchSnippet`, `Email/parse`, `Email/import`, Identity and VacationResponse are part of RFC 8621 (not separately enumerated on the spec index); the app already uses several.

### Gaps
- No Stalwart statement on snooze support. No dedicated source on Email/parse, SearchSnippet details.

## Stalwart 0.11 to 1.2: what the server implements

### Takeaway
Stalwart 1.0 (2026-10-26) moved to a new database layout and added JMAP Object Metadata, updated Calendars and FileNode drafts; recent releases added masked email in the open-source edition (1.1.1), per-user encryption at rest with OpenPGP/S/MIME (1.0.2), passkeys (1.1.0), MDN methods and availability (1.2.0). Releases before 0.16.25 were not covered by the page fetched.

### Cited Findings
- Stalwart README lists: JMAP Mail (RFC 8621), Sieve (RFC 9661), Object Metadata (draft-ietf-jmap-metadata-04, covering emails, mailboxes, calendars, contacts, files, Sieve scripts), WebSocket (RFC 8887), Blob (RFC 9404), Quotas (RFC 9425), Calendars (draft-ietf-jmap-calendars-32), Contacts (RFC 9610), File Storage (draft-ietf-jmap-filenode-17), Sharing (RFC 9670). The README does not mention MDN or a push mechanism. — [Stalwart README](https://raw.githubusercontent.com/stalwartlabs/stalwart/main/README.md)
- Docs: JMAP push is available via event source, WebSocket or Web Push; one session covers mail, calendars, contacts and files; users manage Sieve via ManageSieve or JMAP for Sieve. — [Stalwart email docs](https://stalw.art/docs/email/)
- 1.2.0 (23 Nov 2026): MDN/send and MDN/parse (RFC 9007), `Principal/getAvailability`, per-account working hours, CalDAV availability property, resource accounts, server-side mail rules compiled to Sieve, junk-sender lists, IMAP APPEND trains the classifier only for `$Junk`/`$NotJunk` or uploads to Junk, optional Exchange protocol support. — [Stalwart releases](https://github.com/stalwartlabs/stalwart/releases)
- 1.1.1 (16 Nov): masked email moved into the open-source edition; S/MIME-encrypted messages show decrypted preview text for iOS. 1.1.0 (9 Nov): WebAuthn passkey sign-in for internal-directory accounts. — [Stalwart releases](https://github.com/stalwartlabs/stalwart/releases)
- 1.0.3 (4 Nov): blob encryption (AES-256-GCM or ChaCha20-Poly1305) and zstd compression, OAuth scopes so third-party apps can manage app passwords and settings. 1.0.2 (2 Nov): encryption at rest reworked with OpenPGP and S/MIME, accounts reset to Disabled on upgrade, key harvesting. 1.0.1: draft messages not encrypted on append, OpenTelemetry/Prometheus exporters. — [Stalwart releases](https://github.com/stalwartlabs/stalwart/releases)
- 1.0.0 (26 Oct): database layout break (migration tool needed from 0.16.x), Object Metadata, updated Calendars and File Storage drafts, full-text search of PDF/Office/ODF/EPUB/RTF files, AI classifier with multiple providers (Enterprise), SSRF protection for webhooks. — [Stalwart releases](https://github.com/stalwartlabs/stalwart/releases)
- Spam filter: rules, AI classifier (OpenAI-compatible, Anthropic, Gemini, Ollama), statistical classifier, DNSBL, Pyzor, greylisting, spam traps; webhooks supported; about 85 percent of features are AGPL-3.0, remainder Enterprise (multi-tenancy, SCIM, sharded storage, live telemetry). — [Stalwart README](https://raw.githubusercontent.com/stalwartlabs/stalwart/main/README.md)

### Inferences
- Stalwart implements the calendar and file storage drafts at specific draft revisions (-32 and -17), so client code against them should expect drift.
- A client can expose report-spam via setting `$Junk`/`$NotJunk` keywords or moving to Junk, since the server trains on those (stated for IMAP APPEND; JMAP behavior not separately confirmed).
- 1.2.0's MDN methods make read-receipt support possible for a client.

### Gaps
- Release notes for 0.11 to 0.16.24 not retrieved (page showed 0.16.25+ only).
- Not found in sources: undo send/FUTURERELEASE, auto-expunge, iTIP/iMIP scheduling details, Sieve extension list, JMAP-specific spam-report method, whether Stalwart implements `urn:ietf:params:jmap:mail:snooze`. The Stalwart docs pages for these were not fetched (docs URL guessed at one path returned 404).
