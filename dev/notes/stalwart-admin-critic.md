# Sceptic's review of the three Stalwart admin/Enterprise reports

Checked against the public source already cloned in the scratchpad: `stalwartlabs/stalwart` e7cde9b (tag v1.1.0, 2026-11-09) and `stalwartlabs/website` 232fa1b (2026-11-13). Nothing was sent anywhere, nothing was changed, and no server was contacted.

## A. Claims a design would lean on that are weaker than they look

1. **"Masked email works for every user with the ordinary OAuth client" is not true on any released server.**
   - In v1.1.0 it is blocked in three places, all under `#[cfg(feature = "enterprise")]`: the JMAP gate (`crates/jmap/src/registry/mod.rs:24-43`), the scope mapping of `sysMaskedEmail*` to `settings` (`crates/common/src/auth/oauth/scope.rs:503-510`) and the validator (`crates/jmap/src/registry/set.rs:383-392`).
   - The only evidence for the move is the website: `src/content/pages/compare.yml:61` has `["Masked email addresses (privacy aliases)", "yes", "yes"]`. No commit or changelog entry was found; `CHANGELOG.md:1072` still says "(*Enterprise*)".
   - Whether `settings` will still be the scope after the move, and whether the object shape stays, is unknown.
   - No report made a single successful `x:MaskedEmail` call. Every shape and error text is from docs or source.

2. **Nobody tested a true open build.**
   - The local server answers `edition: "community"`. By `crates/http/src/auth/permissions.rs:38-49` that means an Enterprise build without a licence; a build without the feature answers `"oss"`.
   - So "open edition" in the third report means "Enterprise binary, no key".
   - What an `oss` build answers to `x:MaskedEmail/*`, `x:Metric/*` or `x:Trace/*` is unverified. There `assert_enterprise_object` always returns `Ok(())` and the mapping modules are not compiled (`registry/mapping/mod.rs:48-55`), so the "Enterprise edition" text will not appear.
   - The detection recipe (match the description substring) therefore fails on `oss` builds. The design must handle three edition values.

3. **Only four objects are hard-gated; the "10 Enterprise objects" list is a UI flag.**
   - Source: the gate covers `MaskedEmail | ArchivedItem | Metric | Trace` only.
   - The third report contradicts itself: section 3 says all 10 "answer the Enterprise error on get, query and set", then says `AiModel`, `Alert`, `SpamAi`, `MetricsStore`, `TracingStore` and `Tenant` read fine.
   - I found no licence check on writes to those six in `crates/jmap/src/registry/`. "Writes presumably refused" is a guess; they may be accepted and simply have no effect.

4. **A permission in `/api/account` does not mean the feature exists.**
   - The unlicensed local admin holds `liveMetrics`, `sysMetric*`, `sysTrace*`, `sysTenant*` and `sysAlert*`, while the endpoints return 404 or `forbidden`.
   - The tester holds `sysMaskedEmail*` and is still refused.
   - Gating needs edition and permission, plus the schema flag or a probe for masked email.

5. **How an administrator signs in from the app is the largest untested design point.**
   - Documented and in source, never exercised: only a superuser-created `OAuthClient` can receive `full`.
   - The third report's "probably `full`, or `settings`" is a guess; the docs say only `full`.
   - The API-key alternative: `crates/common/src/auth/access_token.rs:125-160` gives a key the account's permissions (`Inherit`) or a subset. I saw no OAuth scope applied, but did not trace `authenticate.rs` to the end, so treat it as likely.
   - No report tested a scoped token against any `x:` method.
   - This needs no licence and can be settled on the local test server.

6. **"A capability outside the token's scopes is dropped, methods fail with `unknownMethod`" is doubtful for `x:` methods.**
   - `crates/jmap/src/api/request.rs:117-119` exempts `Capability::Stalwart` from the `using` check, which matches the local probe (worked with core only).
   - What a `mail`-only token gets for `x:MaskedEmail/get` (`forbidden` or `unknownMethod`) is untested.

7. **Dashboard: all shapes come from the WebUI's TypeScript, which is under the Enterprise licence.**
   - No report saw an `x:Metric` answer or a live frame.
   - The `x:Metric` filter names (`timestampIsGreaterThanOrEqual`, `metric`, `nodeId`) are from WebUI code and server source; the reference page lists only `nodeId`.
   - No licence file was found for `resources/schema/schema.json.gz`, which holds the 11 dashboard definitions. Reading them from the server at run time is one thing; shipping them in the app is another. Unverified, not legal advice.
   - Live values come from the node serving the request and `queue.count` is 0 on nodes without the `metricsCalculate` role. This is a docs claim, untested.

8. **Licence status has no API.**
   - `edition` is the only field. Two extra sources exist in code: the `ReloadSettings` warnings, and the error "Enterprise licensed account limit reached: {} accounts licensed." (`registry/mapping/principal.rs:52-60`).
   - The licence text says keys lapse 15 days after the subscription ends (`LICENSES/LicenseRef-SEL.txt` 3.4 and 3.5); `docs/server/enterprise.md:81` says "A license has no grace period". Both quotes are verified. The app must cope with `edition` flipping at any time.

9. **Smaller points to correct before designing.**
   - The session string "Stalwart v1.0.0" is hard-coded (`crates/jmap-proto/src/request/capability.rs:450`). It cannot be used to detect the server version.
   - The third report's "/metrics 404" tested the wrong path. The route is `/metrics/prometheus` (`crates/http/src/request.rs:722`) and the exporter is disabled there, so it shows nothing about the community metrics.
   - The fired-alert list, an AI prompt endpoint and compliance APIs are "not found", which is not proof of absence.
   - Push or state changes for `x:` types were not checked.
   - Passkey registration from a native Android app was not found.
   - Whether `GET /logo` needs authentication is unknown; the handler exists only with a licence (`request.rs:757`).

## B. Contradictions between the reports

| Topic | Report says | Other report says | Source (v1.1.0) |
|---|---|---|---|
| `/api` endpoints | features: only `/api/account`, `/api/schema`, `/api/token/*`, `/api/live/*` | web admin: also `POST /api/auth`, `GET /api/auth/client`, `GET /api/discover/{email}`, `POST /api/calendar/rsvp` | The web admin list is the fuller one. `UPGRADING/v0_16.md` "no longer exist" is too strong; all management objects are JMAP. |
| `/api/discover/` argument | web admin: `{email}` | local: `{domain}` | Not checked. |
| `sysPasskey*` scope | features: "probably `full` (unverified)" | web admin: `security` | `security` (`scope.rs:482-486`). The features report is wrong. |
| `edition` values | features: `enterprise` or `community` | web admin: also `oss` | Three values (`permissions.rs:38-49`). |
| Enterprise-only objects | features: exactly 4 | web admin: 10 flagged in the schema; local: 10, of which 6 read fine | 4 hard-gated; the other 6 are a schema flag only. |
| `x:Trace` event filter | features: JSON name unverified | web admin: `event` | Handler `Property::Event` exists (`telemetry.rs:230`); wire name `event` is taken from the schema, plausible. |
| `accountId` on `x:` calls | features and web admin: WebUI always sends it | local: omitted, worked | Optional at least for the caller's own account. |
| Log sorting and "newest first" | web admin: none stated | local: only by `id`, order unverified | Open. |
| Masked email like Fastmail | features: differs (`enable` not `state`, no `lastMessageAt`) | local: "follows the Fastmail model" | Features report is the precise one. |

## C. Questions only a licensed server can answer

All requests below are reads. They should run as an administrator of the production server, with nothing else touched. The app's ordinary OAuth client cannot reach most of them (they need `full`), so the cleanest way is for the owner to run them personally with curl or the WebUI's network tab. Creating an API key or an OAuth client for the purpose would be a write on production.

| # | Question | Read-only request | Side effects and exposure |
|---|---|---|---|
| 1 | What `edition` and permission list does a licensed server give? | `GET /api/account` | None. |
| 2 | Is the schema the same as the local one? | `GET /api/schema`; compare the hash in the redirect with the local one | None. |
| 3 | Real `x:MaskedEmail` list shape | `x:MaskedEmail/query` `{}` then `x:MaskedEmail/get` for own account | Shows own masked addresses. Create, disable and delete are writes: wait for the release that opens the feature and test locally. |
| 4 | Do the `x:Metric` filter names work, and what do rows look like? | `x:Metric/query` `{"filter":{"timestampIsGreaterThanOrEqual":"<24 h ago>","metric":["queue.message-queued"]},"limit":5}` then `x:Metric/get` | Server statistics only. |
| 5 | Live metrics frame format, header auth | `GET /api/live/metrics?metrics=queue.count,user.count&interval=30` with the `Authorization` header; close after the first frame | None with the header. `GET /api/token/metrics` would mint a 60-second token. |
| 6 | `x:Trace` shape and `event` / `queueId` filters | `x:Trace/query` `{"filter":{"event":"smtp.connection-start"},"limit":1}` then `x:Trace/get` | Returns sender and recipient addresses of real mail. |
| 7 | Live tracing frames | `GET /api/live/tracing?filter=<owner's own address>`, close after a few seconds | Streams real traffic. I would skip it; the docs format is enough for a first design. |
| 8 | Deleted-item recovery: is it on, and the list shape | `x:DataRetention/get` `ids:["singleton"]` (look at `archiveDeletedItemsFor`), then `x:ArchivedItem/query` and `/get` for own account | Shows subjects of own deleted mail. Restore is a write: not on production. |
| 9 | How the `x:Enterprise` singleton shows an installed key | `x:Enterprise/get` `ids:["singleton"]` | The local server shows secrets only as `{"@type":"None"}`. Whether a set key is masked is unverified, so the owner should run this and not paste the answer. |
| 10 | Is licence expiry visible anywhere without an action? | `x:Log/query` `{"filter":{"text":"license"},"limit":5}` then `x:Log/get` | Log lines only. `ReloadSettings` would also show it but is an action: do not run it. |
| 11 | Tenants, alerts, AI models on a licensed server | `x:Tenant/query`, `x:Alert/query`, `x:AiModel/query`, each with `/get` | Probably empty. Tests only that reads work; AI model keys should come back masked (unverified). |
| 12 | Does `GET /logo` work without sign-in, and what does it return with no logo set? | `GET /logo`, anonymous | None. |

## D. What does not need the licence (local test server)

These are larger design risks than most of section C and can be settled without touching production:

- which scope reaches which `x:` object, and the error a scoped token gets;
- an `OAuthClient` with `full`, created by the local admin;
- an API key as the admin credential;
- every `/set` on open objects (queue retry and cancel, accounts, app passwords);
- `calculateTotal`, paging, and whether `x:` types appear in push state changes;
- the delivery-test stream;
- the behaviour of the six "soft" Enterprise objects on write.

Two things remain that neither server answers today: a build with edition `oss` (needs a build without the `enterprise` feature), and masked email after it leaves the Enterprise edition (needs the next release).
