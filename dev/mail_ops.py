#!/usr/bin/env python3
"""Acts on the newest Inbox mail of the local test server, as "another client" would.

    python3 dev/mail_ops.py read      # mark the newest unread Inbox mail read
    python3 dev/mail_ops.py unread    # mark the newest read Inbox mail unread
    python3 dev/mail_ops.py archive   # move the newest Inbox mail to Archive
    python3 dev/mail_ops.py delete    # move the newest Inbox mail to Trash
    python3 dev/mail_ops.py list      # print the newest 10 Inbox mails

For the "read elsewhere" rows of the M2 test table. Credentials: see dev/README.md.
"""

import json
import os
import sys
import urllib.request

BASE = "http://127.0.0.1:18080"
USING = ["urn:ietf:params:jmap:core", "urn:ietf:params:jmap:mail"]


def opener():
    path = os.environ.get("KWEB_TEST_CREDS") or os.path.expanduser("~/tools/stalwart-test/etc/tester.txt")
    user, _, password = open(path, encoding="utf-8").read().strip().partition(":")
    passwords = urllib.request.HTTPPasswordMgrWithPriorAuth()
    passwords.add_password(None, BASE, user, password, is_authenticated=True)
    return urllib.request.build_opener(urllib.request.HTTPBasicAuthHandler(passwords))


def main():
    op = sys.argv[1] if len(sys.argv) > 1 else "list"
    http = opener()

    def call(*calls):
        body = json.dumps({"using": USING, "methodCalls": [[n, a, str(i)] for i, (n, a) in enumerate(calls)]}).encode()
        request = urllib.request.Request(BASE + "/jmap/", data=body, headers={"Content-Type": "application/json"})
        with http.open(request, timeout=30) as response:
            return [r[1] for r in json.load(response)["methodResponses"]]

    with http.open(BASE + "/jmap/session", timeout=30) as response:
        account = json.load(response)["primaryAccounts"]["urn:ietf:params:jmap:mail"]
    boxes = call(("Mailbox/get", {"accountId": account}))[0]["list"]
    by_role = {b.get("role"): b["id"] for b in boxes if b.get("role")}

    filter_ = {"inMailbox": by_role["inbox"]}
    if op == "read":
        filter_["notKeyword"] = "$seen"
    elif op == "unread":
        filter_["hasKeyword"] = "$seen"
    ids = call(("Email/query", {"accountId": account, "filter": filter_, "sort": [{"property": "receivedAt", "isAscending": False}], "limit": 10}))[0]["ids"]
    if not ids:
        sys.exit("no matching mail in the Inbox")
    if op == "list":
        mails = call(("Email/get", {"accountId": account, "ids": ids, "properties": ["subject", "keywords", "receivedAt"]}))[0]["list"]
        for m in mails:
            print(("  " if "$seen" in m["keywords"] else "* ") + m["receivedAt"], m["subject"][:60])
        return
    target = ids[0]
    if op == "read":
        patch = {"keywords/$seen": True}
    elif op == "unread":
        patch = {"keywords/$seen": None}
    elif op in ("archive", "delete"):
        if "archive" not in by_role and op == "archive":
            sys.exit("no Archive folder; run dev/seed_mail.py once")
        dest = by_role["archive" if op == "archive" else "trash"]
        patch = {"mailboxIds": {dest: True}}
    else:
        sys.exit(__doc__)
    answer = call(("Email/set", {"accountId": account, "update": {target: patch}}))[0]
    if target not in (answer.get("updated") or {}):
        sys.exit(f"failed: {answer}")
    print(f"{op}: done for {target}")


if __name__ == "__main__":
    main()
