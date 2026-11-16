# Device calendar sync: how Bulwark Mobile does it, and what kweb mail would need

Researched 2026-11-16, read only. Source: github.com/bulwarkmail/native, branch `main`, read raw
(not run). Bulwark's design doc `docs/device-sync.md` (1969 lines) is the main source and is cited
as "doc L<n>"; code is cited as file:line. The three `docs/native-sync-*.md` files are release
reports: they only mention device sync in passing (test-suite fixes, push catch-up) and add nothing
to the design. Nothing here has been tried on a device. Where Bulwark says "checked", that is its
claim (API 34 emulator, Google Calendar, Etar, Fossify Calendar, Stalwart 0.16.23).

Bulwark's sync does contacts and calendars. Tasks are not synced (doc L1856).

## 1. Architecture

```
Android SyncManager --onPerformSync--> DeviceSyncAdapter (Kotlin)
      --starts headless JS task "BulwarkDeviceSync", waits on a latch-->
 TypeScript engine (src/device-sync/engine/*)
      --JMAP over its own detached client--> server
      --NativeModules.BulwarkDeviceSync.query / applyBatch--> CalendarContract / ContactsContract
```

| Part | What | Where |
|---|---|---|
| Account type | `${applicationId}.account`, one Android account per signed-in app account; its name is the login, its `userData` holds only `registryId` (the link back to the app). No password or token is stored in AccountManager. | doc L107-127; `DeviceSyncAccounts.kt:50-70` (`ensure`) |
| Authenticator | `addAccount` never makes an account itself, it opens the app's settings by deep link; removal is allowed (providers then delete the rows); everything else is "unsupported". | `BulwarkAuthenticator.kt:23-64`, doc L128-141 |
| Sync adapters | Two `AbstractThreadedSyncAdapter` services, one class, one per authority: `com.android.calendar` and `com.android.contacts`. `supportsUploading=true`, `userVisible=true`, `allowParallelSyncs=false`, `isAlwaysSyncable=false`. Exported (the system binds them) but `startSync`/`cancelSync` ignore other callers. Run in the main process on purpose. | `res/xml/sync_calendar.xml`, `sync_contacts.xml`; `AndroidManifest.xml:100-120`; doc L142-192 |
| Contacts extra | `CONTACTS_STRUCTURE` meta-data and an `EditSchema` XML, or AOSP Contacts treats the account as read-only. Calendar needs nothing like it. | doc L148-155 |
| Enabling | `ContentResolver.setIsSyncable(...,1)`, `setSyncAutomatically`, `addPeriodicSync`; the account is created with automatic sync off and switched on by the app. | `DeviceSyncAccounts.kt:68-70, 93-107` |
| Permissions | Manifest: `READ_CALENDAR`, `WRITE_CALENDAR`, `READ_CONTACTS`, `WRITE_CONTACTS`, `READ_SYNC_SETTINGS`, `WRITE_SYNC_SETTINGS`, `READ_SYNC_STATS` (lines 16-23). No `GET_ACCOUNTS` (an authenticator sees its own accounts). The four runtime ones are asked only when the user turns sync on for that kind, after a rationale; "denied" leaves sync off, "blocked" offers the app's system settings. | `app/permissions.ts:1-47`, doc L1491-1494 |
| Credentials hand-off | There is none. The adapter passes only `registryId` and `accountName` in the task payload; the JS side loads the account from its own registry (`new JMAPClient().loadAccount(registryId)`, rebuilt once on auth failure). Bulwark chose this on purpose: refresh tokens rotate and a second owner would evict the account. | `DeviceSyncAdapter.kt:68-74, 141-148`; doc L39-70, L417-423 |
| Provider access | Kotlin is a thin port: `query(name, authority, json)` and `applyBatch(name, authority, opsJson)` take JSON ops, plan them, and enforce scope (only rows of our own account type, every table; sync-adapter URIs `CALLER_IS_SYNCADAPTER` so writes neither set DIRTY nor schedule syncs). | `BulwarkDeviceSyncModule.kt:207-230`; `ProviderScope.kt:31-116`; doc L545-548 |
| Run bookkeeping | Per run: random `runId`, latch, cancel flag; deadline 9.5 min (2.5 min for an expedited job); JS stops 30 s earlier and asks for `fullSyncRequested` to continue. | `DeviceSyncAdapter.kt:75-112, 180-185`; `DeviceSyncRuns.kt` |
| Push | The server's StateChange reaches the FCM/UnifiedPush service; `PushRouting.decide(data, routes)` reads the `changed` map (account id -> types), maps `ContactCard`/`AddressBook` and `CalendarEvent`/`Calendar` to `requestSync` for the matching Android account (routes stored by JS through `setPushRoutes`), and starts the mail task only if a mail type (`EmailDelivery`, `Email`, `Mailbox`) or anything unreadable is present. The push subscription adds the calendar types only while calendar sync is on, per account, and falls back to `EmailDelivery` alone if the server refuses. | `PushRouting.kt:22-69`; doc L1596-1618 |

Why the engine is in JS: Bulwark reuses its existing client (auth, mTLS, 429, session discovery)
and keeps the mappers pure so tests run without Android (doc L39-70). The Kotlin side "only
translates URIs, `ContentProviderOperation`s and `SyncResult`".

## 2. Calendar mapping

### Calendars (`calendars.ts:56-101`, doc L1085-1119)

| JMAP `Calendar` | `CalendarContract.Calendars` |
|---|---|
| `id` | `_SYNC_ID` = `<jmapAccountId>/<id>` (ids are `[A-Za-z0-9_-]`, so `/ # ~` never occur) |
| `name` | `NAME`, `CALENDAR_DISPLAY_NAME`; a shared account's calendar is `<name> (<account>)` |
| `color` | `CALENDAR_COLOR` (CSS to ARGB; absent: a palette colour) |
| `myRights` | `CALENDAR_ACCESS_LEVEL`: `mayWriteAll` OWNER(700), `mayWriteOwn` CONTRIBUTOR(500), `mayRSVP` RESPOND(300), else READ(200); a subscribed iCal feed is READ |
| user's address | `OWNER_ACCOUNT` (never null: Etar crashes) |
| `timeZone` | `CALENDAR_TIME_ZONE` (device zone if null) |
| (constants) | `SYNC_EVENTS=1` (default 0 hides every instance), `VISIBLE=1` on insert only (then the user's), `ALLOWED_REMINDERS="1,2"`, `ALLOWED_AVAILABILITY="0,1"`, `ALLOWED_ATTENDEE_TYPES="0,1,2,3"`, `MAX_REMINDERS=5` (0 when the app owns reminders), `CAN_ORGANIZER_RESPOND=0`, `CAN_PARTIALLY_UPDATE=0` |
| (shadow) | `CAL_SYNC2` = last server `Calendar` JSON; `CAL_SYNC3` = flags (`readOnly` reason). `CAL_SYNC1` is left unused on purpose: the provider sends it as the `feed` extra of every sync it requests. |

Not synced: the virtual birthday calendar, calendars without `mayReadItems`, task-only calendars
(first 50 objects all `@type: Task`). A read-only calendar takes no device edits (a dirty event is
rewritten from the shadow, a new one removed). Deleting a calendar row deletes its events (provider
trigger), so dirty events go up first.

### Events (doc L1121-1255, columns in doc L275-297, `calendar/columns.ts:76-98`)

Identity: `Events._SYNC_ID` = `<jmapAccountId>/<eventId>`; an exception is `<id>#<recurrenceId>` with
`ORIGINAL_SYNC_ID` = master's id; a local event waiting for upload is `~pending/<uid>`. A pending id
matters: the provider hard-deletes an app-deleted event with no `_SYNC_ID`, so a delete during a
create would vanish without trace (doc L233-243). Per-row sync columns:

| Column | Holds |
|---|---|
| `SYNC_DATA1` | the shadow: whole last server event with its `recurrenceOverrides` (masters and single events) |
| `SYNC_DATA2` | exception rows: the recurrence id exactly as the server's override key |
| `SYNC_DATA3` | pending create `{uid, target calendar}` |
| `SYNC_DATA4` | baseline: mapped columns, attendees, reminders as stored after our last write (text over 1 KB as a hash) |
| `SYNC_DATA5` | poison marker (back-off 1 h doubling to 24 h) |
| `UID_2445` | the event's `uid` |

| JSCalendar | Android |
|---|---|
| `start` + `timeZone` | `DTSTART` = the instant; `EVENT_TIMEZONE` = the zone (must be an IANA id; an unknown id silently becomes GMT) |
| floating (no zone) | written in the device zone; `SyncState.deviceZone` remembers it; a zone change rewrites clean floating events |
| `showWithoutTime` | `ALL_DAY=1`, `DTSTART` = UTC midnight, `EVENT_TIMEZONE=UTC` |
| `duration` | non-recurring: `DTEND`, `DURATION` null; recurring: `DURATION` = `P<n>D` / `P<s>S`, `DTEND` null; exceptions use `DTEND`. All-day is at least one day. |
| `title`, `description` | `TITLE`, `DESCRIPTION` (HTML is written as plain text and uploaded only when edited; over 64 KB it is truncated and never uploaded) |
| first `locations` entry | `EVENT_LOCATION` from `name`; later entries preserved only |
| `status` | `STATUS` 1/0/2; never NULL on write; device NULL means "no change" |
| `freeBusyStatus`, `privacy` | `AVAILABILITY` 0/1, `ACCESS_LEVEL` default/public/private/confidential |
| `color` | `EVENT_COLOR` (no Colors table) |
| `recurrenceRule` | `RRULE` (FREQ, INTERVAL, COUNT, UNTIL, every BY*, BYSETPOS, WKST); `rscale` other than gregorian, `skip`, leap months are not representable: written without them, timing and rule never uploaded, reported `ruleNotRepresentable` |
| override `{excluded: true}` | `EXDATE` (UTC list, or `YYYYMMDD` all-day) |
| other override (even `{}`) | an exception row: master patched with the override; `ORIGINAL_SYNC_ID`, `ORIGINAL_INSTANCE_TIME`, `ORIGINAL_ALL_DAY` |
| `participants` with `mailto:` | `Attendees` rows (email, name, relationship, type, status); `HAS_ATTENDEE_DATA=1`; `SELF_ATTENDEE_STATUS` never written; own address normalised to exactly `OWNER_ACCOUNT` |
| `organizerCalendarAddress` | `ORGANIZER`, only when there are participants |
| `alerts` before the start | `Reminders` rows: `MINUTES` = -offset, `METHOD` EMAIL for `action: email` else ALERT |
| `useDefaultAlerts` | the calendar's default alerts become the reminders |
| `uid` | `UID_2445` |

**Not mapped (kept on the server, never sent back):** `RDATE` (never written: an infinite rule plus
RDATE cannot be inserted), `excludedRecurrenceRule`/`EXRULE`, `keywords`, `categories`, `priority`,
`virtualLocations`, `links`, `relatedTo`, `sequence`, `locale`, `replyTo`, locations after the first,
participants without `mailto:`, alerts relative to the end, absolute alerts, alerts after the start,
HTML description. DST traps (doc L1148-1161): Stalwart silently drops a `start`/`until`/override key
inside a DST gap or overlap, so a gap time moves forward, an overlap time is sent in a fixed-offset
zone, a recurring event with an overlap start is not uploaded (`dstAmbiguous`).

## 3. Direction, deletions, conflicts, state

**Both ways.** Order in a run (doc L382-453): preflight, collections, download (`/changes` from the
stored state, or a full reconcile), upload, drop deselected collections, report. Upload is always
after download so merges see the newest server truth.

**State kept** (doc L299-380). Per Android account and authority one `SyncState` blob (JSON, v1,
via `CalendarContract.SyncState`): `owner` (registry id + server origin, a run for another owner
stops), per JMAP account `collectionsState`, `itemsState` (the `CalendarEvent` state the rows
describe, written in the same `applyBatch` as the last chunk of a `/changes` page, as its last op,
never before its rows), `selected`, `partial`, `stale`, `created`, `reconcile` (resumable full
reconcile: `from`, `phase`, `after`), `taskOnly`, `deviceZone`, `reminderOwner`. Per row: shadow +
baseline + pending-create + poison marker (table above). `stale` lists objects whose rows could not
be written; they are fetched first next run and upload nothing until then. `created` lists objects our
uploads made after `itemsState`, because Stalwart's `/changes` omits an object created and destroyed
since.

**Downloads.** `/changes` with `maxChanges: 256` and a `hasMoreChanges` loop, objects fetched with
explicit `properties` (with `properties: null` Stalwart leaves out `useDefaultAlerts`; `ids: null`
returns at most 500 silently), batches of `maxObjectsInGet`. `cannotCalculateChanges` or no state:
full reconcile (`/get ids:[]` for the state, `/query` by `inCalendar`, 5000 ids a page with
overlap). Safety: a reconcile that finds zero server objects while the device holds more than 10
rows aborts (`safetyAbort`) instead of wiping the phone.

**Uploads** (doc L728-892). Never a whole-object replace; JMAP patches per mapped property,
computed against the baseline (invariant 1, doc L509-520). Where Stalwart needs a whole map resent,
it is rebuilt from the fresh shadow and the call carries `ifInState`. Every `/set` carries
`ifInState`; `stateMismatch` means download again and retry (3 times, then `io` without progress).
Creates carry a client-minted `uid` written to the row with the target calendar before the `/set`;
a retry looks the uid up and adopts instead of duplicating. A `/get` of every touched id follows,
becoming the new shadow; DIRTY is cleared only if the row was not edited again meanwhile (the clearing
batch asserts the projection read before the upload, one provider transaction).

| Device change | Upload |
|---|---|
| dirty master or single event | patch of the changed units only (`title`, `description`, `location`, `timing`, `rule`, `status`, `availability`, `privacy`, `color`, `calendar`, each attendee, the reminder set, each EXDATE, each exception row) |
| new event (no `_SYNC_ID`) | claim (`~pending/<uid>`), uid lookup, then create in the row's calendar |
| `DELETED=1` | `destroy`; an event in several calendars loses only the synced memberships (`calendarIds/<id>: null`); `notFound` counts as done |
| exception row inserted/edited | `recurrenceOverrides/<key>` (a new override carries master's title, locations, participants, sequence; existing ones are patched one level deep only: deeper makes Stalwart store a partial override) |
| "delete this occurrence" | `recurrenceOverrides/<key>: {excluded: true}`; EXDATE removed: `null` |
| Etar "this and following" | master gets a capped `recurrenceRule`, the new series is a new event with a fresh uid |
| move to another calendar (Etar: delete + insert) | paired and sent as one patch (`calendarIds`), never across JMAP accounts |
| own RSVP | `participants/<id>/participationStatus` with `sendSchedulingMessages: true` |

Scheduling (iTIP) mail is sent only when asked, so Bulwark asks for changes a calendar app would
notify about (organizer with participants: title, time, rule, location, status, attendees, create,
destroy; attendee: own RSVP and destroy), doc L1257-1272. A `forbidden` retries once without it.

**Conflicts** (doc L680-726). Three-way per unit: base = shadow, local = rows, remote = new
server object. Both sides changed differently: **server wins**, `conflicts += 1`. Edit on the device
and delete on the server: **server delete wins**. Delete on the device and edit on the server:
**device delete wins** (a deletion is deliberate), unless the edit moved the object out of every
synced collection. A server change to a field Android does not show is not a conflict. Download
writes are guarded by asserts on DIRTY/VERSION/projection and re-planned up to 3 times, then the
object goes on `stale`.

**Deletion safety.** More than 50 local deletions and more than 20 % of the authority's objects in one
run: nothing is deleted, `tooManyDeletions` is set, Android's own notification and the app's "Review
deletions" ask "delete on server" or "bring them back" (doc L893-908).

**What the clever bits are for:** a baseline per row exists because the provider rewrites values it
stores (fills the organizer, zeroes all-day times, rewrites durations); comparing against a fresh
mapping of the shadow would see edits nobody made (doc L565-586). Lossy editors are handled (Fossify
re-inserts every attendee as required and rebuilds the RRULE; Etar's move re-inserts attendees; Google
Calendar has no move: "Copy to" plus delete only pairs inside one sync) (doc L656-666, L1776-1783).

## 4. Scope

| Item | Bulwark |
|---|---|
| Own calendars | the primary account of the calendars capability; on by default, user can deselect |
| Shared calendars | other JMAP accounts that advertise the capability; **off by default**, chooser warns if another signed-in account already syncs the same one; listed as `<name> (<account>)`; read-only if rights say so; refusal to list them leaves rows untouched and only logs |
| Read-only handling | edit/delete/new in a read-only calendar is put back from the shadow and reported (`readOnly`); the user's RSVP uploads only with `mayRSVP`; Stalwart 0.16.23 refuses even that (`forbidden`) |
| Tasks | not synced; task-only calendars stay off the device; follow-up "through the OpenTasks provider" (doc L1856-1866) |
| Several accounts | one Android account per app account; ids carry `<jmapAccountId>/`; `owner` in SyncState; mutex per (account, authority); a move between accounts is create + destroy |
| Several signed-in accounts, same login on two servers | the second is named by its registry id |

## 5. Schedule

| Trigger | Detail |
|---|---|
| Periodic | per authority and account: 15 min, 30 min, 1 h (default), 6 h, manual (`addPeriodicSync`) |
| Device edits | CalendarProvider schedules an upload sync about 30 s after any app write; an upload-only run with nothing dirty ends before JMAP is opened |
| Push | StateChange types `CalendarEvent`/`Calendar` added to the subscription while sync is on (app closed) |
| App open | SSE StateChange, the app's own successful mutations, return to foreground: `requestSync` after a 5 s coalescing timer, at most once a minute per account and authority; its own echo (state equals the last recorded) is skipped |
| After an upload | one follow-up sync, because the push of our own change arrives while the run is open and SyncManager drops a duplicate request |
| Manual | "Sync now": `requestSync` manual + expedited |

Doze/battery: the sync framework holds a wake lock for the whole `onPerformSync` (the adapter takes
none); SyncManager cancels a sync moving under 10 bytes in 60 s (the engine sends `Core/echo` after 40 s
without a request); a 429 ends the run as `io` with `delayUntil` from `Retry-After` (60 s default);
backoff is the framework's (30 s doubling to 1 h in doc's failure table, L1619-1648); hard errors are
reported without progress so SyncManager does not retry at once. Time budget and chunk limits: doc
L1649-1685. Scale checked on an emulator: 2000 events first sync 38.7 s, no-change run 0.7 s.

## 6. Failures and what the user sees

| Piece | What it does |
|---|---|
| `SyncReports` | maps the JS report to `SyncResult`: `ok` stats only; `io`/`internal` one IO exception (soft); `auth` one auth exception, no progress; `permission`/`unsupported`/`safetyAbort` `databaseError`, no progress; `tooManyDeletions` flag + count; `cancelled`; `disabled` (doc L1301-1335, `SyncReports.kt:37-110`) |
| `SyncProblemNotifier` | one notification per Android account, channel `bulwark_sync` ("Sync problems", default importance, `setOnlyAlertOnce`), tag `bulwark-sync:<account>`; tap opens only a `bulwarkmobile://` deep link, explicit intent; cleared when solved (`SyncProblemNotifier.kt:21-75`) |
| Settings | Settings > Calendar > "Sync to this device": a row per account with a switch (reads Android's `getSyncAutomatically`; "Paused in Android settings" if off there), a chooser sheet of calendars, who reminds (calendar app or Bulwark), interval, "Sync now", last sync time and result line, "Android account settings" (doc L1471-1497). Result lines by outcome in `components/settings/device-sync/status.ts` (auth: "Sign in again", permission: "Grant access", tooManyDeletions: "Review deletions", safetyAbort, cancelled, io, ok with a count of items not synced; conflicts are never shown because they resolve themselves) |
| Per-item errors | `plannerError`, `readOnly`, `rsvpRefused`, `instanceOnly`, `crossAccountMove`, `forbidden`, `dstAmbiguous`, `ruleNotRepresentable`, `remindersNotRepresentable`, `tooLarge`, `uidConflict`, `collectionGone` |
| Turn-off | switch off first (no queued sync re-downloads), then teardown under the mutex uploads dirty/deleted/new items (30 s), and asks "N changes made on this device haven't reached the server. Turn off anyway?" if any failed; with nothing waiting the rows go silently; the Android account goes when both authorities are off |
| Logout | same teardown before credentials go; an account dropped from the registry keeps its Android account with sync off and one "Sign in again" notification, so device edits are not lost |
| Removed in Android settings | detected by listener and on foreground; the store marks it; never recreated by the app |

## 7. Contacts, briefly (doc L910-1081, L1301-1335)

Same machinery, same adapter class, authority `com.android.contacts`, `RawContacts.SOURCE_ID` =
`<jmapAccountId>/<cardId>`, `SYNC1` = address books, `SYNC2` shadow, `SYNC3` pending create, `SYNC4`
poison, `Data.DATA_SYNC1..3` entry key, photo hash, baseline. One unit per name, entry (`emails:work1`),
note, photo, group membership. Groups are `kind: "group"` cards. Read-only books mark
`RAW_CONTACT_IS_READ_ONLY`. Server wins on a same-field conflict; device delete wins. Needs the
`CONTACTS_STRUCTURE` XML. Photos as 512 px JPEG, over about 512 KiB stay on the server. The
"Trusted Senders" book (an allow-list, not a book) is off by default. Known: Google Contacts may not
show third-party accounts; Fossify Contacts re-inserts rows so a cleared field comes back. Not
needed for the calendar work; one adapter class can serve both authorities.

## 8. What kweb mail already has (checked in our code)

| Piece | Where | Use for a sync adapter |
|---|---|---|
| Calendars with rights, colour, visible flag | `Core::calendars`, `jmap/calendars.rs:56-78`, `model.rs` `Calendar` (id, name, colour, is_default, is_visible, may_write) | direct; lacks `mayRSVP`, `mayReadItems`, timezone, subscribed flag |
| Shared calendars | `Core::shared_calendars(account)` (`ffi.rs:1736`), `calendars_in`, `*_in(account, ...)` | shape exists for a later stage |
| All events of calendars, whole | `events_in` (`calendars.rs:122-152`), by `inCalendar` with paging, then `CalendarEvent/get` with no `properties` | full download works; no `/changes`, no state kept |
| One event + state | `event_get_in` returns the event and the `CalendarEvent` state (`calendars.rs:155-162`) | gives `ifInState` for writes |
| Write with `ifInState`, retry once | `change()` (`calendars.rs:235-262`) | the pattern Bulwark uses; our patches are built from `EventInput` |
| Create, update, delete, RSVP, task done | `save_event`, `delete_event` (one occurrence = `exclude_patch`), `respond_to_event` (`sendSchedulingMessages`), `set_task_done` (`ffi.rs:1187-1250`) | usable for the simple cases |
| JSCalendar <-> our model | `jmap/jscal.rs` (1968 lines): `create_object`, `update_patch`, `validate`, `current_input`, `respond_patch`, `exclude_patch`; `recur.rs` expands rules | the base for a unit-level patch builder |
| Alert push | `CalendarAlert` in `TYPES` (`ffi/webpush.rs:72`), `jmap/calendar.rs` builds a `Reminder` from one `CalendarEvent/get`; Android `ReminderRules.kt` | our own reminders already exist and work on the owner's phone |
| One core per account | `android/.../accounts/` (`AccountSession.kt`, `AccountRegistry.kt`) | an adapter must ask for the session of the named account, not "the" core |
| Calendar UI | `ui/calendar/*` (agenda, edit, event screens) | independent; the new feature is a switch in settings |

**What is missing:**

1. **The repeat name.** `jscal.rs` reads and writes `recurrenceRules` (plural) at lines 105, 201, 397, 637,
   742; Stalwart's name is `recurrenceRule` (one object) (`dev/notes/bulwark-research.md`, finding 1, not
   yet verified on the local server). Bulwark names `recurrenceRule` in its explicit `properties` and
   writes `object.recurrenceRule = rule` (`calendar/create.ts:86`); it also tolerates the plural for
   reading (`calendar/exceptions.ts:35`). A sync built on our current reader would show every series
   once and could write plural properties the server ignores. **Fix finding 1 first; it blocks this.**
2. **A changes feed.** No `CalendarEvent/changes`, no stored `CalendarEvent` or `Calendar` state, no
   resumable full reconcile, no per-object created/stale lists.
3. **Overrides.** We handle exclusion of one occurrence and move of the start; we do not model
   `recurrenceOverrides` with changed fields (an edited single occurrence) beyond display, and
   `has_complex_repeat` events are left alone.
4. **A row-to-patch path.** `update_patch` takes `EventInput` (title, place, description, start, zone,
   duration, all-day, repeat, alert minutes). A device sync needs a diff against a baseline per unit,
   with extra units (status, availability, privacy, colour, attendees) and the shadow whole.
5. **Provider port.** The core has no callback interface for a content provider; its callback
   interfaces today are the secret store and the push listener.
6. **Stalwart traps found by Bulwark that our code does not know yet** (`calendars.rs` header notes
   partly overlap): a patch into a map the event lacks fails (we know; Bulwark resends whole maps with
   `ifInState`); override patches deeper than one level make a partial override; empty maps are
   dropped; DST gap/overlap times are dropped; `uid` lookups read an asynchronous index (create then
   query can miss); keys for CalDAV-made events are regenerated on every read.
7. **Settings and permission flow, push routing for `CalendarEvent`/`Calendar` types.**

## 9. Proposed design for kweb mail

Principle: Bulwark's architecture ports well if the engine moves from JS to Rust. The core drives a
small provider port that Kotlin implements; Kotlin never talks to the server, the adapter only asks the
core of a named account to "sync now" and does provider reads and writes.

```
SyncManager -> CalendarSyncService (Kotlin AbstractThreadedSyncAdapter)
   -> Accounts.session(name).core.device_calendar_sync(port, flags)   [blocking, bounded]
        core (Rust): JMAP (own connection), mapping, merge, patches
        port (Kotlin, UniFFI callback interface): query(...), apply_batch(...), read/write sync state
```

The engine is testable in Rust against a fake provider (as Bulwark does, doc L1686-1716), without
Android; Kotlin has JVM tests for URI and op translation.

### Steps

| # | Step | Core | Android | Effort |
|---|---|---|---|---|
| 0 | Fix `recurrenceRule` singular/plural (finding 1) with a repair of the app's own events; verify on the local server | `jscal.rs`, `calendars.rs`, tests | none | M |
| 1 | Read-only mirror: account type, authenticator, calendar sync adapter, XML, runtime permission flow, Settings switch per account, per-calendar chooser, calendars + non-recurring + recurring events (RRULE, EXDATE, exceptions as rows) written to the provider with access level READ. Full download only, no state, rewritten on each run when the server's state changed. | `CalendarEvent/get` with explicit properties, state, mapper `event -> rows`, port trait, fake provider tests | `sync/` package (adapter, authenticator, account helper, port impl), settings screen, string resources | L |
| 2 | Incremental download: `CalendarEvent/changes` and `Calendar/changes` with `maxChanges`, `cannotCalculateChanges` full reconcile, state stored in the provider's SyncState blob (written with its last chunk), `safetyAbort` | changes feed, SyncState model, reconcile | none | M |
| 3 | Push and schedule: add `CalendarEvent`, `Calendar` to the push types and route to `requestSync`; periodic interval (default 1 h); foreground request on state change; one follow-up after an upload; `SyncResult` mapping; problem notification channel | push types, a "what changed" hint | `PushRouting`-like decision, channel, `SyncReports` | M |
| 4 | Two-way for simple events: baseline per row, three-way merge per unit (server wins on conflict, device delete wins), patch upload with `ifInState`, uid-adoption on create, `~pending/` ids, deletion threshold, read-only calendars put back | merge, patch builder (extend `jscal`), poison markers | none beyond port | L |
| 5 | Recurrence upload: exceptions, EXDATE, "this and following", pairs for Etar moves; RSVP | overrides, split logic | none | L (largest risk) |
| 6 | Attendees and reminder owner choice; scheduling messages; shared calendars (off by default) | participants, `*_in(account)` | chooser | M |
| 7 | Contacts on the same adapter | `jscontact.rs` mapping | `EditSchema` XML | L (separate project) |

Reasonable first release: steps 0 to 3 plus 4 for non-recurring events, edits to a recurring series
refused and put back as Bulwark's `readOnly` does ("instance/series edits not sent" reported).
Rough total for 0 to 4: 3 to 4 weeks of focused work for one developer, plus device testing; 5 is
another 2 weeks alone; these are guesses.

### Rules kept

| Rule | How |
|---|---|
| Kotlin never talks to the server | the adapter calls one core function; the port only touches CalendarContract |
| One core per account | one Android account per kweb account; the adapter looks the core up by account name (`registryId` in `userData`, like Bulwark) |
| No credentials in AccountManager | `addAccountExplicitly(account, null, userData)`; the authenticator opens the app's settings and refuses tokens |
| Nothing of a shared account in the cache | the sync state, shadow and baseline live in the provider's own columns, not the core's cache. **Open question:** a shared calendar written into CalendarContract becomes readable by every app holding `READ_CALENDAR`. Our rule says the cache holds nothing of a shared account; the provider is not our cache but is outside our control. Proposal: shared calendars stay out until the owner decides, and then only per calendar, off by default, with a plain consent text. |
| Keys and bodies never in logs | log only counts, outcome and ids, never titles, descriptions or locations (a calendar event is body-like data); tags `KwebMail`; errors without the server's text |
| Sending twice | uid minted on the client and written to the row before `/set`; a retry looks the uid up in the target calendar and adopts it (Bulwark invariant 3) |
| A save patches only what the app owns | same as Bulwark invariant 1: per-unit patches against the baseline; fields not mapped are never sent |

### Switches

Per account: a switch "Show in the phone's calendar apps" under the account's settings (Calendar), asks
`READ_CALENDAR` and `WRITE_CALENDAR` after a rationale (what is written, that other apps with
calendar access can read it). Per calendar: a chooser with the primary account's calendars on and
shared ones off. Interval choice and "Sync now" as in Bulwark's settings. Off removes the rows after
uploading waiting changes, and asks if any could not be sent. Removing the Android account in system
settings is respected and not undone.

### Reminders

kweb already shows server alerts itself (`CalendarAlert` push, works on the owner's phone). If
Reminders rows were also written, Google Calendar and others would remind twice. Bulwark makes this a
per-account choice and defaults to the calendar app when its own reminders are off (doc L1274-1299). For
kweb: default to **the app reminds** (`MAX_REMINDERS=0`, no Reminders rows written, reminder rows added
by other apps neither uploaded nor deleted), and later offer the other choice.

### Checking on the emulator

Whether the `kweb35` image carries Google Calendar or the AOSP calendar app is **not known; to be
checked** (`adb shell pm list packages | grep -i calendar`). Check three sides as Bulwark does: provider
rows (`adb shell content query --uri content://com.android.calendar/events ...`, with the
sync-adapter flag where needed), the server (JMAP calls or the CLI), and the app's screen. Scenarios
to repeat from doc L1869-1969: two zones, an all-day event, a floating event, a weekly series with an
excluded and a moved occurrence, two alerts; an edit, a create and a delete in the calendar app go up
once; process killed between a create's `/set` and its identity write; deletion threshold with 60
events; permission revoked; account removed in system settings; sign-out. A debug broadcast receiver to
create the account without UI (as `DeviceSyncDebugReceiver.kt`) saves a lot of tapping. Also test with
Etar or Fossify Calendar, since they edit differently from Google Calendar.

## 10. Attribution and licence

**Licence:** the repository's `LICENSE` file begins "Bulwark Mobile, Copyright (C) 2026 Bulwark
Project Authors. This project is licensed under the GNU Affero General Public License, version 3
only, with the additional permission for app store distribution below." (GitHub lists it as "other"
because of the added permission.) The addition is an AGPL section 7 permission to convey through an
app store, conditional on: (a) offering the Corresponding Source at no charge from a public network
server, (b) stating where the source is, (c) adding no further restrictions. It is stated to be
provisional, with a "Reservation": it applies to contributions on or after 30 September 2026 and to
older parts only where their authors agreed (discussion bulwarkmail/discussions/1113). The Android
sync code is not marked as under a different licence.

**What that means for us.** kweb mail is **MIT** (`LICENSE`, `Cargo.toml` `license = "MIT"`).
AGPL-3.0 is strong copyleft: copying or translating Bulwark's TypeScript or Kotlin into our code
(a derived work) would require our combined work to be offered under AGPL-3.0 with network-use source
obligations; MIT does not permit that on its own. So:

- Take the **design and facts** only: the idea of one Android account per app account, the column
  usage table, the invariants, the list of server traps, the test scenarios. Ideas, protocols, and
  facts about Android's and Stalwart's behaviour are not covered by copyright.
- Do **not** copy or closely paraphrase code, tests or doc prose; write our own from Android's
  documented provider contract and our own probes. This note describes Bulwark's design in our words and
  quotes no code. Anyone implementing from it should work from this note and the Android documentation,
  not from Bulwark's source open next to the editor, and should verify behaviour against the local
  server. If the owner ever wants to reuse code, the whole project would have to go AGPL-3.0, which is
  an owner decision (the "Filters" entry in `AboutScreen.kt:47` already lists AGPL-3.0 for the Bulwark
  filter format, which is a rule format, not copied code; check that wording is intended).
- **Credit** in three places, as the plan for filters already does: an entry in the About screen's
  libraries/credits list ("Device calendar sync design follows Bulwark Mobile (github.com/bulwarkmail/native),
  AGPL-3.0 with an app store permission"), a line in `CHANGELOG.md` when the feature ships, and a sentence
  in PLAN.md and in the source of the new module naming where the design comes from and that no code
  was taken. Not legal advice; if code is ever copied, ask the owner first.

## Open questions

1. Is finding 1 (plural vs singular) real on the local Stalwart 1.1? Everything else waits on it.
2. Shared calendars in the phone's provider: allowed at all, given the "nothing of a shared account
   outside" rule? Default proposal: no.
3. Reminders: app only (proposed) or choice from the start?
4. Does the `kweb35` image have Google Calendar, AOSP Calendar, or neither (install Etar or Fossify
   through adb)? To be checked.
5. Whether the Rust engine may use the core's blocking tokio runtime inside `onPerformSync` for up
   to 9.5 minutes (Bulwark's deadline), and whether a second core instance per sync run is allowed
   next to the foreground one for the same account (one core per account says it should be the same one).
6. The owner's wish for the first release: read-only mirror (steps 0 to 3), or two-way from the start.
7. Is the About line "AGPL-3.0" for the filter format intended, given our code is MIT (see section 10)?
