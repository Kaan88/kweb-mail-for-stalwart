# kweb mail (Android, Stalwart/JMAP): inventory of existing, planned and rejected features

Sources are repository files (paths relative to the repo root; `K` = `android/app/src/main/kotlin/ovh/kweb/mail`, `R` = `android/app/src/main/res/values`). Status: PRESENT / PARTIAL / ABSENT. Checked against code, strings and manifest, not only docs. Version at time of reading: 0.3.0 (2026-11-25). Read-only; nothing was built or run, so "present" means "in the source", and PLAN.md says several screens were never opened on a device.

## Reading

### Takeaway
Reading is strong: conversations, quote folding, per-sender remote-image control, a server-checked sender mark, in-app PDF viewer, zoom/fit like Gmail, one-time-code copy. Absent: show original/headers, print/PDF, save .eml, translate-a-received-mail (translate exists only in compose), link-destination preview, tracking-parameter stripping, tracker detection.

### Cited Findings
- Conversation view with an on/off setting ("One row for each message"), text size (Small/Default/Large/Larger), preview lines 0-3, mark-as-read timing (when opened / after 2 s / never), where to go after archive or delete (list/next/previous): PRESENT. `R/strings_reading.xml`, `K/ui/ReadingRules.kt`, `K/ui/settings/ReadingScreen.kt`.
- Quote folding: PRESENT (all quotes when thread has 3+ messages, else only long ones; forwarded messages not folded for length; `<details>` without scripts). `K/ui/thread/QuoteFolding.kt`; PLAN.md M13.
- Remote images: PRESENT, off by default. Show once, "Always from this sender" (allow list per account, editable), and a global "Always" setting; never for shared accounts or S/MIME. `R/strings_reading.xml`; `core/src/reading.rs`; PLAN.md M11 stage 2.
- Sender check (DMARC/DKIM/SPF from the server's Authentication-Results, with explanation sheet and warning text): PRESENT. `R/strings_sender.xml`, `core/src/sender_check.rs`, `K/ui/sender`. Live, never cached, no mark when server did not check.
- Show original / full headers / raw source: ABSENT in UI (no string or menu entry; thread menu strings are only reply/forward/archive/delete/mark unread/move/star/labels/spam/block/filter/assistant). The core stores the raw blob id (PLAN.md M3), so it is feasible but unbuilt. Not even listed under "maybe".
- Print / save as PDF / save or share as .eml: ABSENT (grep for PrintManager and message/rfc822 finds nothing). PLAN.md M10 "Maybe" list: "print or save a message as PDF or `.eml`" (not scheduled).
- Translate a received message: ABSENT as a reader feature. "Translate" exists only in the compose assistant menu (language picker, recent first, searchable): `K/ui/compose/TranslateRules.kt`, `ComposeScreen.kt`. The thread assistant menu has Summarize, Explain, Find dates, Reply ideas, Draft reply, Ask about (`K/ui/thread/ThreadScreen.kt`, MessageCard).
- Dark mode for mail: PRESENT in the sense that HTML mail is drawn on the theme background with padding (design review 2026-11-14, PLAN.md M10); no separate "force dark mail" switch found.
- Links: tapped links go through `UrlPolicy` (http, https, mailto allowed; others refused) and open in browser/mail app on tap only: `K/ui/thread/UrlPolicy.kt`. Plain-text URLs are auto-linked (`WebLinks.kt`). Showing the real destination before opening: ABSENT, listed under PLAN.md M10 "Maybe, borrowed" (FairEmail). Stripping `utm_*`/`fbclid`: ABSENT, same "Maybe" list (grep finds no `utm_`).
- Tracker/pixel detection or counts: ABSENT (remote content is simply blocked until Show). Only mention is that pictures "tell the sender that you opened the mail" (`R/strings_reading.xml reading_pictures_note`).
- Attachments: save to device, download with progress/cancel in a foreground service, open with other apps; PDF viewer in the app (`K/ui/thread/PdfViewer.kt`, PdfRenderer, pinch zoom, "Open with"): PRESENT. Inline `cid:` pictures shown. Attachment preview for other types (images/Office): only via "Open with"; no gallery view found. "Add invitation or contact from `.ics`/`.vcf` attachment": ABSENT, PLAN.md M10 "For later consideration".
- Pinch zoom and fit of wide mails (fitted to window, zoom inside the mail, pan stops at edge): PRESENT. `K/ui/thread/MailWebView.kt`, `FitRules.kt`, `FitScript.kt`; PLAN.md M13.
- One-time codes: PRESENT, "Copy code" notification button (first button, unlocked phone only, switchable). `K/background/OneTimeCodeRules.kt`; `R/strings_notifications.xml notifications_copy_code`.
- Encrypted/signed mail (OpenPGP, S/MIME) status chips, decrypt, keyring: PRESENT but off by default and "never seen on a device" per CLAUDE.md. `R/strings_crypto.xml`, `core/src/crypto/`.
- Reading pane in WebView is locked down (no JS, no file access, CSP, cid only): PLAN.md M3.
- Thread pager between conversations (swipe next/previous), open at first unread message, "Not delivered to" line: PRESENT. `K/ui/thread/ThreadRoute.kt`, `ThreadRules.kt`.

### Inferences
- A "view source / headers" and "save as .eml / print" gap is real and not on any planned list except the vague "maybe print/.eml".
- Link-safety features (real URL preview, tracking-parameter stripping) are acknowledged by the owner as "maybe" only.

### Gaps
- Whether message headers are reachable anywhere (e.g. long-press) was not exhaustively checked in MessageCard.kt; no string supports it.

## Triage and list

### Takeaway
Swipes (configurable), multi-select, labels, star, move, spam/block, unsubscribe, mark-all-read, undo, follow-up nudge are present. Snooze, mute, pin, unified inbox, sort options, priority inbox are absent (several explicitly "maybe").

### Cited Findings
- Swipes: configurable left/right from archive, delete, read/unread, star, move, none; confirm-before-delete switch; Reply-all default switch; reset. PRESENT. `K/ui/ActionRules.kt`, `K/ui/settings/ActionsScreen.kt`, `R/strings_actions.xml`.
- Selection mode in the main mail list (long press, select all, bulk actions): PRESENT (only in the main list, known limit in CLAUDE.md; Files has its own selection, 0.2.9).
- Star/flag (Flagged filter chip; Home "Flagged" tile): PRESENT. Chips Unread, Flagged, Attachments (`R/strings.xml filter_*`).
- Labels (JMAP keywords, create/rename/delete, drawer entry, per message): PRESENT. `R/strings_mailextras.xml`, `core/src/labels.rs`.
- Move, archive, delete (Trash), delete forever, undo for each: PRESENT.
- Spam/not spam, block sender (generates a filter rule): PRESENT. `R/strings_folders.xml`.
- One-tap unsubscribe (RFC 8058 post, mailto, or page): PRESENT. `core/src/unsubscribe.rs`.
- Mark all as read per folder (incl. with subfolders, with Undo): PRESENT (drawer folder menu). `K/ui/list/MarkAllRules.kt`.
- Folder management, per-folder "Show with subfolders" (kept), per-folder notify bell: PRESENT. `K/ui/folders`, `K/ui/list/SubfolderRules.kt`.
- Follow-up nudge ("sent mail with no reply" after 3-14 days, with Open, no model): PRESENT. `R/strings_attention.xml att_settings_follow_up`; CHANGELOG 0.2.6.
- Snooze mail (to Snoozed folder / reminder to come back): ABSENT. Listed PLAN.md M10 "Maybe, borrowed" with the note that JMAP has nothing for it so the app would move it back and could be late in Google push mode. The only "snooze" in code is a 10-minute snooze of calendar/task reminder notifications (`R/strings_background.xml action_snooze`).
- Mute conversation: ABSENT, "Maybe" list (Gmail). Pin conversation: ABSENT, "Maybe" list (Spark). Send-and-archive in one tap: ABSENT, "Maybe" list.
- Unified/"All inboxes" across accounts: ABSENT (one core per account; only one account's screens alive at a time; drawer shows each account's unread count). "Maybe" list notes real work.
- Sort options: ABSENT (lists newest first; no sort string/setting found). Thread on/off: PRESENT (Conversation view switch).
- Priority/Important inbox, bundles/categories: ABSENT; "Not wanted: ... Promotions and Social" (PLAN.md M10, text ends at that phrase in the line read; sender logos/BIMI also explicitly not wanted).
- "Remind me" on a mail: ABSENT as a user action; the Home "Worth a look" findings and sparkle "Add task" / "Add to calendar" cover related ground. `R/strings_attention.xml`.
- Home tab (optional): unread tile (chosen folders), next event, due today, flagged, recent files, delivery and away tiles; the sparkle "attention" findings (13 plain rules + follow-up). `K/ui/home`, `core/src/attention/`. Bento home proposal: not built, "to be judged" (CLAUDE.md).
- Server-side catch-up and chat tools: see Assistant.

### Inferences
- Snooze, mute, pin, unified inbox and send-and-archive are the owner's own "maybe" items, not unknowns, so a report should present them as already-considered-but-unbuilt.

### Gaps
- Exact set of items in the list's per-row long-press menu was not enumerated.

## Compose

### Takeaway
Compose is plain-text with drafts, undo send (5-30 s), send later (server-held, up to 7 days), identities/signatures, contact suggestions, greeting/sign-off completion, assistant rewrite/translate/draft-reply, optional PGP/S-MIME. Absent: rich text/formatting, templates/canned replies, forgot-attachment warning, camera/scan attach, spell-check beyond OS, large-file links.

### Cited Findings
- Plain text body with a plain-text signature per sender; no rich-text editor or formatting toolbar found: PARTIAL/ABSENT. `R/strings_sending_later.xml senders_signature` ("plain-text signature" per PLAN.md M10), `R/strings_sending.xml compose_body_hint`. No bold/italic strings in compose.
- Signatures per identity, sender name, reply-to, default sender for new mail: PRESENT. `R/strings_sending_later.xml senders_*`. Reply picks the identity the mail came to (PLAN.md phone feedback round 2/3; plus-tag handling).
- Send later (in 1 hour, this evening, tomorrow morning, pick date/time, up to 7 days, server holds it even when the phone is off; "Scheduled" list with cancel): PRESENT.
- Undo send: PRESENT, 5/10/20/30 s setting; server-side cancel when reachable. `R/strings_actions.xml`, `core/src/compose/later.rs`.
- Drafts synced to the server (autosave, "Draft saved"): PRESENT. Encrypted drafts kept local only. `R/strings_sending.xml compose_draft_saved`.
- Attachments: file picker (`GetMultipleContents`), remove, "larger than the server accepts" check (server limit), share-into-app of text/links/files (SEND and SEND_MULTIPLE filters): PRESENT. `K/ui/compose/ComposeScreen.kt:142`, `AndroidManifest.xml`. Camera/photo-capture attach: ABSENT (no TakePicture/ACTION_IMAGE_CAPTURE). Large-file link (upload to Files and insert a link): ABSENT in compose, although Files has public links with expiry (`R/strings_public_links.xml`, CHANGELOG 0.2.3, only on the Stalwart fork servers). Size warning before upload: PARTIAL (refusal after check against server limit).
- Contact suggestions in To/Cc/Bcc: PRESENT. `K/ui/compose/RecipientField.kt`, `RecipientRules.kt` (suggestions from cache/contacts; M4 "suggestions").
- "Forgot attachment" warning: ABSENT (grep for forgot/attachment_reminder finds only unrelated hits).
- Templates / canned replies: ABSENT; PLAN.md M12: "Later, not planned for this release: ... templates". Assistant "Draft a reply" with nine preset tones/intents covers part of the need (CHANGELOG 0.2.8).
- Spell check: no app-specific feature; relies on the platform text field. Assistant has "Fix spelling". Not verified whether the OS spell checker is active in the compose field.
- Greeting and sign-off completion ("Hi" -> recipient first name, "Best" -> rest), local, no model, switchable: PRESENT. `K/ui/compose/CompletionRules.kt`, `R/strings_completion.xml`.
- Reply / Reply all / Forward, reply-all default setting, reply-all drops user's own plus addresses, forward from shared mail: PRESENT.
- Reply from a shared mailbox, sending as identity: PRESENT (M7).
- Assistant in compose: improve, shorter, formal, friendly, fix spelling, translate (language list), draft reply with hint, translate all, with Undo; nothing sent. `R/strings_assistant*.xml`.
- Sign/encrypt (PGP/MIME, S/MIME), WKD lookup, Autocrypt: PRESENT, off by default, never seen on a device. `K/ui/compose`, `core/src/crypto/`.
- Read receipts (MDN) / delivery status on sent mail: ABSENT; PLAN.md M10 "Maybe: delivery status on sent mail, ... read receipts". Refused recipients keep the draft with the server's reply (M12 stage 3), thread notice not built.
- Masked addresses (receive only): PRESENT, see Privacy.

### Inferences
- The biggest compose gaps vs mainstream clients: rich text, templates, forgot-attachment, camera/scan attach, link-instead-of-attachment.

### Gaps
- Spell-check behaviour in the Compose body field, and any attachment size warning other than the server-limit refusal, not verified at runtime.

## Search

### Takeaway
Server-side full text with snippet highlights and an offline fallback over cached mail; no operators, no saved searches, no attachment-content search.

### Cited Findings
- Search: server `text` filter, all mail or one folder, newest first; offline fallback over cached sender/subject/preview with a notice. `core/src/ffi.rs:548`, `R/strings.xml search_offline`. PRESENT.
- Highlights from SearchSnippet in result rows: PRESENT. `core/src/ffi.rs:2410`, `core/src/snippets.rs`.
- Operators (from:, to:, has:attachment, is:unread, date ranges): ABSENT (search takes one string; grep finds none in `K/ui/search` or core sync). Filter chips Unread / Flagged / Attachments exist on the list instead.
- Saved searches / recent search history: ABSENT (no matching code).
- Search in attachment text: ABSENT. PLAN.md M12: "attachment text for the assistant" also "Later, not planned". Encrypted mail cannot be searched (`R/strings_crypto.xml crypto_not_searchable`).
- Search contacts, files (own search fields): PRESENT. `R/strings_contacts.xml`, `R/strings_files.xml`.
- Search across accounts: ABSENT (per-account core).

### Inferences
- Operators and saved searches are a clean gap; the server's JMAP filter could back them.

### Gaps
- Whether the server's text filter itself handles operator-like strings was not checked.

## Notifications

### Takeaway
One of the most developed areas: per-account channels, per-address Notify/Quiet/Off, per-folder bells, preview level, configurable action buttons, Copy code, own live connection or Google push, removal when read elsewhere. Absent: quiet hours/DND schedule, VIP senders, Direct Reply, notification grouping settings.

### Cited Findings
- Per-account notification screen: new mail on/off, remove when read elsewhere, reminders, invitations/sharing notices, folders that notify, per-address choice, "Sound and vibration" rows opening Android channel screens. PRESENT. `R/strings_notifications.xml`, PLAN.md M10 "notification options".
- Channel per receiving address (Notify / Quiet / Do not notify), Cc and delivery headers in the match: PRESENT. PLAN.md M11 stage 1.
- Preview level per account (subject+text / subject only / nothing): PRESENT.
- Notification buttons chosen and ordered by the user (default Delete, Mark read, Reply; also Archive, Snooze for reminders, Copy code first): PRESENT. `K/ui/settings/NotificationButtonsScreen.kt`, `R/strings_background.xml`.
- Notification rule: screen equals unread mail that arrived after sign-in and not swiped; removal when read elsewhere: PRESENT. CLAUDE.md, PLAN.md "The notification rule".
- Per-folder notify with filters moving mail out of Inbox: PRESENT.
- Calendar/task reminders with 10-minute snooze: PRESENT.
- Notices as notifications (invitation, event change/cancel, share): PRESENT. `core/src/notices.rs`, `R/strings_background.xml`.
- Delivery check screen (connection state, battery optimization, safety poll every 15 min, test notification): PRESENT. `K/ui/delivery`.
- Google push (Web Push through Play services, no Firebase, per account, with fallback to the permanent connection, offers other accounts): PRESENT, works on owner's phone since 2026-11-16, not fully checked overnight/reboot. `core/src/webpush/`, `R/strings_push.xml`.
- Always-on JMAP EventSource connection service (no Google needed): PRESENT. UnifiedPush dropped by owner (PLAN.md line ~712).
- Quiet hours / DND schedule: ABSENT; PLAN.md M10 "Maybe: quiet hours and VIP senders"; "Not done: quiet hours (not asked for)" in the notification options paragraph.
- VIP senders: ABSENT, same lines.
- Direct Reply (inline RemoteInput) from the notification: ABSENT (no RemoteInput in code; "Reply" button opens the app).
- Notification grouping/bundling setting: no setting found (not checked in Notifier.kt beyond grep).

### Inferences
- Quiet hours + VIP senders are the owner's own "maybe" pair.

### Gaps
- Whether notifications are grouped per account/summary was not verified in `K/background/Notifier.kt`.

## Calendar, contacts, files

### Takeaway
Calendar (agenda with expanded repeats, invitations, tasks, device calendar sync), contacts (CRUD, no device contact sync) and Files (browser, sharing, public links) all exist.

### Cited Findings
- Calendar: agenda, event screen, create/change/delete (single or whole series), invitations accept/maybe/decline, tasks (mark done, due), guests, reminders, repeating events (singular `recurrenceRule`), shared calendars. `R/strings_calendar.xml`, PLAN.md M5/M12. Week and month views: ABSENT (M12 "Later, not planned: week and month calendar views"). Find-a-time: ABSENT as a screen, but the sparkle "Find a slot" and "Free time" exist (`R/strings_attention.xml`).
- Device calendar sync into Android's calendar provider (sync adapter; own calendars only; widgets and other calendar apps see them): PRESENT, emulator-checked only. `K/devcal/`, `R/strings_devcal.xml`.
- Device contacts sync (ContactsContract provider): ABSENT. In-app contacts only: list, search, create/edit/delete, address book choice, notes, recent mail, call/map intents. `R/strings_contacts.xml`.
- Add contact/event from .vcf/.ics attachment: ABSENT (M10 "for later consideration").
- Away message (vacation response, with dates): PRESENT. `R/strings_contacts.xml away_*`.
- Files tab: browse own and shared, upload with conflicts and cancel, rename/move/delete, multi-select, search, sharing, public links with expiry (Stalwart fork only). `R/strings_files.xml`, `R/strings_public_links.xml`.
- "Shared with me": other users' mail folders, calendars, address books, read live and never cached; giving shares. `K/ui/shared`, `K/ui/sharing`.

### Inferences
- Device contacts sync is a genuine gap relative to how Bulwark Mobile and DAVx5 behave, but is not in any planned list.

### Gaps
- None material.

## Android integration

### Takeaway
Share target, per-app language, device calendar provider, share-into-app, in-app updater (github flavour) are present; widgets, Quick Settings tile, app shortcuts, mailto handler, tablet layout, predictive back, Wear OS, biometric lock are absent.

### Cited Findings
- Home-screen widgets: ABSENT (no AppWidgetProvider in manifest; grep). PLAN.md M10 "Maybe: home screen widgets"; M12 "Later, not planned for this release: home-screen widgets".
- Quick Settings tile: ABSENT (no TileService). App shortcuts (static or dynamic): ABSENT (no shortcuts.xml in `res/xml`: contents are authenticator, file_paths, locales_config, network_security_config, sync_calendar).
- Share target (SEND, SEND_MULTIPLE, `*/*`): PRESENT. `android/app/src/main/AndroidManifest.xml`.
- `mailto:` default-mail-app handler (VIEW/SENDTO intent filter for mailto): ABSENT in manifest (only SEND/SEND_MULTIPLE and the `ovh.kweb.mail` sign-in scheme). Links in mail with mailto open an external mail app (`UrlPolicy.isMail`), so the app cannot become the default handler. Not mentioned in any planned list.
- Tablet/foldable two-pane, WindowSizeClass, landscape layout: ABSENT. PLAN.md M10 "Maybe: tablet and landscape layout". Single-column Compose screens only.
- Predictive back: ABSENT (`android:enableOnBackInvokedCallback` not set in the manifest).
- Wear OS: not planned; "The Wear OS screens of the design chat are not in the plan" (PLAN.md line ~624).
- Per-app language: PRESENT (`android:localeConfig="@xml/locales_config"`). UI languages: English; PLAN.md lists Turkish under "For later consideration" (so likely English only now; not confirmed by listing `res/values-*` — only `values` and `values-night` exist, so English only).
- Direct Reply: ABSENT (see Notifications).
- Edge-to-edge/keyboard: `windowSoftInputMode=adjustResize` (Android 17 fix). PRESENT.
- Foreground services (live connection, downloads), BOOT_COMPLETED/MY_PACKAGE_REPLACED receiver, battery optimization prompts: PRESENT. Manifest.
- Account authenticator and sync adapter (for device calendar): PRESENT. Manifest.
- In-app updater (github flavour only; fdroid flavour has none; PackageInstaller session on user tap): PRESENT. `core/src/update.rs`, `K/background/UpdateCheck.kt`, `UpdateInstall.kt`.
- Theming: Material You wallpaper palette plus nine built-in palettes (Ward crimson default, Lavender, Moss, Peach, Ocean, Slate, Sand, Rose, Sage, Graphite), light/dark/system, pure black. PRESENT. `R/strings.xml theme_*`.
- Sign-in via browser/OAuth with passkey support, app link return: PRESENT.

### Inferences
- Of the integration items typically expected, none of widgets, tile, shortcuts, mailto handler, tablet layout, predictive back is built; widgets and tablet layout are explicitly "maybe/later", the other three are not on any list.

### Gaps
- Pre-Android-13 behaviours (minSdk 29) not examined.

## Privacy and security

### Takeaway
Server-oriented security is deep (account security, masked addresses, passkeys/app passwords, optional PGP/S-MIME, strict WebView, no tracking pictures by default). Absent: app lock, screenshot blocking in mail, encrypted local cache.

### Cited Findings
- App lock (biometric/PIN): ABSENT (no BiometricPrompt). PLAN.md M10: "For later consideration: ... app lock".
- Screenshot blocking: PARTIAL. `FLAG_SECURE` is set only on screens that show secrets (`K/ui/account/AccountParts.kt:57`, `K/ui/admin/AdminParts.kt`); not on mail screens.
- Encrypted local cache / encryption at rest: ABSENT; PLAN.md M12: "Later, not planned for this release: ... encryption at rest". Secrets (tokens, keys) live in a secret store (`core/src/secrets.rs`), private PGP keys in the secret store only.
- Masked addresses (create, note, site, prefix, switch off/on, delete): PRESENT, tested once against owner's licensed server. `R/strings_account.xml`.
- Account security (password, two-step TOTP, app passwords, API keys, passkeys, connected apps, revoke): PRESENT where the server allows. `K/ui/account`.
- Sender check, remote content off, locked WebView, remote images never in shared or S/MIME mail: PRESENT.
- PGP/S-MIME (M9 stages 0-4): reading signed, keyring, decrypt, sign/encrypt on send, Autocrypt/WKD/TOFU: PRESENT, off by default, never seen on a device; stage 5 (chain validation, hardware tokens) not built.
- Link/URL policy, update download only from the project's own releases, no silent install: PRESENT.
- Sign-out warns about unsent mail that would be lost: PRESENT. `R/strings.xml sign_out_lost`.
- Admin key bound to server/account, never logged: PRESENT (admin features). Server administration (queue, accounts, domains, reports, tasks, cluster, actions, Enterprise dashboard etc.): PRESENT for admins; not a QoL item for ordinary users.

### Inferences
- App lock and encrypted-at-rest cache are not planned; only "app lock" is on the "for later consideration" line.

### Gaps
- Whether the cache database (SQLite) is encrypted was inferred from absence of SQLCipher mentions; not checked in `core/src/store` dependency list.

## Filters, away message, assistant

### Takeaway
Filters (visual rule editor with Bulwark-compatible Sieve JSON), away message and a bring-your-own-model assistant with chat, sparkle actions and catch-up are all built.

### Cited Findings
- Mail filters: rule list with on/off, reorder, delete with undo, full rule editor (conditions, comparators, actions), raw Sieve editor, migration of old script, block sender and "Filter mail like this". `R/strings_filters.xml`, `K/ui/filters`, `core/src/filters/bulwark`. PRESENT. A full Sieve script editor as such is "not planned" (PLAN.md Scope table line 554) but raw text editing is available.
- Assistant (OpenAI-protocol services only, a list of up to 20, each with name, address, model, effort, key, consent; Anthropic removed 2026-11-25): summarize message/thread, explain, find dates (to calendar/tasks), reply ideas, draft reply, rewrite/translate in compose, a chat with read-only tool lookups and change cards (apply on tap, optional automatic allow-list), chat about one conversation, earlier chats, Catch me up (30 unread inbox mails), sparkle actions on Home findings, "Your instructions" up to 8000 chars. `R/strings_assistant*.xml`, `strings_catchup.xml`, `strings_chat_history.xml`, CLAUDE.md. Not seen on a device: the changed settings screen and the Service dropdown.
- On-device model (Gemini Nano) and "Sign in with ChatGPT": NOT planned (CLAUDE.md).

## Accounts and settings

### Takeaway
Multi-account is built (one core per account, drawer switcher, notifications tagged per account); Settings now includes Appearance, Reading, Swipes and actions, Notifications, Delivery, Home and attention, Mail group, Calendar > Device calendar, Encryption, About, data and storage.

### Cited Findings
- Several accounts: PRESENT (no unified inbox). `K/accounts/`, `R/strings_accounts.xml`.
- Settings list (from `K/ui/settings/SettingsScreen.kt` string references): Account (name, server, login, storage/quota, account display name), This app (Delivery check, Notifications, Appearance, Reading, Swipes and actions, Home and attention, About), Mail group (Senders and signatures, Scheduled, Away message, Mail filters, Masked addresses, Account security, Encryption), Calendar (Device calendar), "Keep mail on the phone" window (200/500/2000/everything), "Clear cache and resync", Sign out, Add account, Remove account, Assistant. PRESENT.
- Settings gaps relative to common clients: no conversation sort, no default folder or "start on" except Home/Mail toggle, no mailto default, no quiet hours/VIP, no signature rich text, no app lock, no per-sender notification rule, no "always show images" beyond what exists (present), no export/import of settings (not found).
- Update check (weekly, on by default), download on unmetered network, install through the system prompt (github flavour): PRESENT. `R/strings_update.xml`.
- F-Droid: flavour exists, merge request closed 2026-11-18 (app too young), resubmit mid-February 2027 (CLAUDE.md).

## What the docs list as open, rejected or proposed

### Takeaway
Everything below is the owner's own list and should be reported as "already considered".

### Cited Findings
- "Not planned" (PLAN.md line 554 Scope table): IMAP, POP3, SMTP or ManageSieve (JMAP only, Stalwart only); a full Sieve script editor; OpenKeychain.
- "Not wanted" (PLAN.md M10, ~line 351): sender logos (BIMI/favicons leak to third parties); Promotions and Social style categories.
- "Dropped": UnifiedPush (owner 2026-11-14); Anthropic protocol (2026-11-25); Today overview sheet left out of the app (owner 2026-11-14); bento home proposal still "to be judged" (CLAUDE.md, PLAN.md M11 intro says not built).
- "Not planned": Wear OS screens; Bulwark's plugin hooks and Nextcloud/Roundcube region handling; "Sign in with ChatGPT"; on-device Gemini Nano.
- "Maybe" (PLAN.md M10, ~line 350): home screen widgets; delivery status on sent mail; quiet hours and VIP senders; print or save a message as PDF or `.eml`; tablet and landscape layout; read receipts.
- "Maybe, borrowed from other apps" (owner 2026-11-23; ~line 351): send and archive in one tap; show where a link really goes (domain highlighted); strip tracking parameters; mute a conversation; pin a conversation; All inboxes across accounts; snooze; quiet hours with VIP senders. Marked "built instead": mark all read, open at first unread, follow-up nudge.
- "For later consideration": add invitation/contact from `.ics`/`.vcf` attachment; Turkish (UI language); app lock.
- "Later, not planned for this release" (PLAN.md M12 ~line 448): home-screen widgets, attachment text for the assistant, encryption at rest, templates, find-a-time, week and month calendar views.
- Not built / open (CLAUDE.md): a download that survives leaving the app (partly addressed since: foreground-service downloads, 2026-11-16 stage 7); M9 stage 5 (chain validation, hardware tokens); the thread notice for partially refused recipients; Undo of "Report spam" in a folder-with-subfolders view returns mail to the parent folder; inline `cid:` pictures in decrypted mail show as files; selection exists only in the main mail list; Google push overnight/reboot checks.
- Open owner items (PLAN.md M13): all 14 items of the 2026-11-23 and 2026-11-24 lists are built (header row, swipe with wide mails, quote folding, table fit, one-time codes, greeting/sign-off completion, Settings in the account menu, Home unread folders, 8000-char instructions, draft-a-reply, translate language list, pan stops at edge, PDF open) but "nothing opened on an emulator or phone" for much of M13.
- Release signing in CI secrets added (2026-11-23), not yet seen in a run; version code fixed at 1 so "register again after an update" never fires. CI core job has been failing on flaky timing tests since 0.2.3.

### Inferences
- Nearly every QoL idea a competitor review would raise (snooze, mute, pin, widgets, quiet hours/VIP, tablet layout, link preview, tracking stripping, print/.eml, unified inbox) is already on the owner's "maybe" lists; a report should frame these as "owner-considered, unbuilt" rather than new.
- Items not found on any list (so potentially new to the owner): show original/headers/view source, mailto default-handler intent filter, app shortcuts and Quick Settings tile, predictive back, Direct Reply, forgot-attachment warning, camera/scan attach, search operators and saved searches, sort options, device contacts sync, rich-text compose, translate a received message, large-file link in compose, bulk settings export, autolink-safety beyond UrlPolicy.

### Gaps
- Turkish translation status: only `values` and `values-night` directories exist; no `values-tr`. Confirms English-only UI, not intent.
- Features absent in grep might exist under unexpected names; the strings files and the ui/ file list were scanned, but not every Kotlin file was read.
