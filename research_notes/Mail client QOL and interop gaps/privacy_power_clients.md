# Privacy and power-user features of FairEmail, Thunderbird for Android (ex K-9), Proton Mail, Tuta, Thunderbird desktop

Research date: 2026-11-25. Method note: the FairEmail FAQ is ~372k characters; the fetch tool read it in 100k chunks via a summarising model, so some FAQ items (notably 30 quick settings, 82 tracking image, 35/182 punycode details, 62 authentication) were only partly retrieved. Anything not confirmed is listed under Gaps.

FAQ URL used throughout: https://raw.githubusercontent.com/M66B/FairEmail/master/FAQ.md (also https://github.com/M66B/FairEmail/blob/master/FAQ.md). README: https://raw.githubusercontent.com/M66B/FairEmail/master/README.md

## 1. Link safety (confirmation dialog, tracking parameters, redirect unwrapping, custom tab vs browser)

### Takeaway
FairEmail shows a confirmation dialog on every link tap (default on) with per-domain "do not ask again", a "remove tracking parameters" option (UTM only per its FAQ) and optional Disconnect/AdGuard lists; Proton does link confirmation on all platforms but strips tracking parameters only on web. I found no source confirming IDN/punycode warnings or Outlook SafeLinks/Google redirect unwrapping in FairEmail; that is a gap and an opportunity.

### Cited Findings
- FairEmail privacy features listed in README: "Confirm opening links to prevent tracking and phishing", "Confirm showing images to prevent tracking", "Reformat messages to prevent phishing", "Attempt to recognize and disable tracking images", "Warning if messages could not be authenticated". — [FairEmail README](https://raw.githubusercontent.com/M66B/FairEmail/master/README.md)
- FAQ 182: tapping a link shows a confirmation dialog listing available browsers. Browsers supporting Custom Tabs appear twice: one entry with an "open external" icon runs standalone, the other runs embedded (looks like part of the app). "Select app"/"Open with" hand the link to Android (default browser; Android asks Always/Just once if several apps match). — [FAQ](https://raw.githubusercontent.com/M66B/FairEmail/master/FAQ.md)
- FAQ 182: ticking "Do not ask this again for [domain]" is per-domain and can be undone via "Reset questions" in miscellaneous settings; the whole dialog is disabled by turning off "Confirm opening links" in privacy settings; long-pressing a link still shows it. — [FAQ](https://raw.githubusercontent.com/M66B/FairEmail/master/FAQ.md)
- FAQ 118: "Remove tracking parameters" removes all UTM parameters from a link (the FAQ text retrieved mentions UTM only, not fbclid/gclid). — [FAQ](https://raw.githubusercontent.com/M66B/FairEmail/master/FAQ.md)
- FAQ 35 mentions disabling "Remove tracking parameters by default", implying a by-default option exists; a third-party 2026 blog (derived from FAQ/source, not primary) says the by-default switch ships OFF while "Confirm opening links" ships ON. — [FAQ](https://raw.githubusercontent.com/M66B/FairEmail/master/FAQ.md); [gblock.app article](https://www.gblock.app/articles/is-fairemail-tracking-your-email-block-2026)
- FAQ 200 exists titled "How can I use Adguard to remove tracking parameters?" (text truncated in my fetch); the same third-party blog says an AdGuard filter list drives suggestions in the dialog and has "over 2,000 entries" (unverified against FAQ). — [FAQ](https://raw.githubusercontent.com/M66B/FairEmail/master/FAQ.md); [gblock.app article](https://www.gblock.app/articles/is-fairemail-tracking-your-email-block-2026)
- FAQ 159: Disconnect.me tracker-protection lists are downloaded in privacy settings and used to warn about tracking links and recognise tracking images; images whose domain is classified "Content" are not treated as trackers; weekly list updates suggested; an intent `eu.faircode.email.DISCONNECT.ME` triggers an update. — [FAQ](https://raw.githubusercontent.com/M66B/FairEmail/master/FAQ.md)
- FairEmail's privacy page says the app looks up the IP network address of link domains upon pressing a button in the link confirmation dialog (a user-initiated lookup, not automatic). — via search summary of [FairEmail privacy page](https://email.faircode.eu/privacy)
- FAQ 208 exists: "What does 'about:blank#blocked' mean when I click on a link?" (a Chrome/Android block of a link scheme; body not retrieved). — [FAQ](https://raw.githubusercontent.com/M66B/FairEmail/master/FAQ.md)
- Proton: link confirmation is a setting, enabled by default, asking before opening an external link from an email; offered on all devices. — [Proton link confirmation](https://ProtonMail.com/support/link-confirmation)
- Proton tracking-link protection ("link cleaning") removes known tracking parameters (UTM plus some ad-company custom ones) from a blocklist compiled from internal and community sources; web app only per the blog; link left intact if stripping would break it; shield icon shows counts; Proton calls it "cat and mouse". — [Proton blog](https://proton.me/blog/tracking-links-protection); [Proton support](https://proton.me/support/email-tracker-protection) ("Mobile apps only block spy pixels")
- Thunderbird desktop: link-text vs real-host mismatch is part of its scam detection (numeric-IP links and mismatched hosts), preference `mail.phishing.detection.mismatched_hosts`; newer versions show the warning after the link is clicked rather than before; frequent false positives on newsletter unsubscribe links. — [Mozilla scam detection](https://support.mozilla.org/kb/thunderbirds-scam-detection); [Bugzilla 623198](https://bugzilla.mozilla.org/show_bug.cgi?id=623198)

### Inferences
- A client author can exceed FairEmail: show the full URL and registrable domain, flag IDN/punycode and mixed-script hosts, strip a broader parameter list (utm_*, fbclid, gclid, mc_eid etc.), and unwrap known wrappers locally (no network) before display. None of these were confirmed as present in FairEmail from the sources I could read.
- Link text vs target mismatch should be shown inside the dialog rather than as a blocking warning, given Thunderbird's false-positive history.
- Per-domain "don't ask again" plus a reset action is the established pattern.

### Gaps
- Whether FairEmail shows punycode/IDN warnings, unwraps SafeLinks/Google redirects, or strips fbclid/gclid: not found in retrieved text.
- Exact FAQ 35/182 wording on the dialog's displayed fields (domain highlighted etc.) not retrieved.

## 2. Tracking (pixels, per-sender/domain remote content, CSS/font blocking)

### Takeaway
FairEmail asks before loading images (default), offers per-sender/domain allow ("Do not ask again"), recognises tracking images via Disconnect lists; Proton blocks spy pixels, proxies other images, and shows counts; Tuta blocks all external content by default with an allow-list; Thunderbird desktop blocks remote content with sender/domain allow.

### Cited Findings
- FairEmail FAQ 35: viewing remote images and opening links can tell the sender you saw the message and leak your IP; "Do not ask this again for ..." allows images/original per trusted sender case by case; reset button in miscellaneous settings; FairEmail downloads remote images directly from the source server (the FAQ argues Gmail-style proxying is not more secure). — [FAQ](https://raw.githubusercontent.com/M66B/FairEmail/master/FAQ.md)
- FairEmail FAQ 27: icons for external, embedded and broken images; downloading external images "can be used to record you did see a message". — [FAQ](https://raw.githubusercontent.com/M66B/FairEmail/master/FAQ.md)
- FairEmail settings include showing images for known contacts and selecting domains for images (FAQ titles 27, 35, 82). — [FAQ](https://raw.githubusercontent.com/M66B/FairEmail/master/FAQ.md)
- FairEmail FAQ 159: tracking images disabled only if the main "disable" option is also enabled; Content-classified domains are exceptions. — [FAQ](https://raw.githubusercontent.com/M66B/FairEmail/master/FAQ.md)
- FairEmail FAQ 35: reformatted view strips margins and standardises fonts and colours, which also reveals phishing links; FAQ 43 "Show original" displays the sender's HTML unaltered except `TEXT_AUTOSIZING`; FAQ 35 warns opening attachments or the original may load remote content and run scripts. — [FAQ](https://raw.githubusercontent.com/M66B/FairEmail/master/FAQ.md)
- FairEmail suggests the Email Privacy Tester for checking leaks (FAQ 32). — [FAQ](https://raw.githubusercontent.com/M66B/FairEmail/master/FAQ.md)
- Proton (enabled by default; setting does not sync across devices): removes known tracking pixels from non-E2E mail; other remote images are loaded through Proton's proxy at delivery time (not at open), generic IP/location; Android/iOS toggle at Settings > Privacy and security > "Block email tracking"; remote image auto-load separately switchable; mobile UI: tap the down arrow next to "To:" to see counts, "Learn more". — [Proton support](https://proton.me/support/email-tracker-protection)
- Tuta: images do not load automatically; an allow-list lets trusted senders' images load. — [Tuta feature list](https://tuta.com/blog/posts/development-feature-list); [Tuta support security](https://tuta.com/support/security)
- Thunderbird desktop: remote-image blocking is separate from scam warning; allow by sender or by the domain the images come from. — [Mozilla scam detection thread results](https://support.mozilla.org/kb/thunderbirds-scam-detection) (search summary)

### Inferences
- Expected UI contract: per-message "Show images once", "Always from sender", "Always from domain", a reset-all; a tracker count is a valued detail (Proton). Default off for remote content.
- No source retrieved addresses CSS/web-font blocking specifically; blocking all remote loads implicitly covers it.

### Gaps
- FAQ 82 ("What is a tracking image?") body not retrieved: no details on detection heuristics (1x1 size, known tracker domains).
- Whether FairEmail shows a counted tracker badge: not confirmed.

## 3. Phishing/spoof warnings

### Takeaway
FairEmail marks failed DKIM/SPF/DMARC with a red/orange header stripe; Thunderbird desktop has a "may be a scam" bar; Tuta has a report-based phishing banner. Display-name vs address, reply-to mismatch, lookalike domains, first-time-sender: not confirmed in any retrieved source.

### Cited Findings
- FairEmail FAQ 94: "The red/orange stripe at the left side of the header means that the DKIM, SPF or DMARC authentication failed"; unauthenticated messages may land in spam. — [FAQ](https://raw.githubusercontent.com/M66B/FairEmail/master/FAQ.md)
- FairEmail FAQ 176: a message counts as transport-secure only if every Received header meets the listed criteria. — [FAQ](https://raw.githubusercontent.com/M66B/FairEmail/master/FAQ.md)
- FairEmail rules can test `onBlocklist()`, `hasMx()`, `knownContact()`, header contents, and spam block lists (Spamhaus, Spamcop, Barracuda individually toggleable since 1.1627). — [FAQ](https://raw.githubusercontent.com/M66B/FairEmail/master/FAQ.md)
- Thunderbird desktop scam detection triggers on numeric-IP links and link text naming a different server than the destination; bar reads "This message may be a scam"; can be disabled at own risk. — [Mozilla scam detection](https://support.mozilla.org/kb/thunderbirds-scam-detection)
- Tuta shows a phishing banner when parts of a mail match phishing signatures derived from user reports; the user can mark "not phishing". — [Tuta why-tutanota/support search summary](https://tuta.com/support/security) (marketing-grade source; undated)
- Proton scans incoming unencrypted mail for spam, phishing, malware server-side. — [Proton support](https://proton.me/support/email-tracker-protection)

### Inferences
- Local, server-independent checks (reply-to domain differs from From domain, display name containing a different address, first message ever from an address, confusable domains) are a differentiator but I have no evidence competitors do them; treat as design suggestions.

### Gaps
- No confirmed source for display-name/address mismatch, Reply-To mismatch, "first time sender" or external-sender banners in these clients.

## 4. Reading tools

### Takeaway
FairEmail: reformatted view default, "Show original" on demand, print via Android, character set fallback ISO-8859-1. Thunderbird Android is behind (reading improvements scheduled 2027).

### Cited Findings
- FairEmail FAQ 43: original view unaltered except TEXT_AUTOSIZING. — [FAQ](https://raw.githubusercontent.com/M66B/FairEmail/master/FAQ.md)
- FairEmail FAQ 197 (print): via the three-dots button above the message text; Android's print framework handles paper/printer; prints as-is with sender's margins; a print button can be added to the action bar. — [FAQ](https://raw.githubusercontent.com/M66B/FairEmail/master/FAQ.md)
- FairEmail FAQ 140: with no charset or US-ASCII, ISO 8859-1 is assumed; otherwise no reliable detection. — [FAQ](https://raw.githubusercontent.com/M66B/FairEmail/master/FAQ.md)
- FairEmail FAQ 71 condition "Header contains" requires header download enabled in connection settings (headers not downloaded by default). — [FAQ](https://raw.githubusercontent.com/M66B/FairEmail/master/FAQ.md)
- FairEmail message preview shows 1 to 4 lines, only if the body was downloaded; larger bodies are not pre-downloaded on metered connections by default (size threshold configurable). — [FAQ](https://raw.githubusercontent.com/M66B/FairEmail/master/FAQ.md)
- Thunderbird Android: Plain text view was added in beta 16.0b1; split screen exists under Settings > General > Display. — [Mozilla support threads](https://support.mozilla.org/ne-NP/questions/thunderbird-android)
- Thunderbird Android roadmap: Message View Phase 1 (inline/bottom attachments, text and HTML/CSS rendering fixes) shipped June 2026; Message List improvements, Message View Phase 2 (attachments, loading, zoom, removing images) and JMAP exploration "now planned for 2027". — [Thunderbird roadmap](https://roadmaps.thunderbird.net/en-US/android)

### Inferences
- Gap among competitors: no confirmed "view headers/raw source / save .eml" for FairEmail beyond TOC titles; likely present but not verified.

### Gaps
- FairEmail raw headers / save .eml, quote collapsing settings, fit-to-width behaviour, width/font options: not retrieved in detail.
- Thunderbird desktop view-source/encoding options: Mozilla support page failed to load.

## 5. Triage and compose power features

### Takeaway
FairEmail's rules engine (conditions, expression language, many actions including webhook and snooze) and local snooze, templates, per-identity settings are the benchmark; Thunderbird Android is far behind.

### Cited Findings
- Rules (FAQ 71): edited by long-pressing a folder; run on newly received mail only (manual "Execute now"); order, stop-processing, groups (1.2061), daily run on older mail (1.2018). Conditions: sender/reply-to, recipient, subject, attachment (type), header, text, absolute/relative time, expression; all must match; plain text match case-insensitive, regex case-sensitive full-string, NOT for regex since 1.2173, `jsoup:` selector for HTML. Expression variables/functions include `header()`, `onBlocklist()`, `hasMx()`, `knownContact()`, `Is("flag")`, `AI(prompt)` (1.2174). Actions: read/unread, hide, silence, snooze, star, importance, keyword, local note, move/copy (with `$year$ $month$ $domain$` placeholders), delete permanently, sound, text-to-speech, reply/forward with template, automation broadcast intent, webhook (1.2107, not retried). `$Filtered$` keyword set after rules. POP3 unsupported for keywords/move. Rules are a pro feature. — [FAQ](https://raw.githubusercontent.com/M66B/FairEmail/master/FAQ.md)
- Snooze (FAQ 67/166): hides the conversation until the time then shows a new-mail notification as reminder; "Filter out > Hidden" to view; zero duration cancels; rules can snooze; snooze is local to each device because server-side hiding is impossible; Android 12+ uses exact alarms; Planned-features text says snooze was implemented by adding a time condition to rules. — [FAQ](https://raw.githubusercontent.com/M66B/FairEmail/master/FAQ.md)
- Templates (FAQ 179): defined in the main menu; options Default, Use as read receipt, Favorite, Snippet (1.1857), Hide; inserted from editor overflow menu; automation intent `eu.faircode.email.TEMPLATE` (1.2068, not in Play build). — [FAQ](https://raw.githubusercontent.com/M66B/FairEmail/master/FAQ.md)
- Identities: default CC/BCC in advanced identity settings; placeholders like `$from$`, `$domain$`; one primary account and one primary identity per account used when ambiguous (FAQ 72); signatures via identity settings. — [FAQ](https://raw.githubusercontent.com/M66B/FairEmail/master/FAQ.md)
- Undo/delayed send: swipe an Outbox message to undo; "Send messages after selected time" is a pro feature. — [FAQ/README](https://raw.githubusercontent.com/M66B/FairEmail/master/README.md)
- Read receipts (FAQ 125, experimental, 1.1797+): sets `$Displayed`/`$Delivered` keywords if server supports IMAP keywords. — [FAQ](https://raw.githubusercontent.com/M66B/FairEmail/master/FAQ.md)
- Auto-expunge (FAQ 134/160): can be disabled only via debug panel; then \Deleted messages stay and are expunged by long-press on a folder. Permanent delete always confirms (can be disabled per message since 1.1601). Auto deletion (FAQ 191/194): unread, starred, snoozed and under-24-hour messages are never auto-deleted; trash/spam deleted permanently. — [FAQ](https://raw.githubusercontent.com/M66B/FairEmail/master/FAQ.md)
- Message classification (local learning): thresholds default minimum class probability 15% and minimum difference 50%; never moves out of spam; rules run first. — [FAQ](https://raw.githubusercontent.com/M66B/FairEmail/master/FAQ.md)
- Other FairEmail items: colored stars, account/identity/folder colors/avatars, per-account/folder/sender notification sounds, configurable notification actions, sync scheduling (workday/weekend; end time <= start means next day; since 1.1880 accounts can be excluded), unified inbox, conversation threading, search index (pro, costs battery/storage), calendar invites, vCard attachments. — [README](https://raw.githubusercontent.com/M66B/FairEmail/master/README.md); [FAQ](https://raw.githubusercontent.com/M66B/FairEmail/master/FAQ.md)
- Password-protected messages (FAQ 184): 1,500 byte limit, AES-GCM with PBKDF2 key. Encryption (FAQ 12): OpenPGP via OpenKeychain/PGPony, S/MIME from Android keystore; "Sign by default", "Encrypt by default", "Automatically decrypt" settings; decrypted content stored locally only. — [FAQ](https://raw.githubusercontent.com/M66B/FairEmail/master/FAQ.md)
- Tuta: scheduled send, undo send, aliases, signatures, conversation grouping, local full-text search, offline mode (2022), client-side spam filtering (March 2026), key verification (2025), TutaCrypt post-quantum (all accounts March 2026), 2FA/FIDO U2F. — [Tuta feature list](https://tuta.com/blog/posts/development-feature-list)
- Thunderbird Android open issues sorted by reactions include: JMAP support (#3272), IMAP labels (#769), S/MIME (#1003), threaded view across folders (#735), identity selection/catch-all (#935), different SMTP per identity (#1117), signature editor (#8841), iCalendar invitations (#930). — [GitHub issues](https://github.com/thunderbird/thunderbird-android/issues?q=is%3Aissue+is%3Aopen+sort%3Areactions-%2B1-desc) (page order; counts shown are likely comment counts, not reactions)
- Thunderbird Android 2026: HTML signatures shipped September 2026; Thundermail QR login June 2026; S/MIME not on 2026 roadmap; tablet views not planned. — [Roadmap](https://roadmaps.thunderbird.net/en-US/android); [heise/2026 roadmap summary](https://blog.thunderbird.net/2025/12/state-of-the-thunder-14-the-2026-mobile-roadmap/)

### Inferences
- A "rules on device vs server" choice matters: FairEmail rules are client-side and only for new mail; a JMAP/Sieve client could offer both.
- Snooze on JMAP could be server-side via keywords/mailbox with `$snoozed`-style handling; FairEmail's local-only snooze is a known limitation (multi-device).

### Gaps
- BCC-self, keyword/label colour specifics for Thunderbird Android, delayed send in Thunderbird Android: not retrieved.

## 6. App-level (lock, screenshots, notification privacy, widgets, tiles, battery, offline)

### Takeaway
FairEmail has pro biometric/PIN lock hiding notification content and recents; Tuta has PIN/biometric; Thunderbird Android has no app lock (long-standing request). Lock-screen notification levels exist in K-9.

### Cited Findings
- FairEmail FAQ 113: biometric/PIN auth toggled from the settings navigation menu; prompts after an inactivity period (set in miscellaneous settings) or screen off; when enabled notifications show no content and the app is hidden from Android recents; pro feature. — [FAQ](https://raw.githubusercontent.com/M66B/FairEmail/master/FAQ.md)
- FairEmail widgets: unread-count widget counts only unread mail in folders with new-message notifications enabled (FAQ 106/149); message-list widget is a pro feature. — [FAQ](https://raw.githubusercontent.com/M66B/FairEmail/master/FAQ.md); [README](https://raw.githubusercontent.com/M66B/FairEmail/master/README.md)
- FairEmail permanent notification (FAQ 2): needed for background receive; can be hidden via the "Monitoring" channel on Android 8+, or use periodic sync; battery optimisation off recommended (FAQ 39); manual sync option per account (FAQ 79); offline storage and operations. Preview text can be hidden (FAQ 21/145). — [FAQ](https://raw.githubusercontent.com/M66B/FairEmail/master/FAQ.md)
- FairEmail settings export/import (backup tab), cloud sync end-to-end encrypted (FAQ 189, not in Play build); Android Auto in GitHub build only. — [FAQ](https://raw.githubusercontent.com/M66B/FairEmail/master/FAQ.md)
- K-9 lock-screen notification options: none, app name only, message count (default), count with senders, same as unlocked; Delete action on notifications off by default. — [K-9 docs](https://docs.k9mail.app/en/beta/settings/general)
- Thunderbird Android: no biometric app lock; Mozilla Connect thread requests optional lock, auto-relock timeout, no content on lock screen; linked GitHub issues #7529 and #7530; suggestion to use BiometricPrompt with Keystore-bound keys. — [Mozilla Connect](https://connect.mozilla.org/t5/ideas/biometrics-to-open-thunderbird-android/idi-p/74741)
- Thunderbird Android: Mozilla Connect idea "Bring all features in FairEmail to Thunderbird Android" exists (page returned 403; contents not read). — [Mozilla Connect](https://connect.mozilla.org/t5/ideas/bring-all-features-in-fairemail-to-thunderbird-android/idc-p/82126)
- Tuta: PIN and biometric unlock; Android push via own notification service, no Google dependency; notification previews with quick actions; calendar widgets (no mail widget listed). — [Tuta feature list](https://tuta.com/blog/posts/development-feature-list); [Tuta F-Droid](https://f-droid.org/packages/de.tutao.tutanota/)

### Inferences
- Screenshot blocking (FLAG_SECURE) tied to app lock/recents hiding is implied by FairEmail ("hidden from recents") but not explicitly confirmed as FLAG_SECURE.

### Gaps
- Quick Settings tile details (FAQ 30) not retrieved.
- Screenshot blocking as separate setting: unconfirmed.

## 7. What users most often request or praise

### Takeaway
Evidence is thin: no vote-ranked list retrieved. Visible demand: biometric app lock, S/MIME, JMAP, IMAP labels, cross-folder threads, per-identity SMTP, signature editor in Thunderbird Android; FairEmail is repeatedly held up as the feature benchmark.

### Cited Findings
- FairEmail FAQ has a "Frequently requested features" section (pinch-zoom text size, GIF display, themes, day-time condition for rules, per search snippets); rule time conditions later shipped. — [FAQ](https://github.com/M66B/FairEmail/blob/master/FAQ.md)
- Thunderbird Android requests listed in the Section 5 issue list, plus app-lock thread in Section 6. — [GitHub issues](https://github.com/thunderbird/thunderbird-android/issues?q=is%3Aissue+is%3Aopen+sort%3Areactions-%2B1-desc); [Mozilla Connect](https://connect.mozilla.org/t5/ideas/biometrics-to-open-thunderbird-android/idi-p/74741)
- Thunderbird Android roadmap says many low ratings stem from notifications and synchronisation; this is the top engineering concern. — [Roadmap](https://roadmaps.thunderbird.net/en-US/android)
- Thunderbird roadmap statement: "S/MIME is not on the roadmap for 2026 but we agree, it's important." — [Thunderbird State of the Thunder 14](https://blog.thunderbird.net/2025/12/state-of-the-thunder-14-the-2026-mobile-roadmap/)

### Inferences
- Reliability of notifications/sync is the dominant user concern for Thunderbird Android; for a Stalwart/JMAP client that is already strong (push), so privacy tooling is the differentiator.

### Gaps
- No upvote data from FairEmail issues, r/fairemail, r/Thunderbird, or r/privacy retrieved; the Mozilla Connect FairEmail thread blocked (403).
- Nothing retrieved on the K-9 to Thunderbird transition beyond roadmap context (rebrand occurred 2024; not sourced here).
