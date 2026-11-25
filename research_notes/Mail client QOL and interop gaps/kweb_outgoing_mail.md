# kweb mail: what outgoing mail looks like on the wire (source reading, read-only)

Paths are relative to the repository root. Sources are file:line references (the sources are repository files, not URLs).
Key caveat: for ordinary (unprotected) mail the app does NOT build RFC 5322/MIME itself. It sends a JMAP `Email/set` object and Stalwart serialises it. Only signed/encrypted mail is built by the app (core/src/crypto/build.rs).

## 1. MIME structure, charset, encoding, wrapping

### Takeaway
Ordinary mail is text/plain only (no HTML part, no multipart/alternative), described to Stalwart as JMAP JSON; charset, transfer encoding and any wrapping are chosen by the server, not the app. The app never wraps or flows the text. Only signed/encrypted mail is hand-built MIME (text/plain; charset=utf-8; quoted-printable, 76-col, no format=flowed).

### Cited Findings
- Draft/send object: `bodyValues: {text: {value: draft.body}}`, `bodyStructure = {partId:"text", type:"text/plain"}`; with attachments, `multipart/mixed` of the text part plus blob parts. No `text/html`, no `multipart/alternative` — core/src/jmap/send.rs:596-628 (test send.rs:699-749).
- No charset, Content-Transfer-Encoding or format=flowed is specified in that JSON; "The server writes the headers" — core/src/jmap/send.rs:588-589. `format=flowed` and `Content-Language` are not found anywhere in core/src (grep).
- Body text is passed as typed; the app inserts no hard wrapping. Quoted lines are `"> " + line`, never re-wrapped — core/src/compose/rules.rs:221-230.
- The message is created in Drafts via `Email/set create`, then sent via `EmailSubmission/set` (identityId + emailId), with `onSuccessUpdateEmail` moving it Drafts to Sent and clearing `$draft` — core/src/jmap/send.rs:193-208, 639-657. So the sent bytes are whatever Stalwart generated for that Email object.
- Protected (signed/encrypted) mail is built in-app: inner entity `Content-Type: text/plain; charset=utf-8`, `Content-Transfer-Encoding: quoted-printable`, CRLF, lines <=76 columns — core/src/crypto/build.rs:280-285, 168-190; attachments in `multipart/mixed`, base64 76-col — build.rs:288-316. It is uploaded verbatim with `Email/import` (core/src/jmap/import.rs:22-31), the same bytes on every retry — core/src/compose/flush.rs:146-156.
- PGP/MIME signed: `multipart/signed; protocol="application/pgp-signature"; micalg=pgp-sha256` (build.rs:677-690, 319-330); encrypted: `multipart/encrypted; protocol="application/pgp-encrypted"` (build.rs:479-490). S/MIME: detached `application/pkcs7-signature; name="smime.p7s"`, micalg sha-256 (build.rs:640-648) and `application/pkcs7-mime; smime-type=enveloped-data; name="smime.p7m"` (build.rs:650-656); RSA only, AES-256-CBC (build.rs:589-630).
- Protected mail: no Bcc header, the envelope is spelled out instead — core/src/compose/flush.rs:384-390; test core/tests/suite/crypto_send.rs:98-118. Subject of encrypted mail stays in the clear outer header (build.rs:721); no protected-headers / Encrypted-Subject found.

### Inferences
- The on-wire body encoding of ordinary mail (7bit/QP/base64, line length) depends on Stalwart's JMAP serialiser and is unknown from this repo. Long unwrapped paragraphs probably go out as long lines (the app never inserts breaks), which most clients re-wrap on display.
- No HTML part means no formatting in recipients' clients; quoting is shown only by "> " prefixes.

### Gaps
- What Stalwart emits for charset/CTE/line length: not determinable from source; would need a sent message from the local test server.

## 2. Reply quoting, signature placement

### Takeaway
Reply body = `\n\n[-- \nsignature\n]\n\n<Name <addr>> wrote:\n> line...`. The signature goes ABOVE the quote (top-posting layout). Attribution has no date. Original HTML is flattened to text; nothing is wrapped.

### Cited Findings
- Attribution: `"{Name <email> or email} wrote:"` (no date/time, no "On ... "), then each line prefixed `> ` (blank lines become `> `), CRLF normalised — core/src/compose/rules.rs:209-230. Test rules.rs:693-695: `"\n\nAnn <ann@x.org> wrote:\n> Hi\n> \n> See you"`.
- The display name is not quoted or escaped: raw `Name <addr>` (CR/LF stripped only) — rules.rs:210-217.
- Source text: plain part if non-blank, else HTML rendered by `html_to_text` — core/src/compose/mod.rs:38-45. `html_to_text`: drops script/style, `<br>` and block ends become newlines, other tags removed, entities decoded, blank runs collapsed — rules.rs:305-340. Tables, link URLs and images are lost.
- Signature placement in replies: `reply_with_signature` puts the signature block (`\n\n-- \n<sig>`) before the quote, so the user writes above both — rules.rs:259-263, mod.rs:124. New mail: signature at the end after "-- " — rules.rs:247-257, mod.rs:84-87. The delimiter is "-- " with trailing space — rules.rs:10-12.
- Test of the full reply body: `"\n\n-- \nMe\nAcme\n\nAnn <ann@x.org> wrote:\n> Hello\n> there"` — core/src/compose/mod.rs:305.
- Reply recipients: Reply-To else From; reply-all adds To+Cc minus own addresses (plus-tag aware) — rules.rs:89-126.
- Signature comes from the server identity `textSignature` only (HTML signature ignored) — core/src/jmap/send.rs:551-562; send_later.rs:336-346.

### Inferences
- Gmail/Outlook/Apple Mail write "On <date>, <name> <addr> wrote:"; the date-less attribution is valid but gives no date context and may not match other clients' quote-collapsing patterns.
- The quoted original is plain text, so recipients lose the original's formatting.

### Gaps
- Whether Stalwart wraps quote lines: not found in app code.

## 3. Forward

### Takeaway
Inline forward as plain text with a Gmail-style "---------- Forwarded message ----------" header; ISO-8601 date; original attachments re-attached by blob; no message/rfc822 option; inline cid images dropped.

### Cited Findings
- Header block: `\n\n---------- Forwarded message ----------\nFrom: Name <addr>\nDate: 2026-01-02T03:04:05Z\nSubject: ...\n[To: a, b]\n\n<text>` — core/src/compose/rules.rs:232-245; tests rules.rs:711 and mod.rs:404. Date is ISO-8601 UTC (`iso_utc`, rules.rs:288-303), not RFC 5322 style. No Cc line.
- Subject: `forward_subject` (rules.rs:39-45). Source text is plain, else flattened HTML (mod.rs:38-45, 131-156). Signature appended at the end, after the forwarded text (mod.rs:150).
- Attachments: `files(body)` = non-inline attachments minus PGP/S-MIME signature parts, re-attached by blobId — core/src/compose/mod.rs:51-58, 151. Inline images (`is_inline`) are filtered out. No `message/rfc822` forward-as-attachment found.
- Draft is marked `is_forward`; the source gets `$forwarded` after sending (`$answered` for replies) — core/src/jmap/send.rs:407-411. A forward gets no In-Reply-To/References (threading looked up only for non-forwards) — core/src/compose/flush.rs:183-186.

### Inferences
- A forward does not thread with the original (Gmail also omits References on forwards).

### Gaps
- No "forward as attachment" option found (searched core/src/compose, ffi.rs).

## 4. Subject prefixes

### Takeaway
Only "Re:" and "Fwd:"/"Fw:" are recognised. No localised prefixes (AW:, SV:, Ynt:, Ilt:) and no counters (RE[2]:), so those get a second "Re:".

### Cited Findings
- `reply_subject`: leave as is if it starts with `re` + ':' (case-insensitive, after leading whitespace); else `"Re: " + subject` — core/src/compose/rules.rs:14-36. `forward_subject`: leave if `fwd:` or `fw:`; else `"Fwd: "` — rules.rs:39-45.
- Tests: `reply_subject("Fwd: x") == "Re: Fwd: x"`, `forward_subject("Re: x") == "Fwd: Re: x"`, `reply_subject("Re") == "Re: Re"`; `"Re:Lunch"` is left as is — rules.rs:494-512.
- Empty subject: a reply becomes `"Re:"` (trim_end of "Re: ") — rules.rs:25-27. A new draft with an empty subject is sent as an empty string (send.rs:599); no "(no subject)" handling or warning found in core (Android UI not exhaustively checked).

### Inferences
- A reply to "AW: Termin" becomes "Re: AW: Termin"; "Ynt: x" gives "Re: Ynt: x"; "RE[2]: x" gives "Re: RE[2]: x". Threading by References is unaffected, but subject piles up.

### Gaps
- Android-side empty-subject prompt not checked.

## 5. Threading and other headers

### Takeaway
In-Reply-To/References are set on replies (not forwards) and not trimmed. Message-ID, Date and any User-Agent are server-generated for plain mail. The app adds no User-Agent/X-Mailer, no Reply-To, no read-receipt header, no Content-Language, no Thread-Topic/Index.

### Cited Findings
- Threading: In-Reply-To = [source Message-ID]; References = source's References (or its sole In-Reply-To when it had no References) + source id, deduplicated, no length trimming — core/src/compose/mod.rs:175-189. Header ids are fetched from the server (core/src/jmap/send.rs:327+; core/src/compose/flush.rs:172-190) and passed as JSON `inReplyTo`/`references` — send.rs:610-615. If the source has no Message-ID there are no threading headers — mod.rs:178.
- Message-ID, Date: the app does not set them for plain mail (the JMAP Email object has no messageId/sentAt) — send.rs:596-628; the server assigns them. For protected mail: `Message-ID: <32hex@sender-domain>`, `Date` RFC 5322 in `+0000` — crypto/build.rs:660-671, 721-723.
- From: identity name + email (send.rs:602-604); a display name carrying its own address is cut back by `plain_name` (send.rs:538-549). To/Cc/Bcc as JMAP address objects (send.rs:580-586, 605-609). No Sender header and no per-message Reply-To (identities can carry a server-side Reply-To: send_later.rs:346, 375-377).
- Autocrypt: only on PGP-protected mail built in-app: `Autocrypt: addr=<identity email lowercased>; keydata=<base64, 72-col folded>` (no prefer-encrypt attribute) — crypto/build.rs:733-738; core/src/ffi/crypto_send.rs:233-241. Not on ordinary mail.
- User-Agent, X-Mailer, Disposition-Notification-To, MDN/read receipts, Content-Language, Thread-Topic, Thread-Index: not found (grep of core/src and android main kotlin; only unrelated hits). Incoming read receipts are not answered.
- Headers of protected mail: From, To, Cc, Subject (RFC 2047 UTF-8 B-words, folded), Date, Message-ID, In-Reply-To, References, MIME-Version — build.rs:715-740. Display names with specials are quoted; non-ASCII names use RFC 2047 — build.rs:241-262.
- Keywords: source gets `$answered` / `$forwarded` (send.rs:407-411); drafts are saved with `$draft`, `$seen` (send.rs:598).

### Inferences
- Without Thread-Topic/Index, Outlook still threads by References/In-Reply-To.
- Message-ID/Date/MIME-Version for plain mail come from Stalwart and match what other senders through that server produce.

### Gaps
- Actual Stalwart-added headers (Message-ID domain, MIME-Version, X-Mailer/User-Agent): need a sample from the local test server.

## 6. Attachments and inline images

### Takeaway
Attachments are always `Content-Disposition: attachment`, by blob; filename encoding is the server's (plain mail) or RFC 2231 `filename*=UTF-8''...` (protected mail). Compose has no inline images.

### Cited Findings
- Plain mail: each file `{blobId, type, disposition:"attachment", name?}`, nameless allowed; the text is the first part of `multipart/mixed` — core/src/jmap/send.rs:620-627. Even `is_inline`/`content_id` attachments are sent as `attachment` (test send.rs:727-747).
- Protected mail: `Content-Type: <type>; name=...` and `Content-Disposition: attachment; filename=...` via `param()`: ASCII gives `filename="a.txt"`, non-ASCII gives `filename*=UTF-8''%C3%BC.txt` (RFC 2231), base64 76-col — crypto/build.rs:264-277, 295-312; test build.rs:778-781. Size limited by MAX_RAW_BYTES (core/src/ffi/crypto_send.rs, around lines 200-215).
- Compose picks files (android .../ui/compose/PickedFile.kt) and uploads them as blobs; no inline-image feature found in android ui/compose.

### Inferences
- Forwarded `cid:` images are dropped (mod.rs:57); pasted pictures are not possible.

### Gaps
- Server's filename encoding for non-ASCII names in plain mail: not determinable.

## 7. Calendar (iTIP/iMIP)

### Takeaway
The app builds no iMIP mail. Accepting/declining is a JMAP `CalendarEvent/set` patch with `sendSchedulingMessages: true`, leaving METHOD/PARTSTAT/ORGANIZER/ATTENDEE/SEQUENCE and the email to Stalwart. The app cannot create invitations at all (new events have no attendees), so no invitation leaves it.

### Cited Findings
- Reply: `respond_to_event` patches `participants/<id>/participationStatus` to accepted|tentative|declined — core/src/jmap/calendars.rs:227-231; core/src/jmap/jscal.rs:860-869 (refuses if not invited, or if the user is the owner). `change(..., notify=true)` sets `sendSchedulingMessages: true` — calendars.rs:238-264; "the server tells the organizer" (calendars.rs:227).
- Other edits (`save_event`, delete) use notify=false and no sendSchedulingMessages — calendars.rs:185-225, so editing or deleting an event sends no updates/cancels from the app.
- A new event object holds title, start, duration, timeZone, showWithoutTime, description, locations, recurrenceRule, alerts, calendarIds only: no participants, no replyTo — jscal.rs:632-668. `EventInput` has no attendee field — core/src/model.rs:344-361. PLAN.md:675 says invitations on an added event are refused; PLAN.md:471 lists attendees as later.
- Answering an invitation from the mail that carried it is not built (PLAN.md:176); answering in a shared calendar is unavailable (PLAN.md:304); an iTIP record was not checked (PLAN.md:483).
- Incoming scheduling notices become notifications only (PLAN.md:474).

### Inferences
- Whether the REPLY shows correctly in Gmail/Outlook depends on Stalwart's iMIP output (METHOD:REPLY, PARTSTAT, ATTENDEE, SEQUENCE, text/calendar part); unverifiable from this repo. The question "does an invitation from the app show Accept/Decline" is moot: the app sends no invitations.

### Gaps
- The iMIP bytes Stalwart produces: not testable read-only.

## 8. Unsubscribe, away message, send later

### Takeaway
Unsubscribe is a one-click HTTPS POST (RFC 8058), else a short plain "unsubscribe" mail from the default identity. The away message is a JMAP VacationResponse (text only; the server sends the reply). Send later is a JMAP submission with HOLDUNTIL/HOLDFOR.

### Cited Findings
- One-click: needs `List-Unsubscribe-Post: List-Unsubscribe=One-Click` exactly and an https URL; POST body `List-Unsubscribe=One-Click` — core/src/unsubscribe.rs:20-22, 65-75, 213-217, test 383-389; headers read as `header:List-Unsubscribe:asURLs` (core/src/jmap/features.rs:18-19). Public hosts only (unsubscribe.rs:149-198).
- Mailto fallback: first address only; subject from `subject=` (default `"unsubscribe"`, max 200 chars, one line), body from `body=` (max 1000 chars); cc/bcc ignored — unsubscribe.rs:31, 85-113, tests 316-348. Sent as a normal plain draft from the default identity, no delay, through the usual path — core/src/ffi.rs:2442-2455 (same server-built MIME as section 1; no In-Reply-To).
- Away message: `VacationResponse/set` with `subject`, `textBody` (and `htmlBody=null` when the text changed), `fromDate`/`toDate` — core/src/jmap/vacation.rs:36-50, 103-116. An existing HTML away message is shown as text and replaced by text on save (PLAN.md:650).
- Send later / undo: `EmailSubmission` envelope `{mailFrom:{email, parameters:{HOLDFOR|HOLDUNTIL}}, rcptTo:[...]}`, Bcc included — core/src/jmap/send_later.rs:34-52; hold at most 7 days (send.rs:478-480).
- Drafts: the server copy stays in Drafts with `$draft`; a draft to be encrypted is never uploaded (flush.rs:110-120).

### Inferences
- All plain outgoing mail types share the same plain-text JMAP path; unsubscribe mails therefore get a server Message-ID/Date and a bare text body.

### Gaps
- Headers of Stalwart's vacation reply (Auto-Submitted etc.): server-side, not in the repo.

## 9. Compose capabilities and things that may look odd elsewhere

### Takeaway
Compose is plain text only (no bold/links/lists, no HTML), with smart greeting/sign-off completion and AI rewrite/translate; no templates. Several choices could look odd in other clients.

### Cited Findings
- "Writing is plain text only; a reply quotes the original as text" — PLAN.md:629 and :156. No rich-text controls in compose (ComposeScreen.kt only uses `fontWeight.Bold` for UI labels, lines 729, 756).
- Smart completion (name after a greeting, rest of a sign-off) — android .../ui/compose/CompletionRules.kt:7-84, ComposeScreen.kt:163-176, 297-304. Assistant menu: rewrite styles, translate, draft reply — ComposeScreen.kt:495-515, 370-375. Compose templates/quick text: not found.
- Signature: identity `textSignature` text only; changing the sender swaps the old signature block (`swap_signature`, rules.rs:265-286).
- Send reliability: never twice, never lost; the server draft copy is a backup (compose/mod.rs:7-9, flush.rs:4-21).

### Inferences
Possible oddities in other clients (inferred, not verified on the wire):
- No HTML part: plain-text rendering with ">" quotes, no collapsible quote; the date-less attribution may not trigger Gmail's quote collapsing.
- Long lines never wrapped by the app (depends on Stalwart).
- Forward header uses an ISO-8601 UTC date and lacks Cc; forwarded inline pictures are dropped.
- Subject prefix pile-up for AW:/SV:/Ynt:/Ilt:/RE[2]:.
- Display name in the attribution is unescaped (`Doe, Jane <x> wrote:`).
- No Reply-To / Disposition-Notification-To / User-Agent / Content-Language from the app.
- Empty-subject reply gives `Re:`.

### Gaps
- No capture of a real outgoing message was available; wire-level claims about plain mail's encoding rest on Stalwart's behaviour.
