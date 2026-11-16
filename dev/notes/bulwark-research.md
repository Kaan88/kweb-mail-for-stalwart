# Bulwark: what it uses from Stalwart, and what is worth taking

Researched 2026-11-16 from the source, read only: `bulwarkmail/webmail` at 7cdedb6 (pushed 2026-11-15), `bulwarkmail/plugins` at 7430ea7 (the AI assistant is the plugin `ai-assistant`), the READMEs of `bulwarkmail/native` and `bulwarkmail/relay`, and the docs page `docs/features/overview.md` of `bulwarkmail/website`. Where a claim mattered it was checked in Stalwart's own source (`stalwartlabs/stalwart` main, `stalwartlabs/calcard` main). Nothing was run against a server. File names without a repository are in `bulwarkmail/webmail`; `client.ts` is `lib/jmap/client.ts` (580 KB; most of the Stalwart knowledge is in its comments, line numbers are approximate).

Bulwark targets Stalwart 0.16 and 1.0 and names the version for each quirk. We target 1.1. Where Bulwark says "1.0" it usually still holds for 1.1, but each item below says whether it was checked for 1.1.

The owner already runs Bulwark against the real server (PLAN.md, M12 stage 2: the filter script there is Bulwark's). So anything Bulwark writes into the account (tags, metadata, filters, an away-message event) is data our app meets.

## 0. The three findings that matter most

1. **Repeating events: Stalwart's name is `recurrenceRule` (one object), not `recurrenceRules`.** Our core writes and reads only the plural (`core/src/jmap/jscal.rs` lines 105, 201, 397, 637, 742). Stalwart's JSCalendar code (`calcard`, `src/jscalendar/types.rs`) knows only `recurrenceRule` and `excludedRecurrenceRule`; there is no plural anywhere in it. Bulwark converts on write (`cleanRecurrenceRules`, `client.ts` ~888, used for events and tasks) and on read (`normalizeStalwartPropertyNames`, `lib/calendar-event-normalization.ts`), and says 1.0 drops the plural for tasks. This explains our own notes in `core/src/jmap/calendars.rs` lines 7 to 12 ("does not expand repeating events, whatever expandRecurrences says"; "recurrenceRules is in the answer only when properties is left out"): the server most likely kept our plural as an unknown property and never knew the event repeats. If so: a repeating event made in the app is a single event for CalDAV clients, Bulwark and the server's own alerts; and a repeating event made anywhere else comes back as `recurrenceRule`, which our app does not read, so it shows only its first occurrence. **Not yet checked on the local server.** Check first: create a weekly event with the CLI, read it with `properties` naming `recurrenceRule`, and look at the `.ics` over CalDAV.
2. **A refused recipient does not fail the send.** Stalwart runs RCPT TO inside `EmailSubmission/set` and records a refusal as `deliveryStatus.<addr>.delivered = "no"` with the SMTP reply; when no recipient was accepted, nothing is queued, `undoStatus` is `final`, and the create still succeeds (Stalwart `crates/jmap/src/submission/set.rs`, around lines 715 to 785, read on main). `onSuccessUpdateEmail` then moves the message to Sent. Bulwark reads `deliveryStatus` back in the same request with an `EmailSubmission/get` on `#creationId` (`deliveryStatusCall`, `client.ts` ~1066; issue #1123; it names "550 5.1.2 Relay not allowed" and "452 4.4.5 Rate limit exceeded"). Our `core/src/jmap/send.rs` does not look at `deliveryStatus`, so such a message counts as sent and sits in Sent. This touches the rule "never lost".
3. **Submission records fill up and then nothing sends.** Every send leaves an `EmailSubmission` record; Stalwart counts them against the object quota `maxSubmissions` (Stalwart `crates/jmap/src/submission/set.rs` line 89, `crates/common/src/config/mailstore/email.rs` line 147). Bulwark says the default is 500 and that 1.0's own purge never runs (`client.ts` ~13026), so it destroys each immediate send's record right after the send and prunes records that can no longer be cancelled on `overQuota`, walking `EmailSubmission/changes` from state `n` for old ids. Stalwart main has an optional purge (`expungeSubmissionsAfter`, `crates/email/src/message/delete.rs` line 171); its default was not checked. Our app never destroys a submission record, and `overQuota` is not in `REFUSED_FOR_GOOD` (`send.rs` ~410), so after the limit every send would be retried for ever. Check on the local server: the tester's `maxSubmissions` and whether old records go away.

## 1. Stalwart and JMAP surface

Capabilities, methods and endpoints Bulwark uses, with where, what for, and whether we have them. "1.1" says whether it was checked for Stalwart 1.1.

| Name | Bulwark file | What Bulwark does with it | 1.1 | Ours |
|---|---|---|---|---|
| `urn:ietf:params:jmap:metadata` (Mailbox `metadata`, `privateMetadata`) | `lib/jmap/bulwark-metadata.ts`, `lib/shared-tags.ts`, `lib/settings-metadata-sync.ts`, `lib/templates/metadata-templates.ts` | Its own data in the Inbox's metadata, namespace `org.bulwarkmail.<key>`: shared tag names and colours of a group account (`tags`), team and personal templates, settings synced with the Bulwark phone app (8 shards `settings.shard.0-7` plus `settings.value.<name>`). Stalwart 1.0 limits: 8 KB per namespace, 64 KB shared and 16 KB private per mailbox; vendor namespaces need a dot. A pre-release 1.0 panicked on `Mailbox/get` naming `metadata`, so it is always asked in a request of its own and any failure means "not supported". | not checked | no |
| `Keyword/get` under `https://bulwarkmail.com/ns/jmap/keywords` | `client.ts` ~3290, ~3456 | Keyword counts and the keywords in use. A Bulwark capability (for its IMAP gateway `legacy-proxy`), not Stalwart; on Stalwart Bulwark scans `Email/query` + `Email/get keywords`. | n/a | no, not needed |
| `urn:ietf:params:jmap:emailpush` (PushSubscription `emailPush`) | `client.ts` ~13265 | Per-account filter on a push subscription, Stalwart >= 0.16.16. Refused as `forbidden` for an account the user does not own; `overQuota` past 15 subscriptions per account. | yes (our notes) | yes (`core/src/jmap/pushsub.rs`) |
| `urn:ietf:params:jmap:blob`: `Blob/get`, `Blob/copy` | `client.ts` ~6959, ~11157 | Many contact photos in one call for an export (one download per blob queues behind 4 parallel and 1000 a minute); copying blobs between accounts. `Blob/get` only in accounts the user is a member of. | not checked | no |
| `Email/parse` | `client.ts` | Opens an attached `message/rfc822` as nested mail. | not checked | no |
| `CalendarEvent/parse` | `client.ts` | Reads an `.ics` attachment to answer or import it. | not checked | no (we read invitations our own way) |
| `CalendarEventNotification/get, query, set` | `stores/calendar-event-notification-store.ts` | The server's record of every invitation, update and cancellation another participant made; shown as a toast and acknowledged (destroyed). Pulled on login and on its push type. | not checked | no |
| `ShareNotification/get, query, set` | `stores/share-notification-store.ts` | "X shared a calendar with you" and changed rights (RFC 9670 §3), same pattern. | not checked | no |
| `Principal/getAvailability` (`principals:availability`) | `lib/find-a-time.ts`, `lib/free-busy.ts`, `client.ts` ~8416 | Free/busy of attendees and "Suggest times" on a 30-minute grid in working hours. One id per call; 1.0 rate-limits at 100 calls a minute; `mayGetAvailability` on the user's own principal is false while directory queries are off. | not checked | no |
| `Principal/get` without ids | `client.ts` ~8074 | With directory queries off, still returns the user, their groups and those who shared with them. | yes (our notes) | partly (`jmap/sharing.rs`) |
| `Thread/get` | `client.ts` ~997 | Message count per thread, chained onto the list request. | n/a | no (we count from the cache) |
| `Quota/get` | `lib/quota.ts` | Storage use; uses `softLimit` when set below `hardLimit` (writes fail there). | yes | yes (`send.rs quotas`) |
| `ParticipantIdentity/get, set` | `client.ts` | Which address is "me" in invitations. | yes | yes (`calendars.rs`) |
| `SieveScript/validate`, `onSuccessDeactivateScript` | `client.ts` ~7689 | Stalwart answers `onSuccessActivateScript: null` without error but leaves the script active; only `onSuccessDeactivateScript: true` works. | yes | yes (`sieve.rs`) |
| `x:AccountSettings/get, set` | `lib/stalwart/principal.ts`, `stores/account-security-store.ts` | The live "Full name" (`description`): Stalwart copies it into the default Identity once and never again, so an admin's rename never shows. Also `encryptionAtRest`. | yes (our notes) | no |
| `x:PublicKey/*` + `encryptionAtRest` | `stores/account-security-store.ts` ~450 to 545 | Upload an OpenPGP key or S/MIME certificate, then switch Stalwart's encryption at rest on against it. | yes (`dev/notes/stalwart-admin-features.md`, at-rest) | no |
| `x:AccountPassword`, `x:AppPassword`, `x:ApiKey`, `x:MaskedEmail`, `x:ArchivedItem`, `x:Account`, `x:Tenant` | `stores/account-security-store.ts`, `lib/stalwart/masked-email.ts`, `client.ts` ~13409 | Account security, masked addresses, recover deleted items (Enterprise; a normal user is refused the `accountId` filter with `unsupportedFilter` and asked again without it). Community answers Enterprise objects with `forbidden` and a fixed detail. | yes | yes (`jmap/manage.rs`, `jmap/enterprise.rs`) |
| `x:OAuthClient/query, set` | `app/api/admin/oauth/setup/route.ts` | "Auto-configure OAuth": registers Bulwark's client in Stalwart with the admin's login. | not checked | not needed (we register dynamically) |
| `urn:inbuxa:jmap:registry` | `lib/stalwart/management-capability.ts` | A second management registry (field `enabled` instead of Stalwart's `enable`). Not Stalwart. | n/a | no |
| OAuth scopes `urn:stalwart:oauth:scope:full`, `security`, `settings` | `lib/oauth/scopes.ts` | Stalwart 1.0.3 (#1527). Stalwart lists all three in `scopes_supported` whatever a client may use, and fails the authorization with `invalid_scope` for one the client was not registered with; so Bulwark never adds them just because they are listed. | not checked | we add `settings` and `security` when listed (`core/src/auth/mod.rs` 52); fine while our registration names them |
| `POST /api/auth`, then `/auth/token` | `lib/auth/stalwart-password-login.ts`, `lib/auth/stalwart-login-request.ts` | Password plus TOTP login: 0.16+ no longer takes `password$totp` over Basic. 1.0 wants the authorization request as `query` and `consent: true`, answers with `location`. | n/a | not needed (passkey OAuth) |
| `/auth/revoke`, RFC 9207 `iss`, RFC 8707 `resource`, OIDC `end_session_endpoint` | `lib/oauth/*` | Revokes the refresh token on sign-out; checks `iss` on the callback. | not checked | revoke not found in `core/src/auth` (check) |
| `GET /.well-known/jmap` redirect | `client.ts` ~2094 | Safari drops Authorization on the redirect; Stalwart then answers 200 with empty accounts, not 401. | n/a | not affected (reqwest) |
| `403 application/problem+json` | `client.ts` ~247 | A login refused for good (deleted account, removed credential): stop all requests, or a firewall bans the address. Only Basic counts as a failed login; a bad bearer token does not (`lib/stalwart/stored-credential-budget.ts`). | n/a | low risk (bearer only) |
| `400 urn:ietf:params:jmap:error:limit` / `maxConcurrentRequests` | `client.ts` ~961, ~2486 | Stalwart allows 4 requests at a time per user and refuses the rest before any method runs, so the request is safe to replay; Bulwark retries after 200, 400, 800 ms plus jitter. | yes (setting in the 1.1 registry) | a temporary error, no quick retry (`send.rs read_responses`) |
| `maxCallsInRequest` (16 by default) | `client.ts` ~11895, `lib/jmap/request-limits.ts` | Splits big requests; a request over it is refused whole, which once stopped change detection silently. | yes | not read in the core (check the larger batches) |
| Download endpoint, no `Range` | `client.ts` ~12232 | Stalwart does not honour Range; Bulwark reads the first bytes and cancels. | not checked | downloads restart from zero (fine) |
| CalDAV `MKCALENDAR` | `client.ts` ~9269 | The only way to make a tasks-only calendar: `Calendar/set` cannot set the component set. | not checked | no |
| SSE `/jmap/eventsource` | `client.ts` ~11590 | Stalwart pushes the primary account only; shared accounts are polled every 20 s. | yes (our notes) | not needed (shared is read live) |
| WebSocket | none | Not used. | | |

## 2. Stalwart behaviour Bulwark works around, checked against our app

| Behaviour (Bulwark's words, version) | Where | Ours |
|---|---|---|
| `recurrenceRule` singular (calcard) | `client.ts` ~882 | **Gap**, finding 1. |
| Refused RCPT gives a successful create with `delivered: "no"` (0.16, 1.0; 1.1 source agrees) | `client.ts` ~1052 | **Gap**, finding 2. |
| `maxSubmissions` counts every record; destroying a record never stops delivery (1.0) | `client.ts` ~13026 to 13260 | **Gap**, finding 3. Our `dev/notes/stalwart-sending.md` already found that destroy does not cancel. |
| `maxDelayedSend` is advertised as 30 days, but the MTA refuses holds past `futureRelease` (7 days by default) with "501 5.5.4 Requested hold time exceeds maximum of N seconds" | `client.ts` ~420 | Send later: check that we cap at 7 days or read the limit from the error (not checked). |
| `undoStatus` of a create is wrong on 0.16.10 to 0.16.16; trust `EmailSubmission/get` | `client.ts` ~1379 | We read it back with get (`send_later.rs`). Fine. |
| `Email/query` `limit: 0` means no limit and returns every id; use `limit: 1` with `calculateTotal` | `client.ts` ~3245 | `core/src/jmap/admin.rs` 213 sends `limit: 0` to `x:QueuedMessage/query` and others; harmless if it holds there too, but may return every queued id. Use 1. |
| `total` only with `calculateTotal`; a loop on `total` stopped after one batch | `client.ts` ~4394 | Check folder-empty and delete loops. |
| Identity `name` may come as `Name <addr>`, which doubles the From header and the submission is refused | `client.ts` ~1503 | Not checked. |
| A group's identity is listed before the member's own (1.0) | `client.ts` ~5615 | Our default sender: check it does not take "the first". |
| `forbiddenFrom` for a MAIL FROM other than the identity's address | `client.ts` ~5807 | Handled as refused (`REFUSED_FOR_GOOD`). |
| Message-ids in `inReplyTo`/`references` may come with or without brackets; ids with spaces are dropped by the server | `client.ts` ~5674 | Check `send.rs` reply headers. |
| `onSuccessUpdateEmail` files in Sent only after the submission (encrypt on append) | `client.ts` ~5741 | Same as ours. |
| `$hasattachment` / `$hasnoattachment` on every message (1.0); `hasAttachment` comes only from them; `Email/copy` must carry them | `lib/jmap/server-keywords.ts` | Labels skip `$` keywords (`core/src/labels.rs` 94). Fine. |
| Keywords over 128 bytes or over 100 per message refused per message in `notUpdated` | `client.ts` ~3752 | Labels cap at 128 bytes. Fine. |
| Purely numeric ids in a patch path (`mailboxIds/0`) refused before 0.16.5 | `client.ts` ~181 | Fixed in the server we target. |
| A state below `sinceState` (restore, upgrade, 1.0 counting per collection) gets an empty delta; Bulwark decodes Stalwart's state (`s` + LEB128 in its own base32 `abcdefghijklmnopqrstuvwxyz792013`) and does a full fetch | `lib/jmap/stalwart-state.ts`, `client.ts` ~2883 | **Gap**: our sync trusts any `newState`. Cheap guard. |
| `Mailbox/get` without ids over `maxObjectsInGet`: 0.16 cuts silently, 1.0 says `requestTooLarge`; same for `FileNode/get ids:null` | `client.ts` ~651, ~10834 | Check `jmap/folders.rs` (more than 500 folders is unlikely). Files use query. |
| `Calendar/get` and `AddressBook/get` omit `shareWith` (and `isVisible`, default alerts) unless named | `client.ts` ~574, ~613 | Check `sharing.rs` names them. |
| `Calendar/set`: `isDefault` is read-only, use `onSuccessSetIsDefault`; any unknown key fails the whole update | `client.ts` ~9389 | Not used. |
| `inCalendar` (one id), not `inCalendars` | `client.ts` ~1486 | Same as ours. |
| `recurrenceOverrides` cannot be asked with `utcStart`/`utcEnd` | `client.ts` ~708 | Same as ours. |
| `expandRecurrences` works from 0.16.20 when both `after` and `before` are given, and synthetic ids are then accepted by `CalendarEvent/set` (probed with a harmless update) | `client.ts` ~9576 | Our note says it does not expand; see finding 1 before trusting either. |
| A `timeZone` the server's database lacks fails every calendar call (1.0); retry in UTC | `client.ts` ~1175 | Check what we send for an unusual zone. |
| Import must send `sendSchedulingMessages: false` or the server mails every attendee and may block | `client.ts` ~10131 | We have no import. Keep in mind. |
| A task needs `start` for its rule, and a bare due date is stored without DUE | `client.ts` ~855 | Check our tasks with a repeat. |
| Tasks made over CalDAV (Thunderbird) have no `@type`; detect by `due`/`progress` | `client.ts` ~679 | Check how our agenda tells tasks from events. |
| Contact `uid` must be sent or the card is stored without one (#644); photos as blobs only from 1.0 | `client.ts` ~8910, ~7747 | Check `jmap/contacts.rs` sends a uid. |
| Default calendar and address book are made on first touch; in a cluster parallel first calls make two | `lib/jmap/first-touch-gate.ts` | Single server, low. |
| `Email/copy` with `onSuccessDestroyOriginal` destroyed the wrong id before 0.16.15 and destroys the source even when the copy fails after | `client.ts` ~12294 | We do not use `Email/copy`. If moving between accounts is added: copy, then destroy. |
| Vacation capability is kept in the session but dropped from the account's capabilities when the user lacks the permission (#1488) | `client.ts` ~7428 | `vacation.rs` 56 checks the session only. Cheap fix. |
| A read grant on a shared folder reads back with `maySetKeywords` (1.0) | `client.ts` ~8633 | Note for shared mail. |
| Sender and subject sort: 1.0 collates itself; 0.16 sorts raw bytes | `client.ts` ~7051 | We sort by date. |

## 3. Features

"Ours" uses PLAN.md and CLAUDE.md as of 2026-11-16. "Not for a phone" means desktop or server-side only.

| Area | Bulwark feature | Ours | Mark |
|---|---|---|---|
| Mail | Conversations, unified inbox, All mail / Unread / Starred across folders | conversations yes; unified and cross-folder views no | missing and worth it (Unread and Starred across folders are cheap from the cache) |
| Mail | Search with chips and `word*` | search yes | have |
| Mail | Forgotten-attachment warning, empty-subject question, placeholder check | no | missing and cheap |
| Mail | Read receipts, asked and answered (MDN built by hand, `lib/mdn.ts`: blob upload, `Email/import`, `EmailSubmission/set`) | no | missing, worth it later |
| Mail | Archive by year or month | no | missing and cheap |
| Mail | Tags with colours, nesting, visibility; keyword `$label:<id>` | labels yes, keyword is the name or `kweb-<hex>` | **interop gap**: Bulwark's tags are `$` keywords our app ignores, and ours are not Bulwark's |
| Mail | Sender pictures: contact photo, BIMI, favicon (through the Bulwark server) | no | contact photo cheap; BIMI and favicon need a fetch from the phone |
| Mail | Sender checks from `header:Authentication-Results:all` (SPF, DKIM, DMARC; no "via" badge on a forged From); served from stored headers, cheap in the list | no | missing and worth it (phishing on a phone) |
| Mail | Booking cards from schema.org markup | no | not for now |
| Mail | Import/export eml, mbox, pst; `winmail.dat`; attached messages as nested mail (`Email/parse`) | no | nested mail worth it; the rest not for a phone |
| Mail | Verification codes shown in the list (`showVerificationCodes`) | no | missing and cheap; useful on a phone (copy from the notification) |
| Mail | Phishing report as ARF (`lib/phishing-report*.ts`) | spam report yes | enough |
| Mail | Templates with placeholders `{{date}}`, `{{sender_name}}`, kept in the account's metadata | no | missing and worth it |
| Mail | Send delay, scheduled send, undo | yes | have |
| Mail | Sub-addressing with tag suggestions | plus-tag handling in reply-all | partly |
| Calendar | Month, week, day, agenda, tasks; drag to edit | agenda and tasks | week and month views missing, worth it |
| Calendar | Find a time (free/busy) | no | worth it once sharing is used |
| Calendar | Invitation, update, cancellation notices (`CalendarEventNotification`) | no | missing and cheap (a notification) |
| Calendar | Birthday calendar from contacts | no | missing and cheap |
| Calendar | iCal feed subscriptions | no | not for now |
| Calendar | Away message blocks the calendar: one busy event `bulwark-ooo-<accountId>` while a dated away message is on (`lib/vacation-calendar.ts`) | no | the owner's calendar may hold such events; our away screen should not fight it |
| Contacts | Groups, several address books, vCard import/export | read and edit | groups missing |
| Contacts | Trusted senders kept in an address book of their own (`trustedSendersAddressBook`) | allow list on the phone (`remote_allow`) | interop: read Bulwark's book as allowed senders |
| Filters | Full rule builder, raw editor, other clients' rules kept | M12 stage 2, in progress | in progress |
| Vacation | Date range, audience, forward while away, reply templates, inside Bulwark's filter script | VacationResponse only | M12 stage 2 handles the script; the forward block is `lib/sieve/vacation-forward.ts` |
| Files | Tree, folder upload, preview, share, Office editing (WOPI) | tree, share | have the phone part |
| Security | S/MIME; encryption at rest against an uploaded key | M9 S/MIME and PGP; at rest no | at rest: missing and worth it (pairs with M9) |
| Security | App lock, remote wipe, session timeouts | no | app lock (biometric) missing and cheap on Android |
| Accounts | Several accounts, identities, shared mailboxes, delegates | yes; delegates no | delegates not for now |
| Admin | Setup wizard, policy, branding, plugins, marketplace | Stalwart admin screens | not for a phone |
| Plugins | Sandbox with hooks; auto-tag, e-invoice, parcels, translate, LanguageTool, Jitsi, Nextcloud, send checks, reply-all guardrail, external-mail warning, external-link warning, spam score | no | ideas only; external-mail label, link warning and reply-all guardrail are cheap in the core |
| Interface | Keyboard, guided tour, print, accessibility presets, 29 languages | Android's own | not for a phone, apart from languages |
| Settings | Settings sync between webmail and the Bulwark phone app through private Inbox metadata | no | only if we want one set of settings with Bulwark |
| Bulwark Mobile (`native`) | 32 home-screen widgets; contacts and calendars in Android's own providers (sync adapter, both ways, `docs/device-sync.md`); client certificates; in-app update from GitHub releases; push through its relay (FCM or UnifiedPush) | no (our push needs no relay) | inbox and agenda widgets and the in-app update are worth it for a sideloaded app; the sync adapter is large but valued |

## 4. Techniques worth copying

- **Read back what a send really did** (`deliveryStatusCall`, finding 2): one extra `EmailSubmission/get` on `#creationId` in the same request. Stalwart does not evaluate `#ids` result references into a `/set` response, so it must be a creation-id reference.
- **Keep submission records low** (finding 3): destroy an immediate send's record after the request returns; on `overQuota` prune records whose `undoStatus` from a fresh get is `canceled`, or `final` with `sendAt` in the past; never a `pending` one (destroying it does not stop delivery but makes it uncancellable).
- **State that went backwards**: decode Stalwart's `s` state (`lib/jmap/stalwart-state.ts`, 90 lines) and do a full fetch when `newState < sinceState`. A server restored from a backup otherwise answers old states with empty changes and the cache silently misses everything since.
- **Replay on `maxConcurrentRequests`**: the refusal comes before any method runs, so even a `/set` is safe to send again after 200 to 800 ms. Stalwart's limits per user: 4 requests at a time, 1000 a minute (`client.ts` ~6961).
- **A request of its own for anything that may fail as a whole** (metadata, `x:ArchivedItem`, Enterprise calls): a method error does not take the other calls down. Our admin summary already does this.
- **List properties**: never `attachments`, `bodyStructure` or `textBody` in a list: Stalwart parses the whole raw message for them (#1089). `header:...` properties come from stored headers and are cheap. Our `header_properties()` (`core/src/jmap/convert.rs` 21) is already lean.
- **Say why a lookup is refused**: Bulwark tells "the administrator has disabled directory queries" (Stalwart's exact wording) from a missing permission. We do the first already (`sharing.rs`, `NO_LOOKUP`).
- **Tests against a real Stalwart**: `integration/` runs 80 Playwright specs against Stalwart in Docker, bootstrapped declaratively from an ndjson plan (`integration/stalwart/plan-accounts.ndjson.tpl`, with `usePermissiveCors` and `allowDirectoryQueries`), switchable between 0.16 and 1.0 images; helpers send real mail over SMTP (`integration/tests/helpers/smtp.ts`). Our local server plus `dev/seed_mail.py` covers the same ground by hand; their spec names list cases worth repeating (shared moves, shared scheduled send, group sent copy, live counters).
- **Metadata as shared storage**: one namespace per item (`org.bulwarkmail.<key>`), JSON Pointer escaping for member patches (`~0`, `~1`), member-level patches so two devices do not undo each other, an `at` stamp per setting with the newest winning.

## 5. AI assistant

Bulwark's assistant is the plugin `ai-assistant` (bulwarkmail/plugins), running in a sandbox in the browser; the keys stay on the Bulwark server, which adds the key and sends only to the configured address.

| Topic | Bulwark | Ours (PLAN.md M7, `core/src/assistant`) |
|---|---|---|
| Providers | OpenAI-compatible (`/v1`; Ollama, LM Studio, vLLM) and Anthropic Messages (`anthropic-version 2023-06-01`); defaults `gpt-4o-mini`, `claude-haiku-4-5`; through a server proxy | the same two, direct from the core, with a Service dropdown |
| Consent | the first request to a service shows service, host and model | the same, per service and model, plus a separate chat consent |
| Caps | 30,000 characters per mail text (`src/lib/prompts.js`); 60,000 characters and 8 pictures (4 per message) per request; an attachment read 20,000 characters, up to 100,000 when the model asks (`src/lib/areas/context.js`); history 6,000 characters and 4 tool rounds per answer (`src/lib/chat.js`); 20 messages per thread summary, 50 per conversation or selection | 600,000 characters per run, 30 steps, 8 calls per step, 4,000 per user message (`chat.rs`); no pictures |
| Tools that read | search_mail, read_message, read_original_message, read_thread, read_messages, check_calendar (busy or free days), find_contact, read_my_settings, find_setting (the app's settings index, for "how do I"), list_filters, get_vacation, list_templates, find_file, list_folder, read_file, read_attachment | search_mail, list_messages, read_message, read_thread, list_folders, list_labels, list_events, read_event |
| Tools that change | archive, move, mark, tag, batch (50, admin 1 to 200), create_event, update_event, delete_event, create_task, create_contact, update_contact, answer_invitation, set and turn off vacation, create_filter (never forward, redirect, reject, delete or to Trash), change_setting, draft_email, draft_reply, draft_reply_to_senders, attach_file, save_attachments, create_file, create_folder, move_file, rename_file, insert_template, save_template, remember | archive, move, mark read or unread, star, label, add event or task |
| Never | sends, deletes mail, shares a file | the same, plus no contacts, files, filters, away message |
| Automatic mode | after 5 actions in one answer it asks anyway (`MAX_AUTO_ACTIONS`, `src/lib/actions.js`); always asks for Trash or Junk, anything out of Junk, **moving or archiving any message other than the open one**, a contact or recipient whose address the user did not type in the chat and that is not a participant of the open message (only typed chat text counts, not a subject, a highlighted passage or a saved prompt), invitations, event deletes, filters, vacation, batches, settings, contacts | allow-list `changes::automatic`, up to 10 messages per change and 20 per send; Trash, Junk and Drafts refused outright; **no rule about messages other than the one in view** |
| Context | attachments as text (PDF up to 50 pages, Word, Excel up to 5 sheets of 2,000 rows, PowerPoint, OpenDocument, ics, vcf); pictures shrunk and sent when the model can see them (tracking pixels left out, remote images never loaded, EXIF date and place only with a switch, off by default); the whole conversation without quotes; a list selection; @-mentions of mail, contacts and files | the open conversation's text (`ai_chat_about`), before the first message only |
| Memory | `remember` asks first; up to 50 notes of 200 characters (`src/lib/chat-memory.js`), sent with every request, editable | none |
| Chat tools | earlier chats (one running chat, moved to history after 8 hours idle; 30 chats, 400 KB), saved prompts (user's and admin's), `/` commands, edit the last question, answer again (not after an answer that did something) | one chat in memory, gone when closed |
| Catch me up | unread since the last visit sorted into "needs an answer" and "for information", today's events, tasks due, sent mail nobody answered in a week | no |
| Composer | rewrite as a marked diff (struck and highlighted words), replace or insert, one undo step; spelling fixes accepted one by one | rewrite sheet, replace |
| Who you are | name, addresses, plain-text signature of the first identity, time zone, local time, language, app and server names; admin and user instructions (user's win) | user instructions (M12 stage 1); check what else our system text says |
| Answers | Markdown with cards (quick replies, time slots, checklist, people, table); a link whose text is not its address shows the real site, also for punycode, non-Latin hosts and `name@host` tricks | plain text and cards for changes |
| Tests | e2e specs per area, among them `e2e/injection.spec.mjs` (prompt injection in mail) | mock service tests |

What ours lacks, in order of use: the "not the open message asks first" rule for automatic changes (safety, small); earlier chats kept per account; Catch me up; attachment text (PDF and Office first); memory notes; contact lookup and drafting a reply from the chat; pictures; saved prompts.

## 6. Interop with the owner's Bulwark data

- Tags: Bulwark writes `$label:<id>`. Names and colours of personal tags live on the Bulwark server (`/api/settings`); only a group account's shared tags are in the account (`org.bulwarkmail.tags` in the group Inbox's metadata, `{ "tags": { "<id>": { "label", "color", "visibility" } } }`). Our labels skip every `$` keyword (`core/src/labels.rs` 94), so Bulwark's tags are invisible in the app. Reading `$label:<id>` as a label named `<id>`, and writing new labels the same way, would make both clients show the same tags.
- Filters: M12 stage 2.
- Away message: Bulwark puts a busy event with UID `bulwark-ooo-<accountId>` in the default calendar and may keep the vacation, with a forward block, in its own filter script.
- Trusted senders: an address book Bulwark keeps for showing pictures; ours is on the phone.
- Settings sync with the Bulwark phone app: private Inbox metadata, format in `lib/settings-metadata-sync.ts` (FNV-1a of the name modulo 8 picks the shard). Only worth it if the owner wants one set of settings across Bulwark and our app.

## Worth doing in the app

Ordered by how much it matters. Effort: S under a day, M a few days, L a week or more.

1. **Verify finding 1 on the local server, then read and write `recurrenceRule` (one object) and `excludedRecurrenceRule`** (S to check, M to fix, with a repair of the app's own repeating events). Without it repeating events are probably wrong both ways: ours repeat nowhere else, and everyone else's show once in the app.
2. **Read `deliveryStatus` back in the send request and treat "no recipient accepted" as a failed send that keeps the draft** (S to M). Today a relay refusal or a used-up rate limit looks like a sent message.
3. **Keep submission records under `maxSubmissions`**: destroy an immediate send's record after success, prune on `overQuota`, and stop retrying `overQuota` on submission for ever (M). First check the limit and the purge on the local 1.1 server. Without it the account stops sending after some hundreds of mails.
4. **Show Bulwark's tags: read `$label:<id>` keywords as labels, and name new labels that way** (S to M). The owner uses both clients on one mailbox.
5. **Automatic AI changes ask first for messages other than the one in view, and for anything a mail could have asked for** (S). The cheapest guard against prompt injection that Bulwark has and we do not.
6. **Full fetch when Stalwart's state goes backwards** (S). Protects the cache after a server restore or upgrade.
7. **Sender checks from `Authentication-Results`** (M). A clear mark on a forged From is worth more on a phone, where the address is hidden.
8. **Notifications from `CalendarEventNotification` and `ShareNotification`** (S to M). The server already records "invited", "changed", "cancelled", "shared with you".
9. **Earlier chats and Catch me up in the assistant** (M each). The two assistant features most useful on a phone.
10. **Attachment text for the assistant (PDF, Office)** (M to L). Needs parsers in the core; pictures after that.
11. **Encryption at rest against a key from the M9 keyring** (M). The server API is known (`dev/notes/stalwart-admin-features.md`); it pairs with the keys the app already holds.
12. **The small checks of section 2** (S together): vacation capability per account, `limit: 1` instead of 0, the 7-day hold limit for send later, identity order and `Name <addr>` names, contact `uid`, `shareWith` named in `Calendar/get`, a quick replay on `maxConcurrentRequests`.
13. **Templates** (M), in the account's metadata if the local server's metadata works, else on the phone.
14. **Home-screen widgets (inbox, agenda) and an in-app update check against the release page** (M each). Bulwark Mobile has both; the update check matters for a sideloaded APK.
15. **Find a time with `Principal/getAvailability`** (M), once sharing is in real use; it needs directory queries on.

Not worth it: `Keyword/get` (not Stalwart), WOPI Office editing, mailbox import and export, the plugin system, desktop keyboard and layout features, the Basic-auth failure budget (we use OAuth only).

Not checked at all: anything against a running server. Items 1 to 3 should be checked on the local Stalwart 1.1 before any code is written.
