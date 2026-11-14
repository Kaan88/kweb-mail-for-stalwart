SCEPTIC REVIEW OF M8 (probe + design), with my own checks on the local server

All checks ran against http://127.0.0.1:18080 only, in the admin@kweb.test mail account (scripts) with tester as sender. 10 test mails sent. Final state: admin and second have no scripts; tester has only the inactive "vacation" (not touched); my 3 test folders and all my test mail (admin, second, tester) are deleted; nothing in the repository was edited. Scripts: /tmp/claude-1000/-mnt-C-Users-kaank-Desktop-kweb-mail-for-stalwart/1ec96980-4871-4ff3-b43b-06ef4006739f/scratchpad/sk/v1.py (validate), /tmp/claude-1000/-mnt-C-Users-kaank-Desktop-kweb-mail-for-stalwart/1ec96980-4871-4ff3-b43b-06ef4006739f/scratchpad/sk/d1.py, d2.py, d3.py in the same folder (delivery).

## 1. SieveScript/validate of the design's Sieve (asked for)

All of these returned `"error":null`: the full example in 2.5 verbatim (LF and CRLF); one rule each for From contains / is / begins / ends, Recipient contains / is / matches, Subject is / matches with `\\*`, `\\?`, `\\\\`, any header (also a name made of odd token characters), size over / under (also 2^40 and u64 max), body, `if true`, delete as `fileinto "Deleted Items"; stop;`, the `discard` alternative, all actions in the fixed order with a folder `"Q\"1"`, a script of comment lines only with no `require`, a UTF-8 folder path, and every unicode-casemap form.

One refusal: Id mode as the design writes it, `fileinto :mailboxid "k3x" ;` -> `{"type":"invalidScript","description":"Expected token \"string\" but found \";\" at line 9, column 31."}`. With a name after the id it validates.

Validate proves little here: it also accepted `:comparator "i;unicode-casemap"` without any `require`, and the probe showed it accepts unknown requires, missing folders and extra redirects.

## 2. Where the design and this server disagree (most important first)

1. **Away message.** The design's first save (`Create`, `onSuccessActivateScript`) and `filters_set_active(true)` switch an active away message off, and the response does not say so. The design only covers the other direction (`paused_by`). `Create` is defined as "nothing foreign active" but does not say whether an active `vacation` counts. Also the SieveScript state string is shared with VacationResponse, so any away edit makes a pending filter save fail `ifInState`. The probe's way (A), the vacation command inside the managed script, is not in the design at all and would change M6.
2. **Spam guard is dead code as written.** Stored mail carries `X-Spam-Status: No`, but the script cannot see that header: `exists "X-Spam-Status"` and `header :contains "X-Spam-Status" "Yes"` were both false on delivery. A GTUBE body sent by JMAP submission was also stored as `No`. Whether real spam is seen by the script, and whether an implicit keep after `stop;` leaves spam in the Inbox or in Junk, is unknown and cannot be tested here (no SMTP port; authenticated submission is not scored).
3. **Id mode.** The syntax is refused (above). With a name added, a deleted id falls back to the name: the mail goes to whichever folder now has that path, and to the Inbox only if none has (I checked bogus id + missing name -> Inbox). The design's "deleted folder means implicit keep" is only half true.
4. **`comparator-i;unicode-casemap` is not in `sieveExtensions`.** The design's own capability check would therefore refuse every rule with a non-ASCII value. It is also not needed: with the default comparator, `header :contains "subject" "über"` matched "ÜBER" and `body :text :contains "çaydanlik ÖĞLE"` matched "öğle". The server does run a script that requires and uses it (flags set, filed), but an invented `"i;bogus"` comparator behaved the same, so it is probably just ignored. Drop it and the require row.
5. **Missing folder in Path mode.** The mail lands in the Inbox, nothing is created (probe; I saw the same). The design's "Stalwart may even create it" is wrong; the silent no-op is the real risk, so the folder refresh matters more than the design suggests.
6. **Redirect limit.** The design says the server stops the script. It does not: with two redirects the first went out, the second was silently dropped, and the rest of the script ran (checked in both orders, targets second and tester). The static refusal stays right.
7. **"From contains" uses `address`**, which does not look at the display name: From `"Boss Person" <tester@kweb.test>` with `address :contains "from" "boss"` was false; `header :contains "from" "boss"` was true. Decide and word the UI accordingly.
8. **`require "nonexistent"`** really disables the whole script at delivery (confirmed: no flag set, mail in Inbox). So the capability check before writing is mandatory, not a courtesy.
9. Smaller: error names here are `invalidScript` and `scriptIsActive` (design already accepts both). The created/updated `blobId` differs from the uploaded one, so the re-read must use the returned id. Deactivate and destroy need two method calls (matters for deleting an old backup).

Confirmed as the design assumes: `Inbox/SkSub` is the path of an Inbox subfolder (case-insensitive: `inbox/sksub` works too); `\\*` in `:matches` is a literal star (pattern `...50%\\*deal` false, `...50%*deal` true); `:matches` ignores case; `address :is ["to","cc"]` and `:matches` work; quoted `\"` and `\\` in a header value match; flags-only rule keeps the mail in the Inbox with `$seen $flagged`; the text comes back byte for byte after `SieveScript/set` (938 bytes, LF, comments, UTF-8).

## 3. Probe claims without a recorded request and answer

- No spam verdict was ever produced (`spamtest` only on clean mail, score 0). Everything about spam and scripts is open.
- "Undeclared capability" for `vacation` is marked "presumably".
- Rename of an **active** script, and updating only `blobId` on an active script without `isActive`, are not recorded; `ReplaceEdited` and `Update` depend on them.
- A folder name containing "/", two folders with the same path differing in case, and shared folders as targets: not tried.
- The combined script (A): only one reply seen. Dates, the reply's From, how often it replies, and what VacationResponse/get shows then are not recorded.
- "Three outgoing messages per run" untested; `SieveScript/copy` and Email/import untested; the inbound limit of 25 mails per hour per address is from a log line only (it did not hit admin in my 10 mails).
- The probe's own clean-up note leaves one possible stray copy in files2@kweb.test.

## 4. Questions the design cannot proceed without

1. Away message and filters: mutually exclusive with a warning in both screens, or the vacation command inside the managed script (changes M6)?
2. Spam guard: drop it from version 1, or keep it unproven? It needs a real spam delivery to decide, which this test server cannot produce.
3. Folder naming: Path only, or Id with the path as the mandatory second string (and accept the fallback to a same-named folder)?
4. May a mailbox sync rewrite the script silently after a folder rename (the design leaves this to the owner)? Without it, a rename silently sends mail to the Inbox.
5. "From contains": address only, or the whole From header?
6. Delete: Trash (design) or `discard` (PLAN.md)? Both validate.
