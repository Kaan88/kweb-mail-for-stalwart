# Closing kweb mail's biggest outgoing and daily-use gaps

kweb mail's biggest gap is how its mail looks in other people's clients. Every ordinary message goes out as **plain text only**, with a date-less "Name wrote:" line and `> ` quotes. It has no HTML part and no Gmail-style quote markup, so Gmail and Outlook users get less folding and less polish than they get from other senders. The app builds this in `core/src/jmap/send.rs` (lines 596-628) and `core/src/compose/rules.rs` (lines 209-230). The cheapest fixes are in the same area: an "On <date>, <name> wrote:" line, one clean `Re:` for localised prefixes (AW:, SV:, Ynt:, RE[2]:), and a proper forward header. On the reading side, the main gaps are "show original / headers", print or save as `.eml`, link safety (show the real address, strip tracking parameters) and a `mailto:` handler. On the Android side, the main gaps are Direct Reply, a mail widget and app shortcuts, a mail-list sort order and search operators. Many popular ideas (snooze, mute, pin, widgets, quiet hours, tablet layout) are already on the owner's "maybe" lists in PLAN.md. Those are marked "already planned" below. Source quality is mixed. The outgoing-markup facts come from open-source parsers and bug trackers, not from Google or Microsoft. The notes had no direct user-sentiment data. Ranking is my judgement.

## How this was checked

I read six research notes and checked their claims about kweb against the repository, PLAN.md and CLAUDE.md. A spot check found no HTML send path, `RemoteInput`, `SENDTO` filter, biometric prompt, shortcut or widget code in `core/src/jmap/send.rs`, `core/src/compose/` or the Android main source. The only `text/html` and `gmail_quote` hits are on the reading side (quote folding, the WebView, URL policy). Nothing was built or run. "Present in source" does not mean seen on a device, and PLAN.md says several screens were never opened on one. Items the app already has were dropped. Items PLAN.md lists as "maybe", "later" or "for later consideration" are marked "already planned". Items the owner rejected are in the last section.

Effort guesses: small is days or less, medium is about a week or two, large is several weeks or needs new server work.

## Top list, ranked by value

1. **HTML part with Gmail-style quote markup in replies and forwards (large).** See A1.
2. **Date line, single `Re:` and a fuller forward header (small).** See A2, A3 and A4. These make plain-text mail look normal in Gmail and Outlook.
3. **Show original, headers and save as `.eml` (small to medium).** See B1. Not on any plan list.
4. **Link safety: show the real address, strip tracking parameters, flag odd hosts (medium).** See B2. The owner lists the first two as "maybe".
5. **Direct Reply from a notification (small to medium).** See E1. Not planned.
6. **`mailto:` handler, so the app can be the default mail app (small).** See E2. Not planned.
7. **Search operators and saved searches (medium).** See C3. Not planned.
8. **Snooze (medium to large).** See C1. Already planned as "maybe". No standard JMAP snooze exists, so it needs a design choice.
9. **Templates and canned replies (medium).** See D1. Marked "later, not planned" in PLAN.md.
10. **Forgot-attachment and empty-subject warnings (small).** See D2. Not planned.
11. **Rich-text compose with HTML signatures (large).** See D3. It goes with item 1.
12. **Read receipts, using Stalwart 1.2 MDN methods (medium).** See F1 and A7. Already planned as "maybe". Needs Stalwart 1.2.
13. **Home-screen widget and app shortcuts (medium).** See E3. The widget is already planned as "maybe". Shortcuts are not planned.
14. **App lock with biometrics (medium).** See F2. Already planned as "for later consideration".
15. **Accept an invitation or add a contact from an `.ics` or `.vcf` attachment, and answer an invitation from the mail itself (medium).** See B4. Already planned as "for later consideration".

## A. How outgoing mail looks in other clients

**A1. HTML alternative with Gmail-style quote markup.** Gmail writes replies as `div.gmail_quote` with a `div.gmail_attr` line and a `blockquote.gmail_quote` ([dead-letter #193](https://github.com/BigCactusLabs/dead-letter/issues/193), [google_workspace_mcp PR 1238](https://github.com/taylorwilsdon/google_workspace_mcp/pull/1238)). Outlook on the web uses `#appendonsend` and `#divRplyFwdMsg` ([zapdesk #595](https://github.com/knowall-ai/zapdesk/issues/595)). Thunderbird and Apple Mail use `blockquote type="cite"` ([Mozilla bug 183219](https://bugzilla.mozilla.org/show_bug.cgi?id=183219)). None of this is documented by Google or Microsoft, so a real capture must be tested before relying on it. A combined recipe is `div.gmail_quote` > `div.gmail_attr` + `blockquote.gmail_quote type="cite"`, sent as `multipart/alternative` with the plain part kept. kweb sends `text/plain` only and flattens the original HTML to text (`core/src/compose/mod.rs:38-45`, `rules.rs:305-340`), so tables, links and images in the quoted mail are lost. Gmail also seems to fold history only when the quote matches the thread ([Labnol](https://labnol.org/code/19938-show-trimmed-content-gmail)), so the quote must be the real previous body. Effort: large. The message must go as an HTML body part through JMAP `bodyValues` and `bodyStructure`. Quoting the original HTML needs a sanitiser. Signed and encrypted mail is built by hand in `core/src/crypto/build.rs` and would need its own decision. No Stalwart dependency is known.

**A2. Attribution line with a date.** Gmail, Outlook and Thunderbird write "On <date>, <name> <addr> wrote:" ([Mozilla bug 1392064](https://bugzilla.mozilla.org/show_bug.cgi?id=1392064)). kweb writes `Name <addr> wrote:` with no date, and the display name is not escaped, so `Doe, Jane <x> wrote:` is possible (`rules.rs:209-230`). For Outlook, a From/Sent/To/Subject block also helps its tools find history ([emailforagents.ai](https://emailforagents.ai/blog/strip-quoted-reply-text-for-agents)). Effort: small. Use a localised, short date in the user's time zone.

**A3. One clean subject prefix.** Outlook, Thunderbird and aerc treat AW, SV, VS, WG and similar as reply markers. In mixed threads they pile up as "Re: AW: SV:" ([Office Watch](https://office-watch.com/?p=2081), [Mozilla bug 415655](https://bugzilla.mozilla.org/show_bug.cgi?id=415655)). kweb only knows `Re:` and `Fwd:`/`Fw:` (`rules.rs:14-45`), so "AW: Termin" becomes "Re: AW: Termin" and "RE[2]: x" gets a second prefix. An empty-subject reply becomes `Re:`. Threading by References is not affected. Effort: small. Add a strip list (AW, SV, VS, WG, Antw, Ynt, İlt, RE[n]); the Turkish forms come from reasoning, not from a source.

**A4. Forward header like Gmail and Outlook.** Gmail uses "---------- Forwarded message ---------" plus From, Date, Subject, To. Outlook uses a bold From/Sent/To/Subject block. kweb already uses the Gmail separator, but the date is ISO-8601 UTC (for example `2026-01-02T03:04:05Z`), there is no Cc line, and inline `cid:` pictures are dropped (`rules.rs:232-245`, `mod.rs:51-58`). Effort: small for the header. Inline pictures are medium.

**A5. Forward as attachment (`message/rfc822`).** Thunderbird, Apple Mail and Outlook offer it, and it keeps the original headers and attachments exactly. Evidence for client behaviour is thin in the notes (they list this as a gap). kweb has only an inline forward (`compose/mod.rs`). Effort: medium. JMAP can attach an existing message blob, so this should not need server work.

**A6. Wrapping of plain text.** Outlook, Apple Mail and Gmail do not send `format=flowed`. Fastmail dropped it ([Fastmail blog](https://fastmail.com/blog/format-flowed)). K-9 does not hard-wrap either ([K-9 forum](https://forum.k9mail.app/t/k-9-mail-is-not-sending-format-flowed-for-plain-text-nor-hard-wrapping/1086)). kweb never wraps, so the wire result depends on Stalwart, and nothing in the repo shows what it does. Effort: small, but first send one mail through the local test server and look at the bytes. This may turn out to be a non-issue. If item A1 ships, the HTML part makes it matter much less.

**A7. Receiving side of read receipts and a Content-Language header.** kweb sends no `Disposition-Notification-To`, no `Content-Language`, and answers no incoming receipt requests. A receipt must be opt-in and never automatic when the address differs from Return-Path ([RFC 8098](https://www.rfc-editor.org/rfc/rfc8098)). Effort: medium. Needs Stalwart 1.2, which added `MDN/send` and `MDN/parse` ([Stalwart releases](https://github.com/stalwartlabs/stalwart/releases)). Already planned as "maybe" (read receipts).

**A8. Invitations that other clients can answer.** kweb cannot create events with guests, so no invitation leaves the app; the reply to an invitation is a JMAP patch with `sendSchedulingMessages`, and Stalwart writes the iMIP mail (`core/src/jmap/calendars.rs:227-264`). Whether Gmail and Outlook show Accept and Decline on that mail depends on Stalwart's output, which was not tested. Effort: medium to large. Attendees are marked "later" in PLAN.md. The notes say Gmail needs a valid `mailto:` for ORGANIZER and ATTENDEE and a `method` parameter on the `text/calendar` part ([CalConnect](https://devguide.calconnect.org/imip/best-practices/)); the Gmail-specific advice is anecdotal.

## B. Reading and safety

**B1. Show original, full headers, save or share `.eml`, print or PDF.** FairEmail has "Show original", print through Android's print framework, and a reformatted view that exposes phishing links ([FairEmail FAQ](https://github.com/M66B/FairEmail/blob/master/FAQ.md)). Gmail and Outlook both have show-original or view-headers. kweb has none of these in the thread menu (`R/strings_*`), though the core keeps the raw blob id. PLAN.md lists only "print or save as PDF or `.eml`" as a "maybe". Effort: small for headers and `.eml` (a blob download), small to medium for print.

**B2. Link safety.** FairEmail asks before opening a link, with per-domain "do not ask again" and a reset. It can strip UTM parameters, and it uses Disconnect and AdGuard lists ([FAQ](https://github.com/M66B/FairEmail/blob/master/FAQ.md)). Proton asks by default and strips tracking parameters on web only ([Proton blog](https://proton.me/blog/tracking-links-protection)). Thunderbird flags a link whose text names a different host, but it has many false positives on unsubscribe links ([Mozilla](https://support.mozilla.org/kb/thunderbirds-scam-detection)). kweb allows only http, https and mailto and opens a link on tap (`ui/thread/UrlPolicy.kt`), with no preview of the destination and no stripping. PLAN.md lists "show where a link really goes" and "strip tracking parameters" as "maybe". Extras the notes suggest: flag punycode and mixed-script hosts, and unwrap SafeLinks and Google redirect wrappers locally. FairEmail was not confirmed to do these. Show a text/target mismatch inside the dialog, not as a blocking warning. Effort: medium.

**B3. Tracker counts and tracking-pixel detection.** Proton shows how many trackers it blocked. FairEmail recognises tracking images with Disconnect lists. kweb blocks all remote content until the user taps Show, which is safe but gives no information (`core/src/reading.rs`). Effort: medium for a heuristic count (1x1 images, known domains). A downloaded list needs a network fetch, so decide that carefully.

**B4. Add an event or contact from an attachment, and answer an invitation from its mail.** FairEmail handles calendar invites and vCards. Gmail and Outlook show RSVP buttons in the mail. kweb has the calendar and contact screens but no link from an `.ics` or `.vcf` attachment (PLAN.md M10, "for later consideration"; PLAN.md:176 says answering from the mail is not built). Effort: medium.

**B5. Translate a received message.** Gmail, Outlook and others offer it. kweb has Translate only in compose (`ui/compose/TranslateRules.kt`). The thread assistant menu has Summarize, Explain, Find dates, Reply ideas, Draft reply and Ask about. Effort: small, because the assistant and its consent rules exist. It needs the user's own AI service, so it is optional.

**B6. Local phishing hints.** FairEmail shows a stripe for failed DKIM, SPF or DMARC, and Thunderbird shows a scam bar. kweb has a server-checked sender mark (`core/src/sender_check.rs`). The notes found no competitor that warns on a different Reply-To domain, a display name that holds another address, or a first-time sender, so this is a design idea and not a gap against competitors. Effort: small to medium.

**B7. Force-dark and text controls for HTML mail.** The notes found no separate "force dark mail" switch. Reading has text size and fit-to-width already (`FitRules.kt`). Effort: small to medium. Low value until a user asks.

## C. Triage and organising

**C1. Snooze.** Gmail, Outlook (as "Schedule"), Apple, Spark and Fastmail all have it ([Fastmail help](https://www.fastmail.help/hc/en-us/articles/360058753634)). Fastmail's is server-side and does not work in third-party clients. The IETF snooze draft (`urn:ietf:params:jmap:mail:snooze`) is expired ([draft](https://datatracker.ietf.org/doc/html/draft-ietf-extra-email-snooze)). RFC 9979 only registers the `Snoozed` mailbox name ([RFC 9979](https://www.rfc-editor.org/rfc/rfc9979.txt)). The notes did not find whether Stalwart supports the draft. Test that on the local server first. FairEmail's snooze is local, which fails on a second device, and a lost snooze is worse than a lost push, as Fastmail notes. kweb has no mail snooze, only a 10-minute snooze on reminders. Already planned as "maybe". PLAN.md notes it could be late in Google push mode. Effort: medium if client-side with a folder or keyword and a scheduled wake; large if it uses a server feature.

**C2. Mute, pin, send-and-archive, "All inboxes".** Gmail mutes a thread and silences its later replies. Spark pins. All of these are already planned as "maybe" (PLAN.md M10, borrowed from other apps). The unified inbox is a real change, because kweb runs one core per account. Effort: small each for mute and send-and-archive, medium for pin, large for All inboxes.

**C3. Search operators, filter chips from a query, saved searches, history.** Gmail and Spark take operators or natural language. kweb passes one string to the server's `text` filter and has three fixed chips: Unread, Flagged, Attachments (`core/src/ffi.rs:548`). It also has no saved or recent searches, no attachment-text search and no cross-account search. The notes did not check how the server's text filter treats operator-like strings. Effort: medium. Parse `from:`, `to:`, `subject:`, `has:attachment`, `is:unread`, `before:`/`after:` into the JMAP filter. Not planned. Attachment text is already "later, not planned".

**C4. Sort order and list options.** The notes found no sort setting; lists are newest first. Effort: small. Low to medium value.

**C5. Subscription manager.** Gmail lists subscriptions by sender, sorted by recent volume, with one-tap unsubscribe ([Google blog](https://blog.google/products-and-platforms/products/gmail/new-manage-subscriptions-unsubscribe/)). kweb already has one-tap unsubscribe (`core/src/unsubscribe.rs`) but no list. A client can build it from `List-Unsubscribe` headers and counts, from the cache. Effort: medium.

**C6. Rules that also run on the phone, with more conditions.** FairEmail's rules are client-side, with expressions, time conditions and actions like snooze and webhook ([FAQ](https://github.com/M66B/FairEmail/blob/master/FAQ.md)). kweb has a full visual Sieve editor, so mail filtering is covered on the server. Stalwart 1.2 also compiles server-side mail rules into Sieve ([releases](https://github.com/stalwartlabs/stalwart/releases)). A gap is only run-on-old-mail, which is niche. Effort: medium. Low value.

**C7. Sender-level actions.** Apple Mail has "Categorize Sender" and "Trash All" from a sender. kweb has block sender and "Filter mail like this". Effort: small ("all from this sender" and bulk move). Note that the owner does not want Promotions and Social categories (see last section), so skip Apple-style category tabs.

## D. Composing

**D1. Templates and canned replies.** Spark has reply templates. FairEmail has templates, snippets and an "insert" menu ([FAQ](https://github.com/M66B/FairEmail/blob/master/FAQ.md)). kweb has none. The assistant's "Draft a reply" with nine tones covers part of it. PLAN.md M12: "templates" is "later, not planned". Effort: medium (a local list, an insert menu, optional JMAP-side storage).

**D2. Warnings before send.** Gmail and Outlook warn about a forgotten attachment. Both also warn about an empty subject. kweb has neither warning (grep found only unrelated hits). Effort: small. Keyword list should be multilingual, because the owner also writes Turkish.

**D3. Rich-text compose and HTML signatures.** Thunderbird Android shipped HTML signatures in September 2026 ([roadmap](https://roadmaps.thunderbird.net/en-US/android)). kweb compose is plain text with a plain-text signature, and the server identity's HTML signature is ignored (`core/src/jmap/send.rs:551-562`). Effort: large. Do this together with A1, or the HTML part will not carry the formatting. The owner has chosen plain text so far (PLAN.md:629), so this is the owner's call.

**D4. Camera and scan attach, and a large-file link.** Many clients attach a photo or scan from the camera. Gmail replaces large files with a Drive link. kweb has a file picker only, and Files has public links with expiry only on the Stalwart fork servers. kweb refuses a file the server will not accept. Effort: small for camera; medium for "insert a link" (depends on the fork's public links).

**D5. Default Cc or Bcc, and Reply-To per identity.** FairEmail has default Cc, Bcc and Reply-To per identity ([FAQ](https://github.com/M66B/FairEmail/blob/master/FAQ.md)). kweb identities carry a Reply-To on the server (`send_later.rs:346`) but no default Cc or Bcc. Effort: small.

**D6. Contact groups and a system contact picker.** Android 17 adds a permission-free Contact Picker ([Android 17 features](https://developer.android.com/about/versions/17/features)). Device contact sync is also not built (see E6). Effort: small for the picker, medium for groups.

## E. Notifications and Android integration

**E1. Direct Reply and inline actions from a notification.** Gmail and others reply inline with `RemoteInput`. kweb's Reply button opens the app (no `RemoteInput` in code). The notification buttons are otherwise well developed. Effort: small to medium. It must obey the sending rule "never twice, never lost", so the inline reply should be a draft that goes through the normal send queue and Undo. Not planned.

**E2. `mailto:` default handler.** kweb has no `VIEW` or `SENDTO` filter for `mailto:` in its manifest, so it cannot be the default mail app. Taps on email links elsewhere open another app. The mailto rules are in [RFC 6068](https://www.rfc-editor.org/rfc/rfc6068): fields to, cc, bcc, subject, body, show the user the full message and never send without approval. Effort: small. Not planned.

**E3. Widgets, app shortcuts and a Quick Settings tile.** Widgets are already planned as "maybe", and "later" in M12. Shortcuts (compose, search, unread) and a tile are not planned. Jetpack Glance 1.2.0 is stable, and a widget must read the local cache, not the network ([Glance releases](https://developer.android.com/jetpack/androidx/releases/glance)). FairEmail's unread widget counts only folders that notify. Effort: medium for a widget, small for static shortcuts.

**E4. Quiet hours and VIP senders.** Spark has Smart Notifications for important mail only. kweb has per-address and per-folder control but no schedule and no VIP list. Already planned as "maybe" (and "not asked for"). Effort: medium. Android already has Do Not Disturb, so keep it simple.

**E5. Tablet and landscape layout, predictive back, keyboard shortcuts.** Targeting API 36 forces edge-to-edge and predictive back, and removes the large-screen orientation opt-out ([Android 16 behavior changes](https://developer.android.com/about/versions/16/behavior-changes-16)). `NavigableListDetailPaneScaffold` gives a list-detail layout ([guide](https://developer.android.com/develop/adaptive-apps/guides/list-detail)). kweb is single column, and `android:enableOnBackInvokedCallback` is not set. The tablet layout is already planned as "maybe". Predictive back and keyboard shortcuts are not planned. Effort: medium to large for two panes, small for predictive back.

**E6. Device contact sync.** The device calendar mirror is built. Contacts are in-app only. DAVx5 and Bulwark Mobile show the pattern. Effort: large. Not planned. Medium value.

**E7. Notification extras.** Android 16 Live Updates fit only user-started work, such as send with Undo or a long download ([Live Updates guide](https://developer.android.com/develop/ui/compose/notifications/live-update)). Conversation-style notifications exist, but Google says users do not expect them for email ([Android conversations guide](https://developer.android.com/develop/ui/views/notifications/conversations)). Both are optional polish. Effort: small to medium. Low value.

**E8. Settings export and import, and a Turkish UI.** FairEmail has settings export. kweb has none found, and the UI is English only (`values` only). Turkish is "for later consideration". Effort: small to medium.

## F. Privacy and security

**F1. Read receipts.** See A7. Opt-in per message. Needs Stalwart 1.2.

**F2. App lock.** FairEmail locks with biometric or PIN after inactivity, hides notification content and hides the app in recents ([FAQ](https://github.com/M66B/FairEmail/blob/master/FAQ.md)). Tuta has PIN and biometric. Thunderbird Android has none and users ask for it ([Mozilla Connect](https://connect.mozilla.org/t5/ideas/biometrics-to-open-thunderbird-android/idi-p/74741)). kweb has none; `FLAG_SECURE` is only on screens that show secrets (`ui/account/AccountParts.kt:57`). Already planned as "for later consideration". Effort: medium (BiometricPrompt, relock timer, content hidden in notifications and recents).

**F3. Lock-screen notification levels.** K-9 offers none, app name, count, count with senders, or same as unlocked ([K-9 docs](https://docs.k9mail.app/en/beta/settings/general)). kweb has a per-account preview level (subject and text, subject only, nothing). Probably covered. Effort: small if a "count only" level is wanted.

**F4. Encrypted local cache.** Not built. PLAN.md M12 lists "encryption at rest" as "later, not planned". The notes did not check whether the cache database is encrypted. Effort: large.

**F5. Protected headers and OpenPGP details.** Encrypted mail keeps its subject in the clear (`core/src/crypto/build.rs:721`). Effort: medium. Low value, because M9 is off by default and never seen on a device.

**F6. More phishing signals.** See B6.

## Left out on purpose

These are rejected or dropped by the owner in PLAN.md and CLAUDE.md. They are not proposals.

- Sender logos, BIMI and favicons (they leak to third parties).
- Promotions and Social style categories (so no Apple or Gmail category tabs).
- Wear OS screens.
- UnifiedPush.
- Protocols other than JMAP, and servers other than Stalwart (no IMAP, POP3, SMTP or ManageSieve); a full Sieve script editor; OpenKeychain.
- Anthropic protocol for the assistant (removed 2026-11-25).
- "Sign in with ChatGPT" and on-device Gemini Nano.
- Bulwark's plugin hooks and Nextcloud or Roundcube region handling.

## What the owner has not listed anywhere

Most popular ideas were already on the owner's lists. The gaps with no mention in PLAN.md are these:

- The outgoing-mail items (A1 to A6).
- Show original and headers (B1).
- Direct Reply (E1).
- A `mailto:` handler (E2).
- App shortcuts (E3).
- Search operators (C3).
- Forgot-attachment and empty-subject warnings (D2).
- Translate a received mail (B5).
- A subscription list (C5).
- Predictive back (E5).
- Device contact sync (E6).
- Settings export (E8).

## What is uncertain

- The Gmail and Outlook quote markup comes from reverse-engineered parsers. Test with real mail in both clients before shipping A1.
- What Stalwart writes for charset, transfer encoding and line length in plain mail is unknown. Send a test mail through the local server and read the raw message.
- Whether Stalwart supports JMAP snooze, or accepts the HTML or iMIP shapes above, was not checked.
- The mainstream-app research was thin on search, compose and notification details, and used search snippets in places. No user-sentiment data was found, so the ranking rests on how often the same feature shows up across clients, not on user votes.
- Item counts for FairEmail features come from a very large FAQ that was read only in part.

## Conclusion

The research changes the order of work. The owner's lists focus on triage features such as snooze and mute. The larger gap is on the wire: other people see kweb mail as plain text with a date-less attribution. A small first step covers the date line, one clean `Re:` and the forward header. The next step is an HTML alternative with Gmail-style markup, which is the one big item.

After that, a set of small, cheap items remains that the owner never listed: show original, `.eml`, a `mailto:` handler, Direct Reply and attachment and subject warnings. They cost days each. Snooze, widgets and tablet layout cost weeks and are already on the list.
