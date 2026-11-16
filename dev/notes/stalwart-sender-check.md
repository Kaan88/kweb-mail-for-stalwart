# What Stalwart 1.1.0 writes for the sender check (probed 2026-11-16)

Probed on the local test server (`~/tools/stalwart-test`), by delivering mail through its SMTP listener and
reading the headers back over JMAP (`Email/get` with `headers`). Nothing of this touches the owner's server.

## How mail got in

- **Mail made over JMAP** (the seeded mail, `EmailSubmission/set` between the two local users) gets **no**
  `Authentication-Results`, no `Received` and no `Received-SPF`. The server did no checks, so the app shows no mark.
- **Mail over SMTP** gets the full set. The server had a listener named `smtp` on `127.0.0.1:18025`, but it
  answers `503 5.5.1 You must authenticate first` to anyone (the "no login needed" rule is for port 25 itself).
  To probe, a second bind was added to that listener with the admin API and the server restarted:

  ```
  x:NetworkListener/set  update  <id of "smtp">  {"bind": {"127.0.0.1:18025": true, "127.0.0.1:25": true}}
  ```

  The test server now also listens on `127.0.0.1:25` (loopback only). Mail to `tester@kweb.test` from any
  sender is then taken without a login:
  `python -c "import smtplib; ..."` with `smtplib.SMTP('127.0.0.1', 25)` and `send_message(msg, from_addr=..., to_addrs=['tester@kweb.test'])`.
  The server checks SPF, DKIM and DMARC for real; from loopback with a made-up domain they all come out as failed
  or none, and the spam filter marks the message (`X-Spam-Status: Yes`) but still delivers it to the Inbox.

## The header, as Stalwart wrote it

```
Delivered-To: tester@kweb.test
X-Spam-Status: Yes
Received: from mail.example.org (netlimiter.com [127.0.0.1])
	by localhost (Stalwart SMTP) with ESMTP id 4BB28FB60A00000;
	Mon, 16 Nov 2026 14:25:54 +0000
Authentication-Results: localhost;
	spf=none (localhost: no SPF records found for postmaster@mail.example.org) smtp.helo=mail.example.org;
	spf=fail (localhost: domain of someone@example.org does not designate 127.0.0.1 as permitted sender) smtp.mailfrom=someone@example.org;
	iprev=pass policy.iprev=127.0.0.1;
	dmarc=fail (policy not aligned) header.from=example.org policy.dmarc=reject
Received-SPF: fail (localhost: domain of someone@example.org does not designate 127.0.0.1 as permitted sender) ...
X-Spam-Result: ...
X-Spam-Score: spam, score=13.80
Return-Path: <someone@example.org>
Date: ...   (the sender's own headers start here)
```

- The **authserv-id** (first word) is the server's hostname (`localhost` here, the `defaultHostname`), the same
  word as the `by` of the topmost `Received`.
- The order is: `Delivered-To`, `X-Spam-Status`, `Received`, `Authentication-Results`, `Received-SPF`,
  `X-Spam-*`, `Return-Path`, then the sender's own headers. The server's block is **above** every header the sender
  wrote (`Date`, `Message-ID`, `From`, ...).
- JMAP `header:Authentication-Results:asText:all` is refused for `Received` and `Return-Path` as text
  (`Invalid property`); `asRaw` works, and so does the whole-block property `headers` (name and raw value in the
  order they stand). The app asks for `headers` and `from`.

## A forged header is not removed

A message that carried its own `Authentication-Results: kweb.test; dkim=pass header.d=paypal.com; spf=pass; dmarc=pass header.from=paypal.com`
kept it. It stands **below** the server's block, after `Date`, `Message-ID` and `From`. Stalwart neither
removes nor renames it (RFC 8601 section 5 says it should). So the app believes only the topmost
`Authentication-Results` whose authserv-id equals the `by` host of the topmost `Received`, and only when no header of the
sender's own (From, To, Subject, Date, Message-ID, MIME-Version, Content-Type, Reply-To) stands above it
(`core/src/sender_check.rs`). Not covered: a local user who writes both a `Received` and an `Authentication-Results`
header into mail they hand to the server over JMAP; that mail came from an account of the same server.

## What the app does with it

`Core::sender_check(message_id)` asks for `headers` and `from` live, keeps nothing, and gives Verified (DMARC pass, or
a DKIM signature or SPF pass for the From domain), Failed (DMARC fail, or a DKIM or SPF fail for it with no pass),
Unverified (nothing passes for the From domain, or only for another domain) or Not checked (no header of the
server's): the last shows no mark. Shared mail is not checked (`sender_check` is for the own account only, and a
shared message is never cached or computed here).
