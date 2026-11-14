#!/usr/bin/env python3
"""Puts sample mail into the Inbox of the local Stalwart test server.

    python dev/seed_mail.py            # the full sample set
    python dev/seed_mail.py one        # one new plain mail (to watch a notification arrive)
    python dev/seed_mail.py bulk 200   # 200 short mails, for lists and search

See dev/README.md for the server itself.
"""

import json
import os
import sys
import time
import urllib.request
from email.message import EmailMessage
from email.utils import formatdate, make_msgid

BASE = "http://127.0.0.1:18080"
TO = "tester@kweb.test"

# An 8x8 crimson PNG, for the inline image and the attachment.
PNG = bytes.fromhex(
    "89504e470d0a1a0a0000000d49484452000000080000000808020000004b6d29dc"
    "000000154944415478da633ca1ecc5800d3031e00083530200f6840145c67bd891"
    "0000000049454e44ae426082"
)


def base(sender, subject, to=TO):
    m = EmailMessage()
    m["From"] = sender
    m["To"] = to
    m["Subject"] = subject
    m["Date"] = formatdate(localtime=True)
    m["Message-ID"] = make_msgid(domain="example.org")
    return m


def plain(sender, subject, text):
    m = base(sender, subject)
    m.set_content(text)
    return m


def html(sender, subject, text, markup):
    m = base(sender, subject)
    m.set_content(text)
    m.add_alternative(markup, subtype="html")
    return m


def sample_set():
    out = []
    out.append(plain("Ada Lovelace <ada@example.org>", "Lunch on Friday?", "Are you free at 12:30?\n\nAda\n"))
    out.append(plain("Zeynep Çağlar <zeynep@example.org>", "Toplantı notları: şüphe, ığdır, öğle", "Merhaba,\n\nNotlar ektedir.\nİyi çalışmalar.\n"))

    newsletter = html(
        "Weekly Digest <news@example.com>",
        "This week: remote images and tracking pixels",
        "Plain text part of the newsletter.\n",
        """<html><body style="font-family:sans-serif">
<h1 style="color:#c8234a">Weekly Digest</h1>
<p>This mail has a <b>remote image</b>; it must not load until the reader allows it.</p>
<img src="https://picsum.photos/600/200" width="600" height="200" alt="remote banner">
<p><a href="https://example.com/article?id=1">Read the article</a></p>
<table border="1" cellpadding="6"><tr><th>Item</th><th>Price</th></tr><tr><td>Tea</td><td>4.50</td></tr></table>
<img src="https://example.com/pixel.gif?u=tester" width="1" height="1" alt="">
<script>document.body.innerHTML = "SCRIPT RAN: the sandbox is broken";</script>
</body></html>""",
    )
    out.append(newsletter)

    only_html = base("Shop <orders@example.net>", "Your order 10482 has shipped (HTML only)")
    only_html.set_content("<html><body><p>Order <b>10482</b> is on its way.</p><p style='color:gray'>No plain part in this mail.</p></body></html>", subtype="html")
    out.append(only_html)

    inline = base("Design <design@example.org>", "Logo draft with an inline image")
    inline.set_content("The logo is inline in the HTML part.\n")
    cid = make_msgid(domain="example.org")
    inline.add_alternative(f'<html><body><p>Logo draft:</p><img src="cid:{cid[1:-1]}" width="40" height="40"></body></html>', subtype="html")
    inline.get_payload()[1].add_related(PNG, "image", "png", cid=cid)
    out.append(inline)

    attached = plain("Accounts <accounts@example.org>", "Invoice 2026-114 attached", "Please find the invoice attached.\n")
    attached.add_attachment(b"%PDF-1.4\n% sample, not a real PDF\n", maintype="application", subtype="pdf", filename="invoice-2026-114.pdf")
    attached.add_attachment(PNG, maintype="image", subtype="png", filename="receipt.png")
    attached.add_attachment("id,amount\n1,40.00\n2,12.50\n".encode(), maintype="text", subtype="csv", filename="lines.csv")
    out.append(attached)

    # A thread of four between two people.
    first = plain("Grace Hopper <grace@example.org>", "Release plan", "Shall we ship on Monday?\n")
    out.append(first)
    refs = [first["Message-ID"]]
    for who, text in [
        ("Linus <linus@example.org>", "Monday is fine for me.\n\n> Shall we ship on Monday?\n"),
        ("Grace Hopper <grace@example.org>", "Good. I will write the notes.\n\n> Monday is fine for me.\n"),
        ("Linus <linus@example.org>", "Thanks!\n"),
    ]:
        reply = plain(who, "Re: Release plan", text)
        reply["In-Reply-To"] = refs[-1]
        reply["References"] = " ".join(refs)
        refs.append(reply["Message-ID"])
        out.append(reply)

    out.append(plain("No Subject <nosubject@example.org>", "", "This mail has an empty subject.\n"))
    out.append(plain("Long <long@example.org>", "A very long subject " + "that goes on and on " * 12, "Long body.\n" + ("A fairly long line of text that should wrap on a phone screen. " * 8 + "\n\n") * 40))
    return out


def bulk(count):
    words = ["budget", "holiday", "server", "invoice", "meeting", "garden", "report", "ticket"]
    for i in range(count):
        word = words[i % len(words)]
        yield plain(f"Sender {i % 17} <sender{i % 17}@example.org>", f"Bulk {i:04d}: about the {word}", f"Message number {i} mentions the {word}.\n")


def credentials():
    """`user:password` of the test user, written by the set-up (see dev/README.md)."""
    path = os.environ.get("KWEB_TEST_CREDS") or os.path.expanduser("~/tools/stalwart-test/etc/tester.txt")
    user, _, password = open(path, encoding="utf-8").read().strip().partition(":")
    return user, password


def call(opener, url, body=None, content_type="application/json"):
    data = json.dumps(body).encode() if isinstance(body, (dict, list)) else body
    request = urllib.request.Request(url, data=data, headers={"Content-Type": content_type} if data else {})
    with opener.open(request, timeout=30) as response:
        return json.load(response)


def send(messages):
    """Imports the messages into the Inbox through JMAP, as the server does for arriving mail.

    SMTP is not used: the server only takes unauthenticated mail on port 25 and would run
    its spam checks on it.
    """
    user, password = credentials()
    passwords = urllib.request.HTTPPasswordMgrWithPriorAuth()
    passwords.add_password(None, BASE, user, password, is_authenticated=True)
    opener = urllib.request.build_opener(urllib.request.HTTPBasicAuthHandler(passwords))

    session = call(opener, BASE + "/jmap/session")
    account = session["primaryAccounts"]["urn:ietf:params:jmap:mail"]
    using = ["urn:ietf:params:jmap:core", "urn:ietf:params:jmap:mail"]
    boxes = call(opener, BASE + "/jmap/", {"using": using, "methodCalls": [["Mailbox/get", {"accountId": account}, "0"]]})
    boxes = boxes["methodResponses"][0][1]["list"]
    inbox = next(b["id"] for b in boxes if b.get("role") == "inbox")
    if not any(b.get("role") == "archive" for b in boxes):
        create = {"accountId": account, "create": {"a": {"name": "Archive", "role": "archive"}}}
        call(opener, BASE + "/jmap/", {"using": using, "methodCalls": [["Mailbox/set", create, "0"]]})

    sent = 0
    for m in messages:
        upload = call(opener, f"{BASE}/jmap/upload/{account}/", m.as_bytes(), "message/rfc822")
        email = {"blobId": upload["blobId"], "mailboxIds": {inbox: True}, "keywords": {}}
        result = call(opener, BASE + "/jmap/", {"using": using, "methodCalls": [["Email/import", {"accountId": account, "emails": {"m": email}}, "0"]]})
        answer = result["methodResponses"][0][1]
        if "m" not in (answer.get("created") or {}):
            sys.exit(f"import failed: {answer}")
        sent += 1
    print(f"delivered {sent} mail(s) to {user}")


if __name__ == "__main__":
    mode = sys.argv[1] if len(sys.argv) > 1 else "all"
    if mode == "one":
        send([plain("Ada Lovelace <ada@example.org>", f"Test at {time.strftime('%H:%M:%S')}", "A single new mail.\n")])
    elif mode == "bulk":
        send(bulk(int(sys.argv[2]) if len(sys.argv) > 2 else 100))
    else:
        send(sample_set())
