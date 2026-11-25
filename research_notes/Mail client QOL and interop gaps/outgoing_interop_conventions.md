# Outgoing email conventions for native look and good behaviour in Gmail, Outlook, Apple Mail and Thunderbird

Note on sourcing: several vendor-specific points (Gmail and Outlook quote markup) have no official documentation. They rest on open-source parser projects and bug trackers that reverse-engineered the clients, and are flagged "observed". Researched 2026-11-25.

## 1. Quote folding: what markup each client detects, and what happens with unrecognised quotes

### Takeaway
There is no standard. Gmail uses `div.gmail_quote` with a `div.gmail_attr` line and a `blockquote.gmail_quote`. Outlook on the web uses `#appendonsend` and `#divRplyFwdMsg`. Thunderbird and Apple Mail use `blockquote type="cite"`. A plain-text-only client gets folding only partly. Gmail appears to fold by matching quoted content against earlier thread messages, and mismatched quotes stay open.

### Cited Findings
- Gmail's reply is a `div.gmail_quote` holding a `div.gmail_attr` line ("On ... wrote:") and a `blockquote.gmail_quote`. A forward is a `gmail_quote` div (`gmail_quote_container`) with a `gmail_attr` header and no `blockquote.gmail_quote`. Observed by third-party parsers, matched against 2024 and June 2026 captured samples; the sources themselves say to verify against a real capture. — [dead-letter #193](https://github.com/BigCactusLabs/dead-letter/issues/193), [dead-letter #233](https://github.com/BigCactusLabs/dead-letter/issues/233), [google_workspace_mcp PR 1238](https://github.com/taylorwilsdon/google_workspace_mcp/pull/1238)
- Older Gmail forwards are recognised by their separator text "---------- Forwarded message ----------" inside `div.gmail_quote`. — [dead-letter #193](https://github.com/BigCactusLabs/dead-letter/issues/193)
- Gmail's quote blockquote has inline styles: left margin, thin grey left border, left padding (class `gmail_quote`). — [GNOME bug 782470 search summary](https://bugzilla.gnome.org/show_bug.cgi?id=782470)
- Gmail's UI "expects" an HTML `<blockquote>` to show the collapsed "..." control. A CLI tool's plain-text `>` replies initially showed the quote expanded. After an upgrade (v0.18.1, closed 2026-03-20) both plain and `--html` replies reportedly collapsed, so Gmail's folding does not strictly require blockquote. — [googleworkspace/cli #567](https://github.com/googleworkspace/cli/issues/567)
- Gmail trims content that repeats text already in the conversation, shown as "..." / "Show trimmed content". It collapses history only when the quote matches what Gmail already has in the thread. Mismatched, rebuilt quotes stay fully visible. This is user-community explanation, not Google documentation. — [Labnol on trimmed content](https://labnol.org/code/19938-show-trimmed-content-gmail), [eM Client forum](https://forum.emclient.com/t/past-emails-are-not-presented-as-they-should-be-over-gmail/68479) (poster's explanation: Gmail cannot find the earlier content, and the thread does not confirm it; a reply suggested "Include and Indent" may not suit Gmail web)
- Outlook on the web and new Outlook: `id="appendonsend"` is the compose separator and `id="divRplyFwdMsg"` is the reply/forward header div, usually after an `<hr>`. The quoted history runs from `#appendonsend` (or the hr before `#divRplyFwdMsg`) to the end. Header block is bold From/Sent/To/Subject. Observed, not Microsoft-documented. — [search results from docsluice/zapdesk/sessions PRs](https://github.com/sanjay3290/docsluice/pull/243), [zapdesk #595](https://github.com/knowall-ai/zapdesk/issues/595)
- Outlook plain-text replies have no `>` prefixes. History is marked only by a From:/Sent:/To:/Subject: header block (or "-----Original Message-----"). — [zapdesk #595](https://github.com/knowall-ai/zapdesk/issues/595), [emailforagents.ai](https://emailforagents.ai/blog/strip-quoted-reply-text-for-agents)
- Microsoft documents `appendOnSend` for add-ins, which inserts content at the end of the body. — [Microsoft Learn: append on send](https://learn.microsoft.com/office/dev/add-ins/outlook/append-on-send)
- Thunderbird reply: `div.moz-cite-prefix` holds the "On [date], [author] wrote:" line, then `blockquote type="cite"` (with a `cite="mid:..."` attribute). Plain-text quoted into HTML gets `pre.moz-quote-pre` inside (Thunderbird 57+). `type="cite"` is non-standard, kept deliberately. — [markdown-here-revival #44](https://gitlab.com/jfx2006/markdown-here-revival/-/issues/44), [Mozilla bug 183219](https://bugzilla.mozilla.org/show_bug.cgi?id=183219), [bug 1392064](https://bugzilla.mozilla.org/show_bug.cgi?id=1392064), [bug 278663](https://bugzilla.mozilla.org/show_bug.cgi?id=278663)
- Apple Mail uses `blockquote type="cite"`. Mozilla's discussion calls `type="cite"` the closest thing to a standard quote marker. — [Mozilla bug 442755 / search summary](https://bugzilla.mozilla.org/show_bug.cgi?id=442755), [Mozilla bug 183219](https://bugzilla.mozilla.org/show_bug.cgi?id=183219). Apple's own markup is not documented here; only the type="cite" identity is confirmed by a parser write-up: [emailforagents.ai](https://emailforagents.ai/blog/strip-quoted-reply-text-for-agents)
- When Thunderbird replies are read in Outlook or Gmail, quote bars can go missing, and Gmail shows the quote indented in another colour. — [Mozilla support 1372836 (via search summary)](https://support.mozilla.org/yo/questions/1372836)
- Attribute-based detection (class, id, `type="cite"`) breaks after an HTML sanitizer strips attributes, so it is receiver-dependent. — [emailforagents.ai](https://emailforagents.ai/blog/strip-quoted-reply-text-for-agents)
- An IETF draft (draft-ietf-sml-structured-quoted-content-00, July 2026) proposes structured quoted content via `data-source-id` and `data-id` on div/span, and tells MUAs not to reverse-engineer unstructured quotes. Defines no class names; MUA guidance section is still TODO. Draft only. — [IETF draft](https://datatracker.ietf.org/doc/html/draft-ietf-sml-structured-quoted-content-00)

### Inferences
- A client that sends HTML replies should wrap quotes the Gmail way (`div.gmail_quote` > `div.gmail_attr` + `blockquote.gmail_quote`), since most receivers also recognise `blockquote` alone; adding `type="cite"` covers Apple and Thunderbird. This is a combination of the sources above, not a tested recipe.
- A plain-text-only sender can still get Gmail folding if the quote text matches the earlier message (reproduced faithfully, `> ` prefixed). The quote must be the real previous body.
- For Outlook, plain text has no folding hook except the header block, so a reply with a "From/Sent/To/Subject" block makes Outlook-side tools detect history.

### Gaps
- No official Google or Microsoft statement on how Gmail decides to hide quotes in plain text. The "compares with the thread" rule is community-reported only.
- What Gmail or Outlook do for a wholly unrecognised quote (not Gmail-like, not blockquote, no `>`): only the eM Client report that it is shown unfolded; nothing on Outlook.
- Apple Mail's exact reply markup (beyond `type="cite"`) was not sourced.

## 2. Forwarding

### Takeaway
Gmail forwards use a "---------- Forwarded message ---------" separator and a From/Date/Subject/To block. Outlook uses a bold From/Sent/To/Subject block after an hr. Inline vs attached (message/rfc822) was not covered by sources found.

### Cited Findings
- Gmail forward header: separator, then From, Date, Subject and To lines (addresses as mailto links) inside `gmail_attr`; body sits directly in the `gmail_quote_container` without a blockquote. Observed. — [google_workspace_mcp PR 1238](https://github.com/taylorwilsdon/google_workspace_mcp/pull/1238), [dead-letter #193](https://github.com/BigCactusLabs/dead-letter/issues/193)
- Outlook web forward: `#divRplyFwdMsg` with bold header block (From/Sent/To/Subject), as for replies. — [zapdesk #595](https://github.com/knowall-ai/zapdesk/issues/595)
- Parsers that treat all below-marker content as quoted history drop forwarded content; so the forward block must be recognisable as a forward, not a reply, to avoid being stripped. — [dead-letter PR 194](https://github.com/BigCactusLabs/dead-letter/pull/194), [zapdesk #595](https://github.com/knowall-ai/zapdesk/issues/595)
- FairEmail's FAQ index shows there are specific entries for forwarding/attachments, but the relevant text was not retrievable. — [FairEmail FAQ](https://github.com/M66B/FairEmail/blob/master/FAQ.md)

### Inferences
- A plain-text forward should carry the Gmail-style separator and a header block with From, Date, Subject, To (and Cc when present) so Gmail and Outlook users see something familiar.

### Gaps
- Inline versus message/rfc822 attachment guidance, and how Gmail/Outlook preserve the original HTML and attachments, was not found in a citable source.

## 3. Plain-text part: format=flowed, line length, multipart/alternative

### Takeaway
RFC 3676 defines format=flowed. Major receivers and senders (Outlook, Apple Mail, Gmail) do not support sending it, and Fastmail dropped it. Hard-wrapping at about 72 to 78 characters is the safe default, with multipart/alternative for HTML when formatting matters.

### Cited Findings
- format=flowed: a soft break is a trailing space before CRLF; DelSp default No; lines SHOULD be 78 characters or fewer (counting trailing and stuffed spaces, not CRLF); wrap at 72 or fewer is suggested, 66 is cited as most readable. — [RFC 3676 §4.1, 4.2](https://www.rfc-editor.org/rfc/rfc3676)
- Space-stuffing: lines starting with space, `>` or `From ` MUST get one extra leading space; receivers remove one. — [RFC 3676 §4.4](https://www.rfc-editor.org/rfc/rfc3676)
- Quote depth is the number of leading `>`; all lines of a paragraph must share a depth. — [RFC 3676 §4.5](https://www.rfc-editor.org/rfc/rfc3676)
- Signature separator is `-- ` (dash dash space) sent unchanged; generators MUST NOT create soft breaks that yield it, and a flowed line before it ends the paragraph. Receivers test for it before and after unquoting/unstuffing. — [RFC 3676 §4.3](https://www.rfc-editor.org/rfc/rfc3676)
- Clients unaware of flowed show trailing spaces; aware but not DelSp show an extra space; tables and code should be fixed lines. — [RFC 3676 §5](https://www.rfc-editor.org/rfc/rfc3676)
- Fastmail's CTO: "none of the major webmail providers and neither of the big email clients, Outlook or macOS Mail" support sending it; text-only users often disable it; Fastmail chose not to implement it (post undated in the fetch). — [Fastmail blog](https://fastmail.com/blog/format-flowed)
- K-9 Mail does not send format=flowed or hard-wrap plain text; it always uses quoted-printable (maintainer-side reply). Users report that Thunderbird replies to flowed messages come back hard-wrapped, and that Outlook and Lotus Notes often send plain text without format=flowed so quotes arrive unwrapped. — [K-9 forum thread](https://forum.k9mail.app/t/k-9-mail-is-not-sending-format-flowed-for-plain-text-nor-hard-wrapping/1086), [cpbotha.net](https://cpbotha.net/2016/09/27/thunderbird-support-of-rfc-3676-formatflowed-is-half-broken/), [Mozilla bug 1702197](https://bugzilla.mozilla.org/show_bug.cgi?id=1702197)
- Quote handling in Thunderbird: quoted text in plain-text mail that gets wrapped can display as non-quote. — [Mozilla bug 1702197](https://bugzilla.mozilla.org/show_bug.cgi?id=1702197)
- Google's bulk sender rules require RFC 5322 compliance and a valid Message-ID in every message, single instances of From/To/Subject/Date. — [Google bulk sender guidelines](https://support.google.com/a/answer/81126)
- RFC 6047 (iMIP) and CalConnect both recommend multipart/alternative with a human-readable text part. (For calendar only.) — [RFC 6047 §2.4](https://www.rfc-editor.org/rfc/rfc6047)

### Inferences
- For plain-text-only output, either hard-wrap to ~72 with `>` quoting (works everywhere) or send format=flowed with DelSp=yes and strict space-stuffing; the evidence says receivers that ignore flowed will then show ragged lines only if lines are over-long.
- Gmail, Outlook and Apple Mail users are accustomed to HTML; whether plain text renders worse (fonts, wrapping) in Outlook and Apple Mail was not confirmed by a source here.

### Gaps
- No citable source on Outlook's or Apple Mail's plain-text font/wrapping rendering, nor on whether Gmail honours format=flowed on receive.
- 998-character hard limit (RFC 5322) not fetched.

## 4. Threading and subject prefixes

### Takeaway
Gmail threads on matching Subject plus RFC-compliant References/In-Reply-To. Outlook/Exchange writes Thread-Index (22-byte header plus 5-byte blocks) and Thread-Topic. Localised prefixes (AW, SV, VS, WG) pile up as "Re: AW: SV:" unless clients normalise them.

### Cited Findings
- Gmail API: a message joins a thread only if the threadId is given, References and In-Reply-To are RFC-compliant, and the Subject matches. — [Gmail API: Manage threads](https://developers.google.com/gmail/api/guides/threads)
- Gmail conversation view (2019 note): notifications need the same subject to thread; different subjects or unique References values prevent threading. — [Workspace Updates 2019](https://workspaceupdates.googleblog.com/2019/03/threading-changes-in-gmail-conversation-view.html)
- Thread-Index: MIME writers copy PidTagConversationIndex (binary) base64-encoded into the Thread-Index header. — [MS-OXCMAIL Conversation Index](https://learn.microsoft.com/en-us/openspecs/exchange_server_protocols/ms-oxcmail/1b52b014-af7d-41f8-9dc5-4b0b20294457)
- Layout: 22-byte header = reserved 0x01, 5-byte FILETIME (high 40 bits, big-endian), 16-byte GUID generated per new conversation; each reply appends a 5-byte block (1-bit delta code, 31-bit time delta, 8-bit random). — [MS-OXOMSG PidTagConversationIndex](https://learn.microsoft.com/en-us/openspecs/exchange_server_protocols/ms-oxomsg/9e994fbb-b839-495f-84e3-2c8c02c7dd9b)
- A non-Outlook client that copies the parent's Thread-Index and appends a block, or leaves it out and relies on References, are both possible; the docs fetched do not say how Exchange Online groups messages that lack Thread-Index. (The Microsoft threading page URL I tried returned 404.)
- Outlook adds localised prefixes (AW and WG in German; SV and VS in Danish/Norwegian; VS and VL in Finnish) and treats them as the same conversation; the English-headers option in International Options forces RE:/FW:. Mixed-language threads show "Re: AW: SV: Re: SV: AW: stuff". — [Office Watch](https://office-watch.com/?p=2081), [msoutlook.info](https://www.msoutlook.info/?p=656)
- Thunderbird's pref `mailnews.localizedRe` takes a comma-separated list such as AW,SV; aerc's regex list covers AW, RE, SV, VS, ODP, R; Mozilla's bug thread has a longer list (Greek, Italian, Portuguese, Spanish). — [Mozilla bug 415655](https://bugzilla.mozilla.org/show_bug.cgi?id=415655), [aerc commit](https://git.platypush.tech/blacklight/aerc/commit/2f575c00ec5fa2fc8f6d4d65c26557ad7ccab9a3)
- Google's bulk guidance: use Re:/Fwd: only for actual replies and forwards. — [Google bulk sender guidelines](https://support.google.com/a/answer/81126)

### Inferences
- Send In-Reply-To (parent Message-ID) and References (parent's References plus parent's ID), keep the subject identical apart from a single prefix, and strip known localised prefixes before adding one `Re:`.
- Turkish prefixes (İlt, TR) from the brief were not found in sources; they should be added to the strip list by reasoning only.

### Gaps
- Exchange Online's actual grouping rules (Conversation-ID, ConversationTopic) beyond Thread-Index structure; the Microsoft page I tried was not found.
- Verified prefix lists for Turkish, Dutch (Antw), Swedish and others; no authoritative table.

## 5. Signature delimiter

### Takeaway
`-- ` followed by a newline is the delimiter, and it must keep its trailing space.

### Cited Findings
- `-- ` is sent as is, neither fixed nor flowed; receivers check for it before and after removing quote marks and stuffing. — [RFC 3676 §4.3](https://www.rfc-editor.org/rfc/rfc3676)
- Quoted-printable should not be used solely to protect trailing spaces unless the part is signed or encrypted; stripped trailing whitespace degrades lines harmlessly but can break the delimiter. — [RFC 3676 §4.2, 7.1](https://www.rfc-editor.org/rfc/rfc3676)
- Gmail "trimmed content" can hide the sender's signature along with quotes. — [Labnol](https://labnol.org/code/19938-show-trimmed-content-gmail)

### Inferences
- Replies should cut the signature after `-- ` from the quoted text; Gmail and Outlook HTML signatures have no delimiter, so stripping them is unreliable.

### Gaps
- How Gmail, Outlook and Apple Mail strip signatures in replies: not sourced.

## 6. Calendar (iMIP and iTIP)

### Takeaway
Send multipart/alternative with a text part and `text/calendar; method=REQUEST; charset=UTF-8` whose METHOD matches, with UID, incrementing SEQUENCE, and mailto ORGANIZER/ATTENDEE. Gmail-specific RSVP rules are only anecdotal.

### Cited Findings
- `text/calendar` must carry a `method` parameter matching the METHOD property (case-insensitive); a part without it is not iMIP. Charset UTF-8 for non-ASCII. Multiple objects with different METHOD go in multipart/mixed. — [RFC 6047 §2.4](https://www.rfc-editor.org/rfc/rfc6047)
- Provide a human-readable alternative (SHOULD); do not use multipart/alternative for slightly different calendar objects. Compliant receivers MUST process text/calendar nested in multipart/*. — [RFC 6047 §2.4](https://www.rfc-editor.org/rfc/rfc6047)
- Calendar addresses are mailto: URIs; do not infer the originator from Sender or Reply-To; use ORGANIZER/ATTENDEE. — [RFC 6047 §2.3](https://www.rfc-editor.org/rfc/rfc6047)
- Content-Disposition optional; the part is handled by content type, not extension. Use quoted-printable or base64 for non-ASCII. — [RFC 6047 §2.5, 2.6](https://www.rfc-editor.org/rfc/rfc6047)
- In a reply, the attendee is the originator and the organizer the recipient; only the respondent may set their status. Authentication in the RFC is S/MIME; unsigned replies are usually trusted in practice (the RFC says they may not be). — [RFC 6047 §2, 2.2](https://www.rfc-editor.org/rfc/rfc6047)
- Best practice: UID unique per event, SEQUENCE incremented on every update (or clients may discard), organizer address for both SMTP envelope sender and ORGANIZER, REPLY to the organizer only; missing `method` parameter makes many clients ignore the part. — [CalConnect devguide](https://devguide.calconnect.org/imip/best-practices/)
- Field reports for Gmail (forums, anecdotal): RSVP buttons missing when attendee RSVP was FALSE (Yahoo/Outlook still showed them); only "Add to calendar" when the attendee line was off; ORGANIZER/ATTENDEE must be valid mailto and one must match the Gmail account; .ics base64 with `text/calendar; method=REQUEST` and a filename was the top answer; Gmail generates an `invite.ics` attachment. — [ServiceNow community thread](https://www.servicenow.com/community/developer-forum/meeting-invite-ics-with-rsvp-to-google-gmail-calendar/m-p/1380327), [Stack Overflow mirror](https://kiwix.lema.org/stackoverflow.com_en_all_2023-11/a/66151376)
- CalConnect notes some vendors put accept/decline links in HTML for non-aware clients. — [CalConnect devguide](https://devguide.calconnect.org/imip/best-practices/)

### Inferences
- A REPLY should include the same UID, the replier's ATTENDEE with PARTSTAT=ACCEPTED/DECLINED/TENTATIVE, the SEQUENCE from the request, DTSTAMP, and ORGANIZER; this follows RFC 5546 conventions, which I did not fetch.
- Setting RSVP=TRUE on the attendee in outgoing REQUESTs is suggested by the Gmail field reports.

### Gaps
- Google's official developer page for invitation handling returned 404 and was not found; Outlook's requirements are not sourced. RFC 5546 text not read.

## 7. Other: read receipts, List-Unsubscribe, Autocrypt, Message-ID, Content-Language, Reply-To, BIMI, mailto

### Takeaway
Receipts should be opt-in per message with consent. One-click unsubscribe requires an HTTPS POST with no cookies after user consent and a DKIM-signed header. Autocrypt: one header, addr equal to From, under 10 KiB.

### Cited Findings
- MDN (RFC 8098): Disposition-Notification-To once only; a receiver may ignore it; "default value should be not to send MDNs"; must not send automatically when the address differs from Return-Path; never an MDN for an MDN; MDN is multipart/report with a null envelope sender; include Message-ID in messages requesting MDN. — [RFC 8098](https://www.rfc-editor.org/rfc/rfc8098)
- List-Unsubscribe-Post (RFC 8058): sender sends one List-Unsubscribe with an HTTPS URI and List-Unsubscribe-Post `List-Unsubscribe=One-Click`, both covered by a valid DKIM signature; no redirect on the POST endpoint. A MUA performs an HTTPS POST (multipart/form-data preferred), with user consent, no cookies or credentials, and should not offer one-click without a valid DKIM signature. — [RFC 8058 §3, 4](https://www.rfc-editor.org/rfc/rfc8058)
- Google requires one-click unsubscribe for bulk senders (5,000 per day), visible link in body, valid Message-ID, RFC 5322 compliance. — [Google bulk sender guidelines](https://support.google.com/a/answer/81126)
- Autocrypt: `Autocrypt: addr=...; [prefer-encrypt=mutual;] keydata=BASE64`, keydata last; `addr` must match From or the header is invalid; one header per message, at most 10 KiB; same header for all recipients; gossip in the encrypted payload only. — [Autocrypt Level 1](https://docs.autocrypt.org/level1.html)
- FairEmail sends Autocrypt only with signed/encrypted messages because "too many email servers have problems with the often long Autocrypt header". — [FairEmail FAQ 12](https://github.com/M66B/FairEmail/blob/master/FAQ.md)
- mailto (RFC 6068): fields to, cc, bcc, subject, body, and In-Reply-To (example); UTF-8 percent-encoding; `%20` not `+`; `%0D%0A` for body line breaks; originator, routing and MIME headers must be ignored; show the full decoded message and get approval before sending; do not repeat field names; Bcc allowed with harvesting caveat. — [RFC 6068](https://www.rfc-editor.org/rfc/rfc6068)
- Reply-To and defaults: FairEmail allows default CC/BCC/Reply-To per identity; "+extra" address editing. — [FairEmail FAQ 9](https://github.com/M66B/FairEmail/blob/master/FAQ.md)
- BIMI: sender-side needs enforced DMARC (quarantine or reject, pct 100), SVG Tiny PS logo, optional VMC/CMC, DNS TXT at `default._bimi.<domain>`; display is each mailbox provider's choice. Which clients do what (Gmail, Apple, Yahoo, Fastmail) was not retrievable. — [BIMI Group implementation guide](https://bimigroup.org/implementation-guide/)
- Google bulk guidance for Message-ID/headers: valid Message-ID, single From/To/Subject/Date, one address in From, no more than needed header size. — [Google bulk sender guidelines](https://support.google.com/a/answer/81126)

### Inferences
- A client that receives MDN requests should ask the user, and for a mismatched Return-Path never auto-send. A client that performs one-click should check DKIM-covered headers via the server's Authentication-Results rather than doing its own DKIM.
- Message-ID best practice (unique, `<random@own-domain>`) is implied by Google's requirement but no source gave a format.

### Gaps
- Content-Language usage, Reply-To handling in major clients, and Message-ID format recommendations were not found in a citable source.
- Whether Gmail/Outlook/Apple Mail send receipts or auto-handle Disposition-Notification-To: not sourced.

## 8. Interoperability pitfalls of minimal and plain-text clients

### Takeaway
Reported pain points are hard-wrapped or unwrapped quotes, quote bars missing, quotes not folded in Gmail, stacked prefixes, and headers that some servers reject.

### Cited Findings
- Gmail did not fold history in messages from eM Client and others whose quote it could not match. — [eM Client forum](https://forum.emclient.com/t/past-emails-are-not-presented-as-they-should-be-over-gmail/68479)
- Replies to flowed mail come back hard-wrapped from Thunderbird because quoted text is sent as 7bit rather than flowed; Apple Mail and others flow quoted text with quoted-printable. — [Mozilla bug 1702197 and related, per search summary](https://bugzilla.mozilla.org/show_bug.cgi?id=1702197), [cpbotha.net](https://cpbotha.net/2016/09/27/thunderbird-support-of-rfc-3676-formatflowed-is-half-broken/)
- K-9 Mail: no flowed, no hard wrap, QP always. — [K-9 forum](https://forum.k9mail.app/t/k-9-mail-is-not-sending-format-flowed-for-plain-text-nor-hard-wrapping/1086)
- Stacked localised prefixes in mixed-language threads. — [Office Watch](https://office-watch.com/?p=2081)
- Long Autocrypt headers trip some servers. — [FairEmail FAQ 12](https://github.com/M66B/FairEmail/blob/master/FAQ.md)

### Inferences
- The least risky plain-text recipe: hard wrap at about 72, `>` quotes with a Gmail-like attribution line, a single `Re:`, correct References, optional HTML part for rich quoting.

### Gaps
- Mutt and Fastmail plain-text user reports beyond Fastmail's blog were not gathered. The FairEmail FAQ was too large to read past its first 100,000 characters (372,221 total), so its detailed interop entries are unread.
