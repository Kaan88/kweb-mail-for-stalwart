REPORT: JMAP sharing on the local Stalwart (http://127.0.0.1:18080). Everything below was probed with real requests on 2026-11-14. Account ids: tester = "c", second@kweb.test = "d". Ids are short strings and are account-scoped.

## 0. Setup commands (verified)
Admin management via stalwart-cli works in normal mode with the `admin` login from etc/admin.txt (no recovery mode needed):
```
export PATH=~/tools/stalwart-test/node/bin:$PATH STALWART_URL=http://127.0.0.1:18080
export STALWART_USER=$(cut -d: -f1 ~/tools/stalwart-test/etc/admin.txt) STALWART_PASSWORD=$(cut -d: -f2- ~/tools/stalwart-test/etc/admin.txt)
python3 -c "import secrets;print('second@kweb.test:'+secrets.token_urlsafe(12), end='')" > ~/tools/stalwart-test/etc/second.txt; chmod 600 ~/tools/stalwart-test/etc/second.txt
stalwart-cli --no-color create Account/User --field name=second --field domainId=b --field 'roles={"@type":"User"}' --field description="Second test user" --field "credentials={\"0\":{\"@type\":\"Password\",\"secret\":\"$(cut -d: -f2- ~/tools/stalwart-test/etc/second.txt)\"}}"
```
That printed "Created Account d". Basic auth with second@kweb.test works on /jmap/. Second's password is in ~/tools/stalwart-test/etc/second.txt (mode 600), same format as tester.txt.

Directory queries are OFF by default (see section 3). The setting and the reload that makes it take effect:
```
stalwart-cli --no-color update Sharing --field allowDirectoryQueries=true      # singleton Sharing: fields allowDirectoryQueries (default false), maxShares (10; "max sharees per item")
stalwart-cli --no-color create Action --json '{"@type":"ReloadSettings"}'     # REQUIRED: without it Principal/query kept answering forbidden after 20+ s
```
Scripts are in the scratchpad: /tmp/claude-1000/-mnt-C-Users-kaank-Desktop-kweb-mail-for-stalwart/1ec96980-4871-4ff3-b43b-06ef4006739f/scratchpad/j.py (JMAP client for both users) and ws.py (minimal websocket client).

Other accounts exist that are not mine: e = filesq@kweb.test and f = files2@kweb.test. They appeared during my session, created by someone else. I did not touch them.

## 1. Capabilities and the JMAP session
Capability URIs on every session: urn:ietf:params:jmap:principals, :principals:availability, :mail, :calendars, :contacts, :filenode, :metadata, :blob, :quota. The tester's own account "c" has accountCapabilities "urn:ietf:params:jmap:mail:share": {} and "urn:stalwart:jmap": {} (both empty objects).

**`using` requirements (observed):**
- Mailbox/Email methods (including `shareWith` on Mailbox/set) need only `urn:ietf:params:jmap:mail`. `urn:ietf:params:jmap:mail:share` is NOT required.
- Calendar methods need `urn:ietf:params:jmap:calendars`.
- AddressBook and ContactCard methods need `urn:ietf:params:jmap:contacts`.
- Principal/* AND ShareNotification/* need `urn:ietf:params:jmap:principals`. Without it: `{"type":"unknownMethod","description":"Method ShareNotification/get requires capability urn:ietf:params:jmap:principals which is not present in the \"using\" property."}`. Adding mail:share instead does not help.
- Principal/getAvailability needs `urn:ietf:params:jmap:principals:availability`.

**Session of second after tester shared things.** "accounts" gains the tester's account:
```
"accounts": {
 "d": {"name":"second@kweb.test","isPersonal":true,"isReadOnly":false,"accountCapabilities":{...full set incl. submission, vacationresponse, sieve, quota, emailpush, principals:owner{"accountIdForPrincipal":"d","principalId":"d"}...}},
 "c": {"name":"tester@kweb.test","isPersonal":false,"isReadOnly":false,"accountCapabilities":{...}}
}
```
- primaryAccounts is unchanged: all entries are "d".
- The shared account's `name` is the owner's address (tester@kweb.test). There is no separate display name.
- Account "c" has accountCapabilities keys: mail, contacts, contacts:parse, calendars, calendars:parse, websocket, blob, principals, principals:availability, filenode, mail:share, urn:stalwart:jmap, metadata, principals:owner.
- It does NOT have: submission, vacationresponse, sieve, quota, emailpush.
- In the shared account the per-account limits change. contacts `mayCreateAddressBook:false`, calendars `mayCreateCalendar:false`, filenode `mayCreateTopLevelFileNode:false`. mail `mayCreateTopLevelMailbox:true` is still shown, but creating a root folder in the shared account is forbidden ("You are not allowed to create root folders.").
- principals:owner on c is `{"accountIdForPrincipal":"d","principalId":"c"}`. principalId is the OWNER's principal id (c). accountIdForPrincipal is the viewer's own account d, i.e. where to call Principal/get.
- Capabilities are a fixed list. They do NOT tell which data types are actually shared. Calling a method for a type that is not shared in that account returns `{"type":"accountNotSupportedByMethod","description":"The accountId given corresponds to a valid account, but the account does not support this method or data type."}`. Example: only a mailbox is shared, Calendar/get and AddressBook/get on c return this. The client must probe or handle this error per type.
- isReadOnly is true only when everything shared in that account is read-only. Observed: mailbox with only mayReadItems gives isReadOnly=true. With any write right anywhere it is false.
- The account appears in "accounts" only while at least one item is shared with a read right:
  - A mailbox with only `mayAddItems` shared: account NOT visible.
  - A calendar with only `mayReadFreeBusy` shared: NOT visible, and Calendar/get on c gives `accountNotFound` ("The accountId does not correspond to a valid account").
  - After the last share is removed, "c" disappears from the session. Calls with accountId c then give `accountNotFound`.
- Session `state` is a hash that changes with the set of accessible account/type combinations. Examples: none "33901491", mailbox only "bb55cd82", calendar only "ebf735bd", address book only "994a5a77", all three "3405204d". It does not change when only the rights level changes. So a client can detect "share added or removed" by session.state, and "rights changed" only via ShareNotification or a re-get.

## 2. Principals (RFC 9670)
**Principal/get**
```
["Principal/get",{"accountId":"d","ids":["c"],"properties":["id","type","name","email","description","capabilities","accounts"]},"x"]
```
Answer for the owner of a share (viewed by second):
```
{"id":"c","type":"individual","name":"tester@kweb.test","email":"tester@kweb.test","description":"Test user",
 "capabilities":{"urn:ietf:params:jmap:mail":{},"urn:ietf:params:jmap:contacts":{},"urn:ietf:params:jmap:filenode":{},"urn:ietf:params:jmap:principals":{},
  "urn:ietf:params:jmap:calendars":{"accountId":"c","mayGetAvailability":true,"mayShareWith":true,"calendarAddress":"mailto:tester@kweb.test"}},
 "accounts":{"c":{"name":"tester@kweb.test","isPersonal":false,"isReadOnly":false,"accountCapabilities":{...}}}}
```
- Principal/get works with accountId "d" (own account) and also with accountId "c". With "c" it returned the same data.
- `calendars.accountId` is the account id the viewer can use for that principal. It is null when nothing is shared with the viewer. `accounts` is null in that case too.
- `mayShareWith` is false when the viewer is that principal (can't share with yourself) and true for other principals.
- Other properties seen: `timeZone` (null). Principal state is the constant "n". Principal/changes and Principal/queryChanges answer `cannotCalculateChanges`.
- Principal/get with `ids:null` returns all principals once directory queries are on: 5 here.
- Without directory queries, Principal/get only returns (a) yourself and (b) principals that have shared something with you. For anyone else, even one you already shared TO, it returns `notFound`.
  - Tester asking for d (the sharee) gave `"list":[],"notFound":["d"]`.
  - With directory queries on, tester gets d's `{"id":"d","name":"second@kweb.test","email":"second@kweb.test","description":"Second test user","capabilities":{... "calendars":{"accountId":null,"mayGetAvailability":true,"mayShareWith":true,"calendarAddress":"mailto:second@kweb.test"}}, "accounts":null}`.
- All principals here are `type:"individual"`. No groups exist and I did not create any.

**Principal/query, how a client finds a user to share with**
- Default server: every call, with any filter or none, returns `{"type":"forbidden","description":"The administrator has disabled directory queries."}`. On a default server a client therefore cannot discover users. It has to be given the principal id, which equals the account id. Email-to-id lookup is impossible, apart from principals that already shared with you.
- After the setting and ReloadSettings in section 0:
  - `["Principal/query",{"accountId":"c"}]` gives `{"accountId":"c","queryState":"n","canCalculateChanges":true,"position":0,"ids":["b","c","d","e","f"]}`. It returns ALL users including admin and yourself.
  - Filters that work:
    - `{"email":"second@kweb.test"}` gives ["d"]. It is an exact match: "second" gives [].
    - `{"name":"sec"}` gives []. Exact match, so it needs the full name, which equals the address.
    - `{"text":"second"}` gives ["d"]. It is a substring match, also on the description ("Second test" matches).
    - `{"type":"individual"}` gives all. `{"type":"group"}` gives [].
    - `{"accountIds":["c"]}` gives ["c"].
  - Unsupported filters (`description`, `foo`) give `{"type":"unsupportedFilter","description":"description"}`.
  - `limit` and `position` and `calculateTotal` work: `limit:2` returned ids ["b","c"] with `"total":5`. The `sort` property seemed to have no effect: `email` descending still returned ascending order.
- Privacy consequence: enabling directory queries lets every user list every user on the server.

**Principal/getAvailability** (needs the availability capability): `["Principal/getAvailability",{"accountId":"c","id":"c","utcStart":"2026-11-20T00:00:00Z","utcEnd":"2026-11-21T00:00:00Z"}]` as second gave `{"list":[{"utcStart":"2026-11-20T10:00:00Z","utcEnd":"2026-11-20T11:00:00Z","busyStatus":"confirmed","event":null,"accountId":null}]}`. The shared calendar events with mayReadFreeBusy were counted as busy.

**ShareNotification** (RFC 9670 object, in the sharee's OWN account d, needs the principals capability)
`ShareNotification/get`, `/query`, `/changes`, `/set` with destroy all exist.
- Item shape: `{"id":"jooa70puezaa","name":"","changedBy":{"principalId":"d333333","name":"Recovery admin account","email":"admin"},"created":"2026-11-14T09:58:57Z","objectAccountId":"c","objectId":"j","objectType":"Mailbox","oldRights":{<all rights, true/false>},"newRights":{<all rights>}}`.
- `objectType` is "Mailbox", "Calendar" or "AddressBook". `name` is always "".
- One notification is created per share or unshare or rights change, from the tester's `shareWith` edits. Notifications for the other actions I tried (the sharee's own writes, plain data changes) did not appear. A reshare "old to new" creates one entry. No notification when you share with yourself.
- An unshare is an entry with `newRights` all false. The rights maps are always complete (every right key, true or false).
- `changedBy` is unreliable. For Mailbox changes it says principalId "d333333", name "Recovery admin account", email "admin", even though the tester made the change. For Calendar and AddressBook changes it says "Test user" (correct). Do not trust Mailbox attribution.
- ShareNotification/query supports `limit`, `sort` on `created`, `calculateTotal`, and filters `objectType` ("Calendar"), `objectAccountId` ("c"), `after` (timestamp). Without a filter it returned 37 items.
- ShareNotification/set destroy works (`{"destroyed":["jooa9lhwe7aa"]}`) and the item then goes to notFound. Create gives `{"type":"forbidden","description":"Cannot create share notifications."}`. The state string looks derived from the newest notification id (e.g. "sqdiit10mtpynyba").
- ShareNotification/get on the shared account "c" gives accountNotSupportedByMethod. Use the own account.

## 3. Sharing objects: property shapes
`shareWith` is a map `{principalId: {right: bool,...}}`.

**Mailbox rights (11 keys):** mayReadItems, mayAddItems, mayRemoveItems, maySetSeen, maySetKeywords, mayCreateChild, mayRename, maySubmit, mayDelete, mayShare, maySetMetadata.
- A Mailbox/get with no `properties` does NOT include `shareWith`. It must be requested explicitly. Calendar/get and AddressBook/get include it by default.
- `myRights` is a full map of the 11 keys.

**Calendar rights (9 keys):** mayReadFreeBusy, mayReadItems, mayWriteAll, mayWriteOwn, mayUpdatePrivate, mayRSVP, mayShare, mayDelete, maySetMetadata. Calendar properties include `isSubscribed`, `isVisible`, `includeInAvailability` ("all"/"none"), `defaultAlertsWithTime`, `defaultAlertsWithoutTime`, `timeZone`, `isDefault`, `color`, `description`, `sortOrder`.

**AddressBook rights (5 keys):** mayRead, mayWrite, mayDelete, mayShare, maySetMetadata.

**Reading shareWith:**
- The owner always gets the full map of all rights per principal (false for absent ones).
- A sharee without mayShare gets `"shareWith":null`.
- A sharee WITH mayShare (granted on a Mailbox "i" and a Calendar "d") sees the full `shareWith`, including all other principals.

**Share operations:**
- Share on create or update (these shapes all worked): `["Mailbox/set",{"accountId":"c","create":{"r":{"name":"Share-RO","parentId":null,"shareWith":{"d":{"mayReadItems":true}}}}}]`. Unlisted rights count as false.
- Same for Calendar/set and AddressBook/set (creates ids c/d/e/f and d/e).
- Whole-map replace: `"update":{"i":{"shareWith":{"d":{"mayReadItems":true,"maySetSeen":true}}}}`.
- Patch of one right: `"shareWith/d/mayAddItems":true` (works, set false to remove).
- Unshare: `"shareWith/d":null`, or `"shareWith":{}`, or `"shareWith":null` (all three removed the share), or an all-false rights object for d (`{"mayReadItems":false}`; this removed the share too).
- Errors:
  - Unknown principal: `{"type":"invalidProperties","description":"Account id zz is invalid."}`.
  - Unknown right: `{"type":"invalidProperties","description":"Invalid permission \"mayFoo\".","properties":["shareWith"]}`. The same applies when a right belongs to another type (mayReadItems on an AddressBook).
  - Non-boolean right: `{"type":"invalidProperties","description":"Permission \"mayReadItems\" must be a boolean.","properties":["shareWith"]}`.
- Sharing with yourself is silently accepted (no error).
- Quirk: maySetSeen implies maySetKeywords, and vice versa. Sharing `{"mayReadItems":true,"maySetSeen":true}` gives the sharee myRights with both maySetSeen and maySetKeywords true.
- A sharee can't share onward on Mailbox even when granted mayShare: `"shareWith/b"` on a Mailbox gave forbidden "You are not allowed to modify this mailbox." On a Calendar it worked (`{"updated":{"d":null}}`).
- Mailbox sharing is per mailbox and NOT inherited. When only a child is shared, the sharee sees the child with a `parentId` pointing to a mailbox they cannot see (`[('Child','l','m')]` where l is not in the list). When only the parent is shared, the child is invisible. Children created by a sharee under a shared mailbox got `myRights` all false (the response of Mailbox/set create shows it) and are invisible to the sharee (Mailbox/get gives notFound).

**What the sharee sees (Mailbox/get on c):**
- Only mailboxes with mayReadItems. Not Inbox. No `role` ("role":null).
- `isSubscribed` is false by default, per user (see section 4 for write rights).
- Counts (`totalEmails` etc.) are correct.
- Shared mailboxes seen as "Share-RW" and "Share-RO" in this test. Example myRights of RO: only mayReadItems true.
- A calendar with only mayReadFreeBusy is NOT returned to the sharee at all. A Calendar/get listed only the d, e, f calendars out of c, d, e, f.
- For shared calendars the sharee sees `isSubscribed:false`, `isVisible:true`, `includeInAvailability:"none"` by default. These flags are per user.
- Shared address books: `isSubscribed:false`, `isDefault:false`.

## 4. Reading and writing as the sharee (all against accountId "c")
**Reads:**
- Email/query, Email/get, Thread/get, Email/queryChanges, Email/changes, Thread/changes, Mailbox/query and Mailbox/changes work with accountId "c".
- Email/query returns only mails in shared mailboxes:
  - No filter: 3 shared mails. `{"inMailbox":"a"}` (unshared) gives `ids:[]`, not an error.
  - Email/get on an unshared id gives `notFound`, not forbidden.
- Blob download works through the shared account: `/jmap/download/c/{blobId}/x.eml?accept=message/rfc822` gave 200 for mails in shared mailboxes and 404 for unshared ones.
- ContactCard/query and /get, and CalendarEvent/query and /get, only return items in shared containers (free-busy only calendar: its event id "9" was notFound).
- `CalendarEvent/query` filters inCalendar and after/before work. Item rights are enforced with `forbidden`.

**Mailbox writes (error type is always `forbidden`):**
- Email/set keywords on RO-shared mail: "You are not allowed to modify keywords."
- Email/set create in RO or in unshared mailbox: "You are not allowed to add messages to mailbox i."
- Email/set destroy: "You are not allowed to delete this message."
- Move out of RO: "You are not allowed to delete messages from mailbox 8" (that id is numeric/internal).
- Mailbox/set create child under a mailbox without mayCreateChild: "You are not allowed to create sub mailboxes under this mailbox." Root folder in the shared account: "You are not allowed to create root folders." Rename without right: "You are not allowed to modify this mailbox."
- With the full-rights share, Email/set `keywords/$seen`, create (inline bodyStructure), Email/import, Email/copy d to c, and destroy all worked. Email/copy from c (shared RO) into own d also worked.
- `$seen` and `$flagged` set by the sharee are shared with the owner (the owner's Email/get showed `$flagged` and `$seen` for the mail the sharee flagged). There is no per-user seen state.
- Upload quirk: `POST /jmap/upload/c/` as second (and `/upload/d/` as tester) gives HTTP 400 `{"type":"about:blank","status":400,"title":"Invalid Parameters","detail":"One or multiple parameters could not be parsed."}` even for write-shared accounts. WORKAROUND that worked: upload the blob to your own account d, then use that blobId in `Email/import` or in `Email/set` bodyStructure with `accountId:"c"`. Both succeeded (attachment email created in a shared mailbox using a blob from d).
- Blob/upload and Blob/copy to the shared account: `accountNotSupportedByMethod`. EmailSubmission/set and Identity/get on "c": `accountNotSupportedByMethod`, so mail cannot be sent from a shared account, although `maySubmit` exists as a right.

**Calendar writes:**
- CalendarEvent/set create: forbidden "You are not allowed to add calendar events to calendar d." (RO; also b the unshared default and c the free-busy one). Allowed in the WriteOwn calendar (e) and the WriteAll calendar (f).
- Update on an event in the RO calendar: forbidden "You are not allowed to modify this event." Destroy: forbidden "You are not allowed to remove events from calendar d."
- Surprise: a sharee with only `mayWriteOwn` could update and destroy an event created by the OWNER (ev-own, which had no participants), and in the WriteAll calendar too. The own/all distinction was not enforced for that event.
- Privacy: a sharee's update with `"privacy":"private"` gave `{"type":"invalidProperties","description":"Only the calendar owner can set a privacy other than public.","properties":["privacy"]}`.
- Calendar/set by the sharee on name, color, sortOrder, timeZone, isSubscribed, isVisible, includeInAvailability, defaultAlerts succeeded EVEN on a read-only calendar. They are per-user overrides: the owner still sees the original name (checked). `description` is not an override: it gave forbidden "You are not allowed to modify this calendar." on d (RO) (earlier whole-set failure message was the same).
- Calendar/set on the unshared default calendar (b) as a sharee is not applicable (not visible).

**Address book writes:**
- ContactCard/set create in the RO book: forbidden "You are not allowed to add contacts to address book d."; update: forbidden "You are not allowed to modify address book d."; destroy: forbidden "You are not allowed to remove contacts from address book d." Create, update and destroy all worked in the RW book.
- AddressBook/set by the sharee on isSubscribed, name, description, sortOrder succeeded even on the RO book. They are per-user overrides: the owner's view was unchanged (name "Book-RO", isSubscribed false).

## 5. State changes and push
- Every account has per-account state strings. In the shared account c, Mailbox/get, Email/get etc. give state strings such as "shy" (the same value across types, a single account-wide counter). Mailbox/changes in c worked: it reported share events. Sharing an existing mailbox gave `"updated":["j"]` (not "created"), a rights change gave `"updated":["j"]`, unshare gave `"destroyed":["j"]`.
- Email/changes, Thread/changes and Email/queryChanges in c only list mails in mailboxes shared with the viewer (a mail added to an unshared mailbox did not appear).
- StateChange over the websocket (ws://127.0.0.1:18080/jmap/ws, basic auth, subprotocol jmap, message `{"@type":"WebSocketPushEnable","dataTypes":null,"pushState":null}`) and over EventSource (`/jmap/eventsource/?types=*&closeafter=no&ping=0`): the sharee gets NO StateChange for the shared account "c", for any change. Tested: the owner adds a mail to the shared mailbox, adds an event to the shared calendar, adds a contact to the shared book, the sharee itself writes into the shared mailbox. The owner's own websocket receives `{"@type":"StateChange","changed":{"c":{"Thread":"slm","Mailbox":"slm","EmailDelivery":"slm","Email":"slm"}}}` and `{"c":{"CalendarEvent":"sja","CalendarEventNotification":"sbm"}}` and `{"c":{"ContactCard":"sia"}}`. The sharee's websocket received nothing, for two websocket runs and one EventSource run.
- The ONLY push the sharee gets about sharing: when the owner changes shareWith, the sharee's own account gets `{"@type":"StateChange","changed":{"d":{"ShareNotification":"sqdeit1xvtlynyba"}}}`. A client that wants live updates for shared accounts must poll them (Email/changes etc. on c). It can use the ShareNotification push in d as the trigger to re-read the session and the shared containers.
- Sharee's own account d gets normal StateChange (Thread/Mailbox/EmailDelivery/Email) for its own changes.

## 6. Observed quirks, summary
1. Principal/query is forbidden by default (Sharing.allowDirectoryQueries=false), needs a ReloadSettings action after changing it.
2. Mailbox/get omits shareWith unless requested.
3. Upload to another accountId gives 400 (not forbidden). Use own-account blobs.
4. Free-busy-only shares are invisible in Calendar/get (they only matter for Principal/getAvailability).
5. mayWriteOwn does not stop edits of owner-created events without participants.
6. Mailbox ShareNotification.changedBy names the recovery admin.
7. Account capabilities in the shared account do not narrow to the shared types, probe per type.
8. The own/other `$seen` keyword is account-wide.
9. A child mailbox created by a sharee in a shared mailbox is invisible to the sharee.
10. The Mailbox rights maySetSeen and maySetKeywords come as a pair.

## 7. Could not make work / not tested
- Group principals as sharees: none exist, not tested. maxShares (10) enforcement not tested.
- FileNode sharing not tested (another session seems to cover it: accounts e and f).
- A sharee's `maySubmit` has no effect since submission is not available on the shared account.
- Resharing a Mailbox as sharee with mayShare is forbidden (see above). Sharing a Calendar onward worked.
- A WebPush (webpush-vapid) subscription for the sharee was not tested; only websocket and EventSource.

## 8. What I left behind on the local server
- User second@kweb.test (id d), password in ~/tools/stalwart-test/etc/second.txt.
- Server setting Sharing.allowDirectoryQueries = true (default is false). To revert: `stalwart-cli --no-color update Sharing --field allowDirectoryQueries=false` then the ReloadSettings action. I left it ON on purpose, so Principal/query can be tried.
- In the tester's account: mailboxes Share-RO (i, shared with d: mayReadItems), Share-RW (j, shared with d: all mailbox rights except mayShare and maySetMetadata), childrw (k), Parent (l, shared with d: mayReadItems), Child (m), Grand (n). All shares were restored to the baseline (RO/RW levels as originally created, calendars c/d/e/f = free-busy / read / write-own+RSVP / write-all, books d = read, e = read+write+delete). Plus test mails, events, contacts in them.
- Calendars Cal-FreeBusy (c), Cal-RO (d), Cal-WriteOwn (e), Cal-WriteAll (f); address books Book-RO (d), Book-RW (e); the second user's per-user overrides (names "x2", "hax", "hax2", "mybook") remain in his view.
- ~37 ShareNotifications in second's account d.
- Nothing was changed in /mnt/C/Users/kaank/Desktop/kweb-mail-for-stalwart.