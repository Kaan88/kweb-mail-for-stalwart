# Stalwart admin UI: what it shows and which calls it makes

## Sources (all public, fetched read-only; nothing about this project was sent anywhere)

| Repo | Commit | Date | Note |
|---|---|---|---|
| `stalwartlabs/stalwart` | `e7cde9b3` (tag `v1.1.0`, `main`) | 2026-11-09 | server |
| `stalwartlabs/webui` | `6b8ad61b` ("v2.0.6") | 2026-11-13 | current web UI (React, schema-driven) |
| `stalwartlabs/webadmin` | `200e34e2` | 2026-04-22 | old Leptos UI, README says archived, replaced by `webui` at v0.16 |
| `stalwartlabs/cli` | `17b5f950` | 2026-11-01 | `stalwart-cli` |
| `stalwartlabs/website` | `232fa1b1` | 2026-11-13 | docs source (`src/content/docs/docs/...` = stalw.art/docs/...) |

Local clones: `/tmp/claude-1000/-mnt-C-Users-kaank-Desktop-kweb-mail-for-stalwart/1ec96980-4871-4ff3-b43b-06ef4006739f/scratchpad/src/{stalwart,webui,webadmin,cli,website}`. The decompressed UI schema is at `/tmp/claude-1000/-mnt-C-Users-kaank-Desktop-kweb-mail-for-stalwart/1ec96980-4871-4ff3-b43b-06ef4006739f/scratchpad/work/schema.json` (Enterprise build) and `/tmp/claude-1000/-mnt-C-Users-kaank-Desktop-kweb-mail-for-stalwart/1ec96980-4871-4ff3-b43b-06ef4006739f/scratchpad/work/schema_oss.json`.

## 1. How management is exposed in 1.x

- **Everything is JMAP.** `stalwart/UPGRADING/v0_16.md` ("REST API replaced by JMAP"): "The `/api/...` endpoints from previous releases no longer exist. All management operations happen through **JMAP objects** reachable at `/jmap`."
- **Capability:** `urn:stalwart:jmap` (`stalwart/crates/jmap-proto/src/request/capability.rs:99`). Type names carry an `x:` prefix on the wire (`x:Domain`); the CLI drops it (`website .../ref/index.md`).
- **Methods:** standard RFC 8620 `x:<Object>/get`, `/set`, `/query`. Singletons use id `singleton`. Which of get/query/create/update/destroy exists per object is visible from the permission names `sys<Object>{Get,Query,Create,Update,Destroy}`. One extra method was found: `x:Passkey/registrationOptions` (CHANGELOG 1.1.0).
- **Filter operators:** `<field>`, `<field>IsGreaterThan`, `IsGreaterThanOrEqual`, `IsLessThan`, `IsLessThanOrEqual` (`webui/src/lib/listFilter.ts:11-14`), combinable with `AnyOf`/`AllOf`/`Not` (ref docs).
- **Account id:** the web UI sends `accountId` = the primary account for every `x:` object (`webui/src/services/jmap/client.ts:64-70`; primary = `primaryAccounts['urn:ietf:params:jmap:core']`, `webui/src/data/session.ts:28-30`). The docs' curl examples omit `accountId`.
- **`using` sent by the web UI:** core, `urn:stalwart:jmap`, blob, mail, calendars, contacts, principals, sieve, vacationresponse (`client.ts:22-32`).
- **Paging:** `anchor` + `anchorOffset: 1`, continuing while the response carries `limit` (`client.ts` `jmapQueryAll`); gets are batched by `maxObjectsInGet`.

**The few HTTP helper endpoints that remain** (`stalwart/api/v1/openapi.yml`, router in `stalwart/crates/http/src/api/mod.rs`):

| Endpoint | Purpose |
|---|---|
| `POST /api/auth`, `GET /api/auth/client` | login page backend |
| `GET /api/discover/{email}` | anonymous OAuth discovery for a username |
| `GET /api/account` | permissions, edition, locale of the caller |
| `GET /api/schema` → redirect to `/api/schema/{sha256}` | gzipped JSON UI schema, immutable cache |
| `GET /api/token/{delivery,tracing,metrics}` | 60-second stream token |
| `GET /api/live/delivery/{target}`, `/api/live/tracing`, `/api/live/metrics` | SSE streams |
| `POST /api/calendar/rsvp` | calendar RSVP |

**Old line (≤0.15, archived `webadmin`)** used REST paths that are now gone, for example `/api/principal`, `/api/queue/messages`, `/api/queue/reports`, `/api/queue/status/{start,stop}`, `/api/reports/{dmarc,tls,arf}/{id}`, `/api/telemetry/metrics`, `/api/telemetry/traces`, `/api/telemetry/live/{metrics,tracing}-token`, `/api/logs`, `/api/settings/*`, `/api/reload`, `/api/spam-filter/{train,classify}`, `/api/store/{undelete,purge,reindex}`, `/api/troubleshoot/*`, `/api/dkim`, `/api/dns/records`, `/api/account/{auth,crypto}` (grep of `webadmin/src`). Their replacements:

| Old | New |
|---|---|
| `/api/principal` | `x:Account`, `x:Tenant`, `x:Role`, `x:MailingList`, `x:Domain` |
| `/api/queue/messages` | `x:QueuedMessage` |
| `/api/queue/status/*` | `x:Action` `PauseMtaQueue` / `ResumeMtaQueue` |
| `/api/reload` | `x:Action` `ReloadSettings` |
| `/api/telemetry/metrics`, `/traces` | `x:Metric`, `x:Trace` |
| `/api/logs` | `x:Log` |
| `/api/spam-filter/*` | `x:SpamTrainingSample`, `x:Action` `ClassifySpam` |
| `/api/store/undelete` | `x:ArchivedItem` |
| `/api/account/*` | `x:AccountPassword`, `x:AppPassword`, `x:AccountSettings`, `x:PublicKey` |

**The web UI is generated from the schema.** `Schema = { objects, schemas, fields, forms, lists, enums, dashboards, layouts }` (`webui/src/types/schema.ts`). At v1.1.0 the Enterprise schema has 162 objects, 79 lists, 401 forms, 189 enums, 11 dashboards, 3 layouts. A phone client can read the same schema, including list columns, filters, row actions and dashboard definitions.

## 2. Sections and pages (from `layouts` in `resources/schema/schema.json.gz`, v1.1.0)

**Management**
- Dashboard (`CustomComponent/Dashboard`)
- Directory: Accounts `x:Account/User`, Groups `x:Account/Group`, Mailing Lists `x:MailingList`, Tenants `x:Tenant`, Roles `x:Role`, OAuth Clients `x:OAuthClient`
- Domains: Domains `x:Domain`, DKIM Signatures `x:DkimSignature`
- Emails: Queued `x:QueuedMessage`; History → Inbound Delivery `x:Trace/InboundDelivery`, Outbound Delivery `x:Trace/OutboundDelivery`; Delivery tests `CustomComponent/LiveDelivery`
- Reports: Inbox → DMARC `x:DmarcExternalReport`, TLS `x:TlsExternalReport`, ARF `x:ArfExternalReport`; Outbox → DMARC `x:DmarcInternalReport`, TLS `x:TlsInternalReport`
- Observability: Live Tracing `CustomComponent/LiveTracing`, Logs `x:Log`
- Tasks: Scheduled `x:Task`, Failed `x:Task/TaskFailed`, Index Queue `x:IndexQueueStatus`
- Cluster `x:ClusterNode`
- Actions `x:Action`

**Settings** (about 110 pages): Network (listeners, HTTP, JMAP, IMAP, WebDAV, DNS), Storage (stores, data retention, cache), Authentication (general, directories, OIDC provider), TLS (certificates, ACME), MTA (session stages, inbound, outbound, rates and quotas, reports, filters), Cluster, Spam Filter (general, classifier, AI classifier, Pyzor, rules, scores, DNSBL, lists), Email, Calendar & Contacts, Files & Sharing (`x:FileStorage`, `x:Sharing`), Sieve, Security (settings, Blocked IPs `x:BlockedIp`, Allowed IPs `x:AllowedIp`), Encryption, Lookups, Search, Telemetry (Tracers `x:Tracer`, Metrics, Webhooks `x:WebHook`, Alerts `x:Alert`, Event Levels), Task Manager, AI `x:AiModel`, Web Applications `x:Application`, Enterprise `x:Enterprise`.

**Account (self-service, served at `/account`)**
- Settings `x:AccountSettings` (display name, locale, time zone, encryption at rest)
- Credentials: Password `x:AccountPassword` (includes 2FA via `otpAuth`), Passkeys `x:Passkey`, App Passwords `x:AppPassword`, API Keys `x:ApiKey`, Connected Applications `x:OAuthGrant`
- Mailboxes, Calendars, Address Books, Sieve Scripts, Vacation Response (standard JMAP types)
- Masked Addresses `x:MaskedEmail`, Archived Items `x:ArchivedItem`, Spam Samples `x:SpamTrainingSample`, Public Keys `x:PublicKey`

**How the items in the task's list map to 1.x**
- API keys are now a per-account credential (`x:ApiKey`), not a directory page.
- Spam "train" is `x:SpamTrainingSample`; "test" is `x:Action` with `@type: ClassifySpam`.
- Maintenance and troubleshooting are `x:Action` variants plus the Delivery tests page.
- There is no separate "Maintenance" section in the 1.x layout.

**`x:Action` variants** (`x:Action/set` create only, permission `sysActionCreate` plus one permission per variant, e.g. `actionPauseMtaQueue`): `ReloadSettings` (returns `warnings`), `ReloadTlsCertificates`, `ReloadLookupStores`, `ReloadBlockedIps`, `UpdateApps`, `TroubleshootDmarc`, `ClassifySpam`, `InvalidateCaches`, `InvalidateNegativeCaches`, `PauseMtaQueue`, `ResumeMtaQueue`.

Create payload (`website .../ref/object/action.md`): `["x:Action/set", {"create": {"new1": {"@type": "ReloadSettings"}}}, "c1"]`.

## 3. Start-up calls of the web UI

1. `GET /jmap/session`.
2. `GET /api/account` → `{ permissions: string[], edition: string, locale: string }` (`webui/src/services/jmap/client.ts:318-325`; server struct `Account { permissions: Vec<Permission>, edition: &'static str, locale: Locale }` in `stalwart/crates/http/src/auth/permissions.rs:19-24`).
   - `edition` is `"enterprise"` (valid licence), `"community"` (Enterprise build, no licence) or `"oss"` (built without the `enterprise` feature) (`permissions.rs:38-49`).
   - The list holds the token's effective permissions. It is empty when the token has no access scope.
3. `GET /api/schema` → 302 to `/api/schema/{hash}` → gzipped JSON. Any authenticated caller may fetch it. An `oss` build serves `schema_oss.json`, which has no dashboards and lacks the Enterprise objects.

**Visibility rule in the UI** (`webui/src/lib/layout.ts`, `lib/permissions.ts`):
- A link shows when the account has `<permissionPrefix>Get`.
- Objects, fields and components flagged `enterprise: true` are locked or hidden when `edition === 'community'`.
- Custom components: `Dashboard` needs `liveMetrics` and read access to `x:Metric`; `LiveTracing` needs `liveTracing` and is Enterprise; `LiveDelivery` needs `liveDeliveryTest` and is not Enterprise.

## 4. Dashboard

**Requests**

a) History: `x:Metric/query` then `x:Metric/get` (`webui/src/features/dashboard/queries.ts`).
```ts
filter = {
  timestampIsGreaterThanOrEqual: from.toISOString(),
  timestampIsLessThanOrEqual: to.toISOString(),
  metric: string[],          // metric ids
  nodeId?: number,
}
```
- Server-side filters are `timestamp` (all five operators), `metric`, `nodeId` (`stalwart/crates/jmap/src/registry/mapping/telemetry.rs:361-420`).
- Permissions: `sysMetricQuery`, `sysMetricGet`.
- Periods: 24h, 7d, 30d, 90d or custom, with 48 / 56 / 60 / 90 buckets (custom 60), sparkline 20 buckets. Client cache 60 s.
- Stored samples are written on `DataRetention.metricsCollectionInterval` (default hourly at minute 0) and kept for `holdMetricsFor` (default 7776000000 ms = 90 days) (`website .../ref/object/data-retention.md`).

b) Cluster nodes for the node picker: `x:ClusterNode/query` + `/get` with properties `['nodeId','hostname']` (not Enterprise).

c) Live values, a token then an EventSource (`webui/src/features/dashboard/stores/liveMetricsStore.ts`, `services/sse.ts`):
- `GET /api/token/metrics` with the Bearer header returns a plain-text token, valid 60 s and only for that stream.
- `GET /api/live/metrics?token=<t>&interval=30&metrics=<comma-separated ids>` is `text/event-stream` with event name `metrics`.
- A client that can set headers may skip the token and send `Authorization` directly (`website .../telemetry/live.md`; server `management_access_token` in `api/mod.rs:318-369`).
- `interval` is in seconds, minimum 1, default 30. Unknown metric names are ignored; with no `metrics` parameter all are sent. The first update is immediate. Values are those of the node serving the request.
- The UI reconnects up to 5 times with 2 s to 10 s backoff, fetching a new token each time, and closes the stream after the tab is hidden for 60 s.

**Types**

```ts
// webui/src/features/dashboard/types/metrics.ts
export type Metric =
  | { '@type': 'Counter'; metric: MetricId; count: number; nodeId?: number; timestamp?: string }
  | { '@type': 'Gauge'; metric: MetricId; count: number; nodeId?: number; timestamp?: string }
  | { '@type': 'Histogram'; metric: MetricId; count: number; sum: number; nodeId?: number; timestamp?: string };
```
Wire frame (server `stalwart/crates/http/src/api/telemetry.rs`):
```
event: metrics
data: [{"metric":"queue.count","@type":"Gauge","count":12},{"metric":"delivery.attempt-time","@type":"Histogram","count":340,"sum":182000}]
```
Dashboard definitions come from `schema.dashboards` (`webui/src/features/dashboard/types/schema.ts`):
```ts
export type Dashboard = { id: string; label: string; cards?: Card[]; charts?: Chart[]; items?: DashboardItem[] };
export type DashboardItem = Placement & (({type:'stat'}&Card) | ({type:'chart'}&Chart) | ({type:'headline'}&Headline) | ({type:'ranking'}&Ranking));
export type Card = { title: string; icon: string; source: CardSource; metrics: string[]; aggregate?: Aggregate; scope?: MetricScope; format: MetricFormat; description?: string; sparkline?: boolean; delta?: boolean; polarity?: Polarity };
export type Chart = { title: string; kind: ChartKind; series: Series[]; stacked?: boolean; valueFormat?: MetricFormat; description?: string };
export type Series = { label: string; metrics: string[]; aggregate?: Aggregate; scope?: MetricScope };
export type Headline = { title: string; icon?: string; numerator: string[]; denominator: string[]; aggregate?: Aggregate; scope?: MetricScope; description?: string };
export type Ranking = { title: string; icon?: string; series: Series[]; valueFormat?: MetricFormat; description?: string };
export type Aggregate = 'sum' | 'avg' | 'last'; export type MetricScope = 'node' | 'cluster';
export type ChartKind = 'line' | 'area' | 'bar'; export type CardSource = 'live' | 'history';
export type MetricFormat = 'number' | 'bytes' | 'duration' | 'percent';
```
Reduction (`webui/src/features/dashboard/helpers.ts:125-175`): counters are aggregated over samples; a histogram's value is `sum / count`; gauges are aggregated per node, then summed across nodes for scope `node` or the maximum taken for scope `cluster`.

**The 11 dashboards** (ids: `overview`, `network`, `delivery`, `security`, `authentication`, `encryption`, `performance`, `storage`, `search-index`, `tasks`, `cache`). The two a phone would use first:

*Overview*
- Stat "Received Messages": history, sum of `queue.message-queued`
- Stat "Sent Messages": history, sum of `queue.authenticated-message-queued` + `queue.dsn-queued` + `queue.report-queued`
- Stat "Pending Messages": live, `queue.count`
- Stat "Server Memory": history, last, per node, bytes, `server.memory`
- Chart "Messages": stacked area, received vs sent (same metrics as above)
- Headline "Clean mail": `message-ingest.ham` / (`message-ingest.ham` + `message-ingest.spam`)
- Ranking "Rejections": `message-ingest.spam`, `security.ip-blocked`, `auth.failed`, `smtp.mailbox-does-not-exist`, `smtp.relay-not-allowed`, `smtp.rate-limit-exceeded`
- Stats "Total Users" (live `user.count`) and "Total Domains" (live `domain.count`)
- Chart "Memory usage": `server.memory`, average per node

*Delivery*
- Cards: Queued Messages (live `queue.count`), Active Sessions (live `delivery.active-connections`), Session Time (`delivery.attempt-time`, avg), Delivery Time (`delivery.total-time`, avg), Messages Received (`queue.message-queued`), Messages Sent (`queue.authenticated-message-queued`), DSNs Sent (`queue.dsn-queued`), Reports Sent (`queue.report-queued`), Queue Back-pressure (`queue.back-pressure`)
- Charts: Messages Received (ham vs spam), Messages Sent (messages, DSNs, reports), Delivery time, Queue size (`queue.count`)

*Others, main metrics*
- Network: `{smtp,imap,pop3,http,sieve}.active-connections` live; `{smtp,imap,pop3,http}.connection-start`, `manage-sieve.connection-start`, `delivery.attempt-start`
- Security: `security.ip-blocked`, `security.{authentication,abuse,scan,loiter}-ban`, `auth.failed`, `message-ingest.spam`
- Message Authentication: `smtp.{spf-from,dkim,dkim2,dmarc}-{pass,fail}`, `smtp.dkim2-dsn-rejected`, `incoming-report.{dmarc,tls}-report-with-warnings`
- Encryption: `encryption.*`
- Performance: `message-ingest.time`, `dns.lookup-time`, `server.memory`, `server.buffered-memory-{smtp,read}`, `limit.memory-budget`
- Storage: `store.{data,blob}-{read,write}-time`
- Search Index: `search-index.*`
- Tasks: `task-manager.{pending,due}` live; `task-manager.task-{failed,retry,stalled,expired}`
- Cache: `cache.*`

The full `MetricType` enum has 399 names (in `schema.enums.MetricType`; also `website .../ref/metrics.md`). The complete per-card list can be regenerated from the schema with `/tmp/claude-1000/-mnt-C-Users-kaank-Desktop-kweb-mail-for-stalwart/1ec96980-4871-4ff3-b43b-06ef4006739f/scratchpad/work/c.py`.

**Enterprise gating of the dashboard**
- `x:Metric` is refused server-side without a licence: `assert_enterprise_object` in `stalwart/crates/jmap/src/registry/mod.rs:24-43` covers `MaskedEmail | ArchivedItem | Metric | Trace` and returns JMAP `forbidden` with "This feature is only available in the Enterprise edition…".
- `/api/token/metrics`, `/api/token/tracing`, `/api/live/metrics`, `/api/live/tracing` are `#[cfg(feature = "enterprise")]` and require `is_enterprise_edition()`; otherwise 404 with detail "Enterprise feature" (`api/mod.rs:181-246, 289-311`).
- The dashboard code in the web UI is itself under the Enterprise licence (SPDX `LicenseRef-SEL`: `features/dashboard/*`, `features/tracing/*`). The types above are quoted as interface facts, not for copying code.
- Without a licence there are no stored metrics and no live stream. Community servers only have the Prometheus and OpenTelemetry exporters (`x:Metrics`).

## 5. Queue

Object `x:QueuedMessage`; permissions `sysQueuedMessage{Get,Query,Update,Destroy}`; not Enterprise. Tenant scoping of queued messages is Enterprise (CHANGELOG 1.1.0).

- **List:** columns `returnPath` (Sender), `nextRetry`, `createdAt` (Received), `size`. Query filters `text`, `to`, `due` (date), `queueName`, `returnPath`. Sort only by `due` (`mapping/queued_message.rs:421`).
- **Retry now:** `x:QueuedMessage/set` update `{ "<id>": { "nextRetry": "2000-01-01T00:00:00Z" } }` (schema item action "Retry Now"; mass action "Retry All").
- **Cancel:** `x:QueuedMessage/set` `destroy: [ids]`.
- **View message:** view `x:QueuedMessage/MessageContents`, using `blobId`.
- **Delivery history:** `x:Trace/OutboundDelivery` query filtered by `queueId` (Enterprise).
- **Pause / resume the whole queue:** `x:Action` `PauseMtaQueue` / `ResumeMtaQueue`.

Fields (schema; same in `website .../ref/object/queued-message.md`):
```
QueuedMessage { id, blobId (serverSet), createdAt (serverSet), envId?: string, flags: set<MessageFlag> (serverSet),
  nextNotify?: UTCDateTime (serverSet), nextRetry?: UTCDateTime, priority: int, receivedFromIp (serverSet),
  receivedViaPort (serverSet), recipients: map<emailAddress, QueuedRecipient>, returnPath (serverSet), size (serverSet) }
QueuedRecipient { expires: QueueExpiry, flags: set<RecipientFlag> (serverSet), notifyCount, notifyDue, orcpt?,
  queueName (serverSet), retryCount, retryDue, status: RecipientStatus }
QueueExpiry = {@type:"Ttl", expiresAt} | {@type:"Attempts", expiresAttempts}
RecipientStatus = {@type:"Scheduled"} | {@type:"Completed", responseCode?, responseEnhanced?, responseHostname?, responseMessage?}
  | {@type:"TemporaryFailure"|"PermanentFailure", errorType: DeliveryErrorType, errorMessage?, errorCommand?, responseCode?, responseEnhanced?, responseHostname?, responseMessage?}
MessageFlag = authenticated | unauthenticated | unauthenticatedDmarc | dsn | report | autogenerated
RecipientFlag = dsnSent | spamPayload
DeliveryErrorType = dnsError | unexpectedResponse | connectionError | tlsError | daneError | mtaStsError | rateLimited | concurrencyLimited | io | encryptionError
```
The queued reports of the old UI are now the Reports pages (`x:DmarcInternalReport`, `x:TlsInternalReport` for outgoing; `x:*ExternalReport` for received).

## 6. Account management

Object `x:Account`, variants `User` and `Group`; permissions `sysAccount{Get,Query,Create,Update,Destroy}`; not Enterprise.

- **List (Accounts):** static filter `{"@type":"User"}`; columns `emailAddress`, `description` (Full Name), `createdAt`; filters `text`, `name`, `domainId`, `memberGroupIds`, `memberTenantId`. Row actions: View, Spam Samples (`x:SpamTrainingSample` by `accountId`), Public Keys, Delete.

```
UserAccount { name, domainId: Id<Domain>, emailAddress (serverSet), description?, aliases: EmailAlias[],
  credentials: Credential[], createdAt (serverSet), encryptionAtRest: {@type:"Disabled"} | {@type:"Enabled", publicKey, encryptOnAppend, allowSpamTraining, headerProtection},
  locale, timeZone?, memberGroupIds: set<Id>, permissions: {@type:"Inherit"} | {@type:"Merge"|"Replace", enabledPermissions, disabledPermissions},
  quotas: map<StorageQuota, uint>, roles: {@type:"User"} | {@type:"Admin"} | {@type:"Custom", roleIds},
  usedDiskQuota (serverSet), externalId? [Enterprise], memberTenantId? [Enterprise] }
EmailAlias { name, domainId, enable, description? }
Credential = Password { credentialId, secret, otpUrl?, expiresAt?, allowedIps }
  | AppPassword/ApiKey { credentialId, description, secret (serverSet), createdAt, expiresAt?, allowedIps, permissions: Inherit | Disable{permissions} | Replace{permissions} }
  | Passkey { credentialId, description, algorithm, keyId, publicKey, createdAt }
StorageQuota = maxEmails, maxMailboxes, maxEmailSubmissions, maxEmailIdentities, maxParticipantIdentities, maxSieveScripts,
  maxPushSubscriptions, maxCalendars, maxCalendarEvents, maxCalendarEventNotifications, maxAddressBooks, maxContactCards,
  maxFiles, maxFolders, maxMaskedAddresses, maxAppPasswords, maxApiKeys, maxPublicKeys, maxDiskQuota, maxPasskeys
```
- `GroupAccount` has the same shape without `credentials` and `memberGroupIds`; its `roles` is `Default | Custom`.
- Related directory objects: `x:Tenant` (Enterprise: `name`, `logo`, `quotas`, `roles`, `permissions`, `usedDiskQuota`), `x:Role`, `x:MailingList`, `x:Domain`, `x:OAuthClient`.

**Self-service objects**
- `x:AccountPassword` singleton `{ currentSecret?, secret?, otpAuth: { otpCode?, otpUrl? } }`, permissions `sysAccountPassword{Get,Update}`. A password change signs out every OAuth application of the account (`website .../management/webui/account-manager.md`). The server withholds these permissions for accounts of an external directory and for the recovery admin (`permissions.rs:83-95`).
- `x:AppPassword`, `x:ApiKey`: list columns `description`, `expiresAt`.
- `x:OAuthGrant` `{ accountId, clientId, clientName?, clientKind, scopes, createdAt, lastUsedAt, remoteIp?, pending }`; destroying one signs that application out.
- `x:MaskedEmail` `{ accountId (immutable), email (serverSet), description?, forDomain?, url?, createdBy?, enable, emailPrefix? (immutable), emailDomain? (immutable), expiresAt? (immutable), createdAt }`; list filter `accountId`; permissions `sysMaskedEmail*`; per-account quota `maxMaskedAddresses`.
- CHANGELOG 1.0.1: listing connected applications, public keys, masked addresses and archived items returns only the requested account's objects, also for administrators, who reach other accounts through the account id.

## 7. History / tracing

- **Stored traces:** `x:Trace` (Enterprise; `sysTrace{Get,Query}`). Views `x:Trace/InboundDelivery` (static filter `{"event":"smtp.connection-start"}`) and `x:Trace/OutboundDelivery`. List columns `timestamp`, `from`, `to`, `size`. Filters `text`, `timestamp`, `queueId`, `event` (`mapping/telemetry.rs:192-300`). Sort by `id` or `timestamp` only.
  - Shape: `Trace { id, timestamp, from, to, size, events: TraceEvent[] }`.
  - Up to 2048 events per trace. Only SMTP sessions and delivery attempts that involve a queued message are recorded; not IMAP, POP3, JMAP or HTTP sessions.
  - Search needs `Search.indexTelemetry`. Retention is `DataRetention.holdTracesFor` (`website .../telemetry/history.md`).
- **Live tracing** (Enterprise, permission `liveTracing`): `GET /api/token/tracing`, then `GET /api/live/tracing?token=…&filter=<text>&<key>=<text>…`. It starts with `event: ping` / `data: {"interval": 30000}`, then sends `event: trace` batches at most once per second. Key names are case-sensitive; once an event matches, the rest of its session is included (`telemetry.rs`, `website .../telemetry/live.md`).
```ts
// webui/src/features/tracing/types.ts
export interface TraceEvent { event: string; keyValues: TraceKeyValue[]; timestamp: string }
export interface TraceKeyValue { key: string; value: TraceValue }
export type TraceValue =
  | { '@type': 'String'; value: string } | { '@type': 'UnsignedInt'; value: number } | { '@type': 'Integer'; value: number }
  | { '@type': 'Boolean'; value: boolean } | { '@type': 'Float'; value: number } | { '@type': 'UTCDateTime'; value: string }
  | { '@type': 'Duration'; value: number } | { '@type': 'IpAddr'; value: string } | { '@type': 'List'; value: TraceValue[] }
  | { '@type': 'Event'; event: string; value: TraceKeyValue[] } | { '@type': 'Null' };
```
- **Logs:** `x:Log { timestamp, level, event, details }`, filter `text`, permissions `sysLog{Get,Query}`, both editions. Entries are read from the log files of the first enabled Log tracer on the node serving the request; with none the call fails with "No log tracers configured on the server" (`website .../telemetry/management.md`).
- **Delivery test** (both editions, permission `liveDeliveryTest`): `GET /api/token/delivery`, then `GET /api/live/delivery/{target}?token=…&timeout=<s, default 30>`, event name `event`. The stage types are the `DeliveryStage` union in `webui/src/features/troubleshoot/types.ts` (`mxLookupStart` … `completed`). The UI stops after 120 s and does not retry.

## 8. Alerts

- `x:Alert` (Enterprise; `sysAlert{Get,Query,Create,Update,Destroy}`):
  `{ enable: boolean, condition: Expression { match: {if, then}[], else }, emailAlert: Disabled | Enabled { fromName?, fromAddress, to: set<email>, subject, body }, eventAlert: Disabled | Enabled { eventMessage? } }`.
- Conditions use `metric('queue.count') > 5000`. They are evaluated every five minutes per node with no memory between runs, so notifications repeat while the condition holds (`website .../telemetry/alerts.md`).
- Counters never decrease, so the docs advise gauges. `queue.count` is 0 on nodes without the `metricsCalculate` role.
- **No object lists fired alerts** (not found in the schema). An alert surfaces as an email, or as an event (metric names `telemetry.alert-event`, `telemetry.alert-message` exist) that tracers and `x:WebHook` can carry. A phone would learn of one through the alert mail, a webhook receiver, or by polling metrics itself.

## 9. Enterprise-only parts (v1.1.0)

- **Objects flagged `enterprise` in the schema:** `x:AiModel`, `x:Alert`, `x:ArchivedItem`, `x:MaskedEmail`, `x:Metric`, `x:MetricsStore`, `x:SpamAi`, `x:Tenant`, `x:Trace`, `x:TracingStore`.
- **Absent from the OSS schema as well:** `x:Enterprise`, the two `x:Trace/*` views, `x:DataRetention/ArchivingRetention`, `x:DataRetention/TelemetryRetention`, all dashboards.
- **Enterprise fields:** every `memberTenantId`, `UserAccount.externalId`, `GroupAccount.externalId`, `Domain.{logo,directoryId,allowScimProvisioning}`, `Email.maxMaskedAddresses`, `DataRetention.{archiveDeleted*For,holdMetricsFor,holdTracesFor,metricsCollectionInterval}`, `Search.indexTelemetry`, `SpamClassify.ai*`, calendar templates, read replicas, `Authentication.defaultTenantRoleIds`, `LdapDirectory.urlFailover`, `OidcDirectory.trustedProvider`.
- **Hard server gate:** `MaskedEmail | ArchivedItem | Metric | Trace` (section 4), plus the live metrics and tracing endpoints.
- **Documented feature list** (`website .../server/enterprise.md`): multi-tenancy, SCIM, per-domain directories and LDAP failover, AI models (spam, Sieve `llm_prompt`), restoring deleted items and accounts, stored metrics and traces with dashboard, live telemetry and alerts, sharded stores and read replicas, logos and custom calendar templates. Masked email is not on that list.
- **Masked email:**
  - In server v1.1.0 it is still Enterprise: `crates/common/src/enterprise/masked.rs` carries the SEL header, the gate in `registry/mod.rs` includes it, and the scope mapping for `sysMaskedEmail*` is under `#[cfg(feature = "enterprise")]` (`crates/common/src/auth/oauth/scope.rs`).
  - The docs at website `232fa1b1` (2026-11-13) already treat it as general: no Enterprise note on `email/management/masked-email.md` or `ref/object/masked-email.md`, no asterisk in `ref/index.md`, and the account-manager page lists Masked Addresses under the default User role, saying only "The Enterprise edition adds Archived Items".
  - This fits the owner's statement. No changelog entry or commit moving the code was found (the repository has only `main` plus dependabot branches).

## 10. Administrator sign-in and scopes

**Flow** (`webui/src/services/auth/oauth.ts`): `GET /api/discover/{username}` (anonymous) returns `{ authorization_endpoint, token_endpoint, end_session_endpoint?, scopes_supported?, client_id_metadata_document_supported?, stalwart_required_scopes?, protected_resources? }`. Then authorization code with PKCE S256, with a `resource` parameter when HTTPS. OIDC metadata also lists `registration_endpoint` `/auth/register` and `device_authorization_endpoint` `/auth/device` (`stalwart/crates/http/src/auth/oauth/openid.rs:111-135`).

**Scopes** (`stalwart/crates/common/src/auth/oauth/scope.rs`, `website .../auth/oauth/scopes.md`):

| Scope | Allows |
|---|---|
| `urn:ietf:params:oauth:scope:mail` | IMAP, POP3, submission, ManageSieve, JMAP Mail; also `sysSpamTrainingSample{Get,Create,Destroy,Query}` |
| `…:contacts`, `…:calendars`, `…:files` | CardDAV / CalDAV / WebDAV and the matching JMAP types |
| `urn:stalwart:oauth:scope:security` | `sysAccountPassword{Get,Update}`, all `sysAppPassword*`, `sysApiKey*`, `sysPasskey*`, `sysOAuthGrant{Get,Query,Destroy}` |
| `urn:stalwart:oauth:scope:settings` | `sysAccountSettings{Get,Update}`, all `sysPublicKey*`, and (Enterprise build) all `sysMaskedEmail*` |
| `urn:stalwart:oauth:scope:full` | token is unscoped and carries every permission of the account, administration included |
| `openid`, `offline_access` | always allowed; `offline_access` has no effect (a refresh token is always returned) |

- Docs: "Neither grouped scope reaches an administration object, even on an administrator's account. Only `urn:stalwart:oauth:scope:full` does." Management `sys*`, telemetry, SCIM and AI interaction are never reachable with a scoped token. Archived items need `full`.
- **Who may get `full`** (`crates/common/src/auth/oauth/client.rs:343-357, 426-442`; `crates/http/src/auth/oauth/registration.rs:108-130`; `crates/common/src/auth/oauth/registration.rs:516`): only an `x:OAuthClient` created by a superuser outside any tenant, or an installed first-party Application (the web UI).
  - Dynamically registered clients have `full` stripped at registration.
  - Clients identified by a metadata document, and tenant-registered clients, never get it.
  - **Consequence for the app:** an admin dashboard over OAuth requires the server admin to register the app as an `OAuthClient` with `scopes` containing `full`, and the app to request it. With dynamic registration the app can reach at most `security` and `settings`, which covers the self-service pages (password, 2FA, app passwords, API keys, connected apps, account settings, public keys, masked addresses).
- A scoped token cannot create an app password or API key with `permissions: Inherit` when the account holds permissions outside the token's scopes; it must use `Replace` with a list drawn from its own scopes (scopes.md).
- 1.0.3 changelog: third-party apps need these Stalwart scopes; "only a superuser can allow `full`".
- **Other credentials:** the OpenAPI file lists `bearerAuth` and `basicAuth`. `stalwart-cli` authenticates with Basic (`--user`/`--password`) or a Bearer token passed as `--api-key` / `STALWART_TOKEN` (`cli/src/cli/mod.rs:28-46`). So an account API key (`x:ApiKey`, whose permissions can be `Inherit` or a replacement list) is a second way for an admin to connect.

**Permission model:** the enum has 667 permissions (`schema.enums.Permission`; `website .../ref/permissions.md`). Roles on an account are `User`, `Admin` or `Custom{roleIds}`. Permissions relevant here:
- `sys<Object>{Get,Query,Create,Update,Destroy}`
- `liveMetrics`, `liveTracing`, `liveDeliveryTest`
- `action*` (one per Action variant), `task*`
- `impersonate`, `interactAi`, `scimAccess`, `fetchAnyBlob`, `unlimitedRequests`, `unlimitedUploads`

Default roles created before a release do not gain its new permissions; the server must be started once with `STALWART_UPDATE_ROLE_PERMISSIONS=true` (CHANGELOG 1.0.3, 1.1.0).

## 11. Not found or not verified

- **Masked email leaving Enterprise:** no source commit or changelog entry; only the docs evidence in section 9. The shipped v1.1.0 server still gates it.
- **A list of fired alerts:** no such object in the schema.
- **Push or state changes for `x:` objects:** not found. The web UI polls (60 s cache) and uses SSE only for metrics, tracing and delivery tests. No WebSocket use for management was seen. I did not check whether `x:` types appear in JMAP StateChange.
- **Whether `accountId` may be omitted** for `x:` methods: the docs' examples omit it and the web UI always sends it; not tested against a server.
- **Exact JSON of `/api/discover` and the `/api/auth` login body:** read only from the web UI's TypeScript interface, not the server struct. The OpenAPI file has the schemas but I did not copy them.
- **Per-variant fields of `x:Task`, `x:Role`, `x:Domain`, report objects and the Settings pages:** present in the schema file, not transcribed here.
- **The stalw.art/compare page** was not fetched.
- **Nothing was run against a live Enterprise server;** all findings are from source and docs. The owner's trial licence was not used.
- **Whether the API-key Bearer path is subject to the same scope limits as OAuth tokens:** only inferred from the CLI, not verified in `authenticate.rs`.
- **`Capability` session string:** `capability.rs:450` shows `implementation: "Stalwart v1.0.0"`; whether that is a default or the live value was not checked.
