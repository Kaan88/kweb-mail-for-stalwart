# Stalwart 1.1: the server's notices (CalendarEventNotification, ShareNotification)

Probed on the local server (2026-11-16) as `tester` (account `c`) and `second` (account `d`) with Basic auth. The app reads
them in `core/src/jmap/notices.rs`; the pass that shows them is `core/src/notices.rs`.

## What makes one

- **CalendarEventNotification** is made in the user's own account when someone else writes into the user's calendar: the
  tester shared calendar `b` with `second` (write rights), `second` made, renamed and deleted an event in it with
  `CalendarEvent/set` and `accountId: "c"`, and the tester got three records. The user's own writes make none. An iTIP
  invitation made by `second` with the tester as a guest (`sendSchedulingMessages: true`) made no record, no mail and no
  event in the tester's account on this server (no scheduling over the local test setup), so "invited" is only seen
  when the event snapshot has `participants`; not seen on the server.
- **ShareNotification** is made in the sharee's own account when `shareWith` is set, changed or taken away (see
  `stalwart-sharing.md`). The newest one of the probe: `objectType` `Calendar`, `objectAccountId` `c`, `objectId` `b`,
  `oldRights` `{}`, `newRights` the rights given; `changedBy` names the right person for a calendar and the recovery
  administrator for a mailbox, so the app names the sharing account from the session (`accounts[objectAccountId].name`).

## Shape (CalendarEventNotification/get)

```
{"id":"b","created":"2026-11-16T13:45:24Z","type":"created"|"updated"|"destroyed",
 "changedBy":{"name":"Second test user","email":"second@kweb.test","principalId":"d","calendarAddress":"mailto:second@kweb.test"},
 "comment":null,"calendarEventId":"bc","isDraft":false,
 "event":{"start":"2026-11-26T10:00:00","title":"...","duration":"PT1H","timeZone":"Etc/UTC","uid":"...","@type":"Event","version":"2.0"},
 "eventPatch":null | {"title":"new title","updated":"...","sequence":1}}
```

An update has the old snapshot in `event` and what changed in `eventPatch`; a destroy has the last snapshot.

## Methods and push

- `CalendarEventNotification/query|get|set` need the calendars capability; `ShareNotification/query|get|set` the principals one.
  `query` takes `sort` on `created` and `limit`; `set` with `destroy` removes (the acknowledgement); a gone id is `notFound`.
- The EventSource stream (`types=*`) sent `{"changed":{"c":{"CalendarEvent":"see","CalendarEventNotification":"sai"}}}` for each
  write above, so subscribing to `CalendarEventNotification` and `ShareNotification` reaches the app. A Web Push subscription
  accepts both types (`types` null stores all 23, the app names five).
- The state strings are Stalwart's own (`s` and a base32 counter), see `core/src/jmap/state.rs`.

## Other things found on the same pass

- `x:Account/query` with `limit: 0` returns no ids on 1.1 (the total is there), `limit: 1` one id: both work; the app asks for 1.
- A contact made without `uid` is kept without one; the app sends `urn:uuid:...`.
- Holds: `HOLDFOR` 604800 is accepted, 604801 is `forbiddenMailFrom` "501 5.5.4 Requested hold time exceeds maximum of 604800
  seconds." (the session still says `maxDelayedSend` 2592000).
- Session `maxConcurrentRequests` is 4; 80 simultaneous `Email/query` requests were all answered (not enforced at that load).
- The state of `Email` and `Mailbox` is a counter that goes up by one per change (7365, 7366, ...); see `state.rs`.
