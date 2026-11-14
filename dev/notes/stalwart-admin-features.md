# Stalwart Enterprise and administrator features: what a client can call

## Sources and versions

- **Server source:** `stalwartlabs/stalwart`, tag v1.1.0, commit e7cde9b (2026-11-09). `main` has no later commit. Cited as `stalwart/…`.
- **Docs:** `stalwartlabs/website`, commit 232fa1b (2026-11-13), folder `src/content/docs/docs/`. A file `X.md` there is `https://stalw.art/docs/X`. Cited as `docs/…`.
- **WebUI:** `stalwartlabs/webui`, commit 6b8ad61 (2026-11-13). Cited as `webui/…`.
- **Local copies:** `/tmp/claude-1000/-mnt-C-Users-kaank-Desktop-kweb-mail-for-stalwart/1ec96980-4871-4ff3-b43b-06ef4006739f/scratchpad/src/stalwart`, `/tmp/claude-1000/-mnt-C-Users-kaank-Desktop-kweb-mail-for-stalwart/1ec96980-4871-4ff3-b43b-06ef4006739f/scratchpad/src/webui` and `/tmp/claude-1000/-mnt-C-Users-kaank-Desktop-kweb-mail-for-stalwart/1ec96980-4871-4ff3-b43b-06ef4006739f/scratchpad/ent-research/website`.
- Everything was fetched anonymously from public repositories. Nothing about the project was sent anywhere. Nothing was run against a server; all findings are read from docs and source.

## Findings that shape the design

1. **One API for everything.** Management goes through JMAP at `/jmap/` with capability `urn:stalwart:jmap` and methods `x:<Object>/get|set|query`. docs/management/index.md: "Every management operation reads or writes those objects through one interface, the JMAP management API at `/jmap/`."
   - The only extra HTTP endpoints are `/api/account`, `/api/schema`, `/api/token/{kind}` and `/api/live/{delivery|tracing|metrics}` (docs/http/api.md; `stalwart/api/v1/openapi.yml`).
   - The WebUI sends `x:` calls with the primary account id (`webui/src/services/jmap/client.ts`, `getAccountId`).

2. **Permission names follow a pattern** (docs/auth/authorization/permissions.md).
   - Objects: `sys<Object>Get|Query|Create|Update|Destroy`; singletons have only `Get` and `Update`.
   - Actions and tasks: `action<Variant>` and `task<Variant>`, plus `sysActionCreate` or `sysTaskCreate`.
   - Standalone: `impersonate`, `liveDeliveryTest`, `liveTracing`, `liveMetrics`, `interactAi`, `scimAccess`, `fetchAnyBlob`.

3. **The OAuth scope decides what the app can reach** (docs/auth/oauth/scopes.md).
   - `urn:stalwart:oauth:scope:settings` covers `sysAccountSettings*`, `sysPublicKey*` and `sysMaskedEmail*`.
   - `urn:stalwart:oauth:scope:security` covers `sysAccountPasswordGet/Update` (password and 2FA), `sysAppPassword*`, `sysApiKey*` and `sysOAuthGrantGet/Query/Destroy`.
   - Spam training samples belong to the `mail` scope.
   - Everything else needs `urn:stalwart:oauth:scope:full`: "Neither grouped scope reaches an administration object, even on an administrator's account. Only `urn:stalwart:oauth:scope:full` does". Archived items are also "reachable only with `urn:stalwart:oauth:scope:full`".
   - `full` is allowed "only to clients an administrator has vouched for: an OAuthClient created by a superuser outside any tenant. Dynamically registered clients, clients identified by a metadata document and clients registered by a tenant administrator can request `security` and `settings`, but never `full`."
   - So admin screens need one of two things. Either the server admin creates an `OAuthClient` with `"scopes": {"full": true}` and the app's redirect URI (example in docs/auth/oauth/register-client.md, "Grant full access to a trusted application"). Or the app uses an API key (`Authorization: Bearer API_...`, which has no scopes; docs/auth/authentication/api-key.md) or an app password.
   - A session with only `security` or `settings` lists just `core` and `urn:stalwart:jmap`. A capability outside the token's scopes is dropped, and its methods fail with `unknownMethod`.

4. **Feature detection.** `GET /api/account` returns `{"permissions":[...], "edition":"enterprise"|"community", "locale":...}`. The docs say the WebUI "uses this information to decide which parts of the interface to show" (docs/http/api.md).
   - Enterprise objects on a community server fail with JMAP `forbidden`: "This feature is only available in the Enterprise edition." (`stalwart/crates/jmap/src/registry/mod.rs`, `assert_enterprise_object`). In v1.1.0 that check covers exactly `MaskedEmail`, `ArchivedItem`, `Metric` and `Trace`.
   - Enterprise `/api` endpoints return 404 without a licence.

5. **Acting on another account** means passing that account's `accountId` at request level, and needs `impersonate`. Tenant members never get `impersonate`, tenant administrators included (docs/auth/authorization/tenants.md).

6. **Licence, one line:** the Stalwart Enterprise License (SELv2, `stalwart/LICENSES/LicenseRef-SEL.txt`) binds whoever runs the server; a client that only calls the APIs is not covered by it.
   - The licence defines "Software" as "the Stalwart Server Enterprise Edition software … self-hosted by Licensee", and says nothing about clients or API use. This is my reading, not legal advice.
   - What the app must not do: copy SEL-only code. In the WebUI, 30 files are SEL-only, namely `webui/src/features/dashboard/*` and `webui/src/features/tracing/*`. In the server, the SEL-only files include `crates/common/src/enterprise/*`, `crates/jmap/src/registry/mapping/{masked_email,archived_item,telemetry}.rs` and `crates/http/src/api/telemetry.rs`.
   - It must also not help bypass the key (sections 3.2 and 4.3).
   - The licence text says keys become invalid 15 days after the subscription ends (3.4), while docs/server/enterprise.md says "A license has no grace period".

## Feature table

| Feature | Edition (v1.1.0) | What a client could do | API (exact names) | Permission / scope |
|---|---|---|---|---|
| Masked email | Enterprise in v1.1.0 source; website already lists it for both editions | Create, list, enable/disable, delete own masked addresses; admin lists another account's | `x:MaskedEmail/get`, `/set`, `/query` (filter `accountId`) | `sysMaskedEmailGet/Query/Create/Update/Destroy`; in User role; scope `settings` |
| Deleted-item recovery | Enterprise | List and restore deleted mail, files, events, contacts, Sieve scripts; purge from archive | `x:ArchivedItem/get`, `/set`, `/query` (filter `accountId`); restore with `status: "requestRestore"` | `sysArchivedItemGet/Query/Update/Destroy`; in User role; scope `full` only; other accounts need `impersonate` |
| Deleted-account restore | Enterprise | List pending account deletions and cancel one | `x:Task/query` (`@type=DestroyAccount`), `x:Task/set` destroy | `sysTaskQuery/Get/Destroy` + `taskDestroyAccount` |
| AI models | Enterprise | Admin: list and edit model definitions. No prompt endpoint exists | `x:AiModel/get`, `/set`, `/query`; `x:SpamAi/get`, `/set` | `sysAiModel*`, `sysSpamAi*` |
| Sieve `llm_prompt` | Enterprise | Offer an "ask the model" action in the filter editor | Sieve `llm_prompt(model, prompt, temperature)`, extension `vnd.stalwart.expressions` | `interactAi` (User and System Administrator roles) |
| Tenants | Enterprise | Admin: list, create, edit tenants, quotas, used disk | `x:Tenant/get`, `/set`, `/query` (filter `text`); `memberTenantId` on member objects | `sysTenant*`; tenant admins hold the Tenant Administrator role |
| Branding (logo) | Enterprise | Show the server, tenant or domain logo in the app; admin sets it | `GET /logo`; `logo` on `x:Enterprise`, `x:Tenant`, `x:Domain` | read: none found; write: `sysEnterpriseUpdate`, `sysTenantUpdate`, `sysDomainUpdate` |
| Dashboard (stored metrics) | Enterprise | Charts of history | `x:Metric/get`, `/query`; `x:ClusterNode/get`, `/query`; layouts from `GET /api/schema` | `sysMetricGet/Query`, `sysClusterNodeGet/Query` |
| Live metrics | Enterprise | Live tiles | `GET /api/live/metrics?metrics=…&interval=…` (SSE); `GET /api/token/metrics` | `liveMetrics` |
| Live tracing | Enterprise | Follow sessions live | `GET /api/live/tracing?filter=…&<key>=…` (SSE); `GET /api/token/tracing` | `liveTracing` |
| Delivery history | Enterprise | Search what happened to a message | `x:Trace/get`, `/query` (filters `text`, `timestamp`, `queueId`) | `sysTraceGet/Query` |
| Alerts | Enterprise | Admin: list, create, edit, enable metric alerts | `x:Alert/get`, `/set`, `/query` | `sysAlert*` |
| Mail queue | Open | List, inspect, retry, cancel recipient or message, pause/resume | `x:QueuedMessage/get`, `/set`, `/query`; `x:Action/set` with `PauseMtaQueue` / `ResumeMtaQueue` | `sysQueuedMessage*`; `sysActionCreate` + `actionPauseMtaQueue` / `actionResumeMtaQueue` |
| DMARC / TLS / ARF reports | Open | Browse and delete received reports; view or discard pending outgoing ones | `x:DmarcExternalReport`, `x:TlsExternalReport`, `x:ArfExternalReport`, `x:DmarcInternalReport`, `x:TlsInternalReport` (each `/get`, `/set`, `/query`) | `sys<Object>Get/Query/Destroy` (internal ones also `Update`) |
| Accounts and groups | Open (tenant fields Enterprise) | Create, edit, suspend, reset password, quotas, aliases, memberships | `x:Account/get`, `/set`, `/query`, `@type` `User` or `Group` | `sysAccount*`; caller must hold every permission of the target |
| Mailing lists | Open | List, create, edit recipients | `x:MailingList/get`, `/set`, `/query` | `sysMailingList*` |
| Domains | Open (`logo`, `memberTenantId`, `directoryId`, `allowScimProvisioning` Enterprise) | List, add, enable/disable, see the DNS zone file | `x:Domain/get`, `/set`, `/query` | `sysDomain*` |
| DKIM | Open | List keys and rotation stage, create, rotate | `x:DkimSignature/get`, `/set`, `/query`; `x:Task/set` `DkimManagement` | `sysDkimSignature*`; `taskDkimManagement` |
| Roles | Open (tenant-owned roles Enterprise) | List and edit roles | `x:Role/get`, `/set`, `/query` | `sysRole*` |
| API keys, app passwords | Open | Own: create (secret shown once), list, delete. Admin: for another account | `x:ApiKey/*`, `x:AppPassword/*` | `sysApiKey*`, `sysAppPassword*`; scope `security`; other accounts need `impersonate` |
| Password and 2FA (TOTP) | Open | Change password, enable or remove TOTP; admin clears a user's TOTP | `x:AccountPassword/get`, `/set`; admin: `x:Account/set` patch `credentials/0/otpUrl` | `sysAccountPasswordGet/Update`; scope `security`; admin `sysAccountUpdate` |
| Passkeys | Open | List, rename, delete own; admin lists and deletes others' | `x:Passkey/registrationOptions`, `x:Passkey/get`, `/set`, `/query` | `sysPasskey*`; not named in either scope list, so probably `full` (unverified) |
| Connected applications | Open | List and revoke OAuth grants | `x:OAuthGrant/get`, `/set` (destroy), `/query` | `sysOAuthGrantGet/Query/Destroy`; scope `security` |
| At-rest encryption | Open | Upload an OpenPGP key or S/MIME certificate, turn encryption on or off | `x:PublicKey/get`, `/set`, `/query`; `x:AccountSettings/get`, `/set` | `sysPublicKey*`, `sysAccountSettingsGet/Update`; scope `settings` |
| Spam training | Open | Junk / not-junk; list and delete samples; admin uploads samples and starts training | `Email/set` keywords or Junk mailbox; `x:SpamTrainingSample/get`, `/set`, `/query`; `x:Task/set` `SpamFilterMaintenance` | sample permissions in scope `mail`; `sysTaskCreate` + `taskSpamFilterMaintenance` |
| Spam settings | Open (AI classifier Enterprise) | Edit thresholds, rules, DNSBLs | `x:SpamSettings`, `x:SpamClassifier`, `x:SpamRule`, `x:SpamTag`, `x:SpamDnsblServer`, `x:SpamDnsblSettings`, `x:SpamFileExtension`, `x:SpamPyzor` | `sysSpam*` |
| Troubleshooting | Open | Delivery test, DMARC check, spam verdict for a message | `GET /api/live/delivery/{target}` (SSE); `x:Action/set` `TroubleshootDmarc`, `ClassifySpam` | `liveDeliveryTest`; `sysActionCreate` + `actionTroubleshootDmarc` / `actionClassifySpam` |
| Server logs | Open | Search the log files of the node | `x:Log/get`, `/query` (filter `text`) | `sysLogGet/Query` |
| Blocked / allowed IPs | Open | List bans, unblock, block | `x:BlockedIp/*`, `x:AllowedIp/*`; `x:Action/set` `ReloadBlockedIps` | `sysBlockedIp*`, `sysAllowedIp*` |
| Tasks | Open | List failed or scheduled tasks, retry, delete, start maintenance | `x:Task/get`, `/set`, `/query`; `x:IndexQueueStatus/*` | `sysTask*` + `task<Variant>` |
| Reload and caches | Open | Reload settings or certificates, clear caches | `x:Action/set` `ReloadSettings`, `ReloadTlsCertificates`, `ReloadLookupStores`, `InvalidateCaches`, `InvalidateNegativeCaches`, `UpdateApps` | `sysActionCreate` + `action<Variant>` |
| Licence status | n/a | Show the edition; admin installs a key | `GET /api/account` (`edition`); `x:Enterprise/get`, `/set` | any signed-in account; `sysEnterpriseGet/Update` |
| SCIM | Enterprise | Nothing for a mail client | `/scim/v2` | `scimAccess` |
| Pure operations (named only) | Enterprise | None | Read replicas (PostgreSQL, MySQL), sharded blob / in-memory / lookup stores, per-domain directories, LDAP failover, TracingStore and MetricsStore, custom calendar templates | `sys<Store>*` |

Add-ons on the compare page with no API found in docs or source: compliance (retention, legal hold, eDiscovery, journaling) and lawful interception.

## Details

### Masked email

- **Edition.** The owner's statement is supported by the website but not yet by the code.
  - v1.1.0 source still gates it: `masked.rs` and `masked_email.rs` carry "SPDX-License-Identifier: LicenseRef-SEL"; the JMAP layer refuses it without a licence; and the SMTP lookup in `stalwart/crates/common/src/network/mta.rs` is under `#[cfg(feature = "enterprise")] if self.is_enterprise_edition()`.
  - The website (2026-11-13) has `["Masked email addresses (privacy aliases)", "yes", "yes"]` in `src/content/pages/compare.yml` (columns Community, Enterprise). The docs page has no Enterprise note, and docs/server/enterprise.md does not list it.
  - No announcement, issue or commit about the move was found. Until that release, the app should treat `forbidden` on these methods as "not available on this server".

- **Not the Fastmail extension.** The capability is `urn:stalwart:jmap` and the methods are `x:MaskedEmail/*`; `https://www.fastmail.com/dev/maskedemail` does not appear in the source.
  - Same as Fastmail: `email`, `description`, `forDomain`, `url`, `createdBy`, `createdAt`, `emailPrefix`.
  - Different: Stalwart has `enable` (boolean) where Fastmail has `state`; no `lastMessageAt`; adds `emailDomain`, `expiresAt`, `accountId`; adds `/query`.

- **Properties** (docs/ref/object/masked-email.md):

  | Property | Type | Notes |
  |---|---|---|
  | `enable` | Boolean | default true |
  | `accountId` | Id | read-only |
  | `email` | String | server-set |
  | `description` | String? | |
  | `forDomain` | String? | e.g. `https://example.com` |
  | `createdAt` | UTCDateTime | server-set |
  | `createdBy` | String? | set by the server from the credentials |
  | `expiresAt` | UTCDateTime? | create only |
  | `url` | String? | |
  | `emailPrefix` | create only | 1 to 64 characters, a-z 0-9 `_`, not starting with `_` |
  | `emailDomain` | create only | a domain where the account has an address |

- **Create:**
  ```json
  ["x:MaskedEmail/set", {"create": {"new1": {"description": "Newsletter signup", "forDomain": "https://example.com", "emailPrefix": "newsletter"}}}, "c1"]
  ```
  with `"using": ["urn:ietf:params:jmap:core", "urn:stalwart:jmap"]`. The response returns `email`, of the form `<prefix>.<code>@<domain>`. The prefix is 16 random characters if not given.

- **Disable, enable, delete.** Update `{"enable": false}` or `true`; `destroy` removes it for good. On update, only `enable`, `description`, `forDomain` and `url` can change; the code answers "Cannot modify read-only property" for the rest. The expiry is part of the address and cannot change.

- **List.** `x:MaskedEmail/query`, optionally with `filter: {"accountId": …}`, then `/get`.

- **Errors.**
  - `overQuota`: "You have exceeded your quota of <n> masked addresses."
  - `forbidden` on `emailDomain`: "The specified domain is not valid for this account." or "No valid domain is available for this account."
  - `invalidProperties` on `emailPrefix`.

- **Limit.** `maxMaskedAddresses` on the `x:Email` singleton, default 5; an account's `quotas` can override it. "Disabled masked addresses count towards the limit until they are destroyed."

- **Per user.** "The built-in User role holds all five, so every user can manage the masked addresses of their own account". Since 1.0.1, listing "returns only the requested account's objects, also for administrators. Administrators reach other accounts through the account id or an explicit `accountId` filter" (`stalwart/CHANGELOG.md`).

- **Behaviour.** Receive-only: "They cannot be used as a sending identity". Mail to a disabled or expired address is rejected as an unknown recipient. The address stays in `To`. A keyword `$maskedemail` exists in `stalwart/crates/types/src/keyword.rs`; when it is set was not checked.

- **Scope.** `settings` is enough, so the app's ordinary OAuth client can do this.

### Deleted-item recovery (Enterprise)

Source: docs/management/maintenance/deleted-items.md and docs/ref/object/archived-item.md.

- Off by default. It is on only when `archiveDeletedItemsFor` on the `x:DataRetention` singleton is set (milliseconds).
- `ArchivedItem` variants: `Email` (`from`, `subject`, `receivedAt`, `size`), `FileNode` (`name`), `CalendarEvent` (`title`, `startTime`), `ContactCard` (`name`), `SieveScript` (`name`, `contents`). All carry `accountId`, `archivedAt`, `archivedUntil`, `blobId`, `status`.
- Restore: `["x:ArchivedItem/set", {"update": {"e": {"status": "requestRestore"}}}, "0"]`. The server creates a `RestoreArchivedItem` task.
- Destroying an item "removes the item from the archive permanently".
- "The default User role holds them for the user's own items, which is what lets users recover their own mail."
- The `accountId` filter "is honoured only for administrators with the `impersonate` permission".
- Reachable only with scope `full`, so not with the app's ordinary OAuth client.
- Deleted account: deleting the pending `DestroyAccount` task restores the account under its original id, name and domain. "credentials, group memberships and the remaining settings take their default values". Needs `taskDestroyAccount`. The delay is `archiveDeletedAccountsFor`.

### AI models and `llm_prompt` (Enterprise)

Source: docs/server/ai-models.md, docs/sieve/llm.md, docs/ref/object/ai-model.md.

- `x:AiModel` variants: `OpenAi` (profiles `OpenAi`, `Azure`, `Vllm`, `LlamaCpp`, `Generic`), `Anthropic`, `Gemini`, `Ollama`, `TypeSafe` (spam classification only).
- Fields: `name`, `model`, `url`, `apiKey`, `httpHeaders`, `allowInvalidCerts`, `timeout`, `maxConcurrentRequests`, `rateLimit`, `maxOutputTokens`, `temperature`, `reasoning`, `extraParameters`.
- Used only by the AI spam classifier and by `llm_prompt`. No client prompt endpoint was found; the HTTP API reference and `openapi.yml` list none.
- `llm_prompt(model, prompt, temperature)` works in user scripts "only when the account holds the `interactAi` permission. Without it, each call returns `false`". All failures return `false`. With exactly one model configured, an empty model name selects it.
- On expiry, "`llm_prompt` returns `false`".

### Multi-tenancy and branding (Enterprise)

Source: docs/auth/authorization/tenants.md, docs/management/webui/branding.md.

- `x:Tenant` fields: `name`, `createdAt`, `logo`, `roles`, `permissions`, `quotas`, `usedDiskQuota`.
- Quota keys: `maxDiskQuota`, `maxAccounts`, `maxGroups`, `maxDomains`, `maxMailingLists`, `maxRoles`, `maxOauthClients`, `maxDkimKeys`, `maxAcmeProviders`, `maxDnsServers`, `maxDirectories`.
- A tenant administrator can manage "the tenant's domains, accounts, groups, mailing lists, roles, OAuth clients, DKIM signatures, ACME providers, DNS servers and queued messages, and nothing outside the tenant".
  - The role also holds `liveDeliveryTest` and `interactAi`.
  - It has no telemetry, logs, alerts, tenants or settings.
  - It cannot use `impersonate`, and its OAuth clients cannot get `full`.
- Over-quota texts: `NO [OVERQUOTA] Organization disk quota exceeded.`; JMAP `overQuota` "You have exceeded your quota of 50 accounts."
- Logo order: Domain `logo`, then Tenant `logo`, then Enterprise singleton `logo`, then built-in. `GET /logo` serves it "while an Enterprise license is active", chosen by the request's hostname.
- Logo value: https URL, base64 or `data:` URI; PNG, JPEG, GIF, WebP, ICO or SVG; at most 1 MiB.

### Dashboard, metrics, live telemetry (Enterprise)

Source: docs/telemetry/management.md, docs/telemetry/history.md, docs/telemetry/live.md.

- Stored metrics: `x:Metric` with `@type` `Counter`, `Gauge` or `Histogram`; fields `metric`, `timestamp`, `nodeId`, `count`, and `sum` for histograms. Read-only.
- Values are per interval, not running totals. Collection is hourly by default (`metricsCollectionInterval`), kept 90 days (`holdMetricsFor`).
- The WebUI dashboard queries `x:Metric` with filter keys `timestampIsGreaterThanOrEqual`, `timestampIsLessThanOrEqual`, `metric` (a list) and `nodeId` (`webui/src/features/dashboard/queries.ts`).
  - The reference page lists only `nodeId` as a filter.
  - The server source handles `Timestamp`, `Metric` and `NodeId` (`stalwart/crates/jmap/src/registry/mapping/telemetry.rs`).
- Dashboard layouts come from the schema document: `dashboards: Dashboard[]` in `webui/src/types/schema.ts`, served by `GET /api/schema`. The exact JSON was not inspected.
- Live metrics: `GET /api/live/metrics?metrics=<name>,…&interval=<seconds>`. Minimum interval 1 s, default 30 s. Event `metrics` with data such as `[{"metric":"queue.count","@type":"Gauge","count":12}, …]`. Values are those of the node that serves the request.
- Live tracing: `GET /api/live/tracing`, filters `filter=<text>` and `<key>=<text>` (case-sensitive key names). Starts with event `ping` (`{"interval": 30000}`), then `trace` batches at most once per second.
- Auth for both streams: an `Authorization` header, or `?token=` from `GET /api/token/{tracing|metrics}` (valid 60 s). 404 without a licence.
- The community edition keeps only "the basic Prometheus metrics" at `GET /metrics/prometheus` and the OTel exporters. No JMAP metrics there.

### Delivery history, "where is my mail" (Enterprise)

- `x:Trace` fields: `events` (list of TraceEvent with `event`, `timestamp`, `keyValues`), `timestamp`, `from`, `to`, `size`. Read-only.
- One trace per SMTP session that queued a message and per outbound delivery attempt. Up to 2048 events. Kept 30 days (`holdTracesFor`).
- Query filters: `text`, `timestamp`, `queueId` (docs/ref/object/trace.md). The source also handles an `Event` filter (`telemetry.rs`); its JSON name is unverified.
- Search finds nothing if `indexTelemetry` on `x:Search` is off.
- WebUI places: Management › Emails › History › Inbound Delivery / Outbound Delivery.
- Not recorded: IMAP, POP3, JMAP and HTTP sessions.
- The community substitute is the queue plus logs.

### Alerts (Enterprise)

Source: docs/telemetry/alerts.md.

- `x:Alert` fields: `condition` (expression, e.g. `metric('queue.count') > 5000`), `emailAlert` (`fromAddress`, `fromName`, `to`, `subject`, `body`), `eventAlert` (`eventMessage`), `enable`.
- Evaluated every five minutes, with no memory: notifications repeat while the condition holds.
- No push to a client. An alert reaches people as an email (header `Auto-Submitted: auto-generated`) or as the `telemetry.alert-event` event through a webhook. The app could surface alert mails; no alert-history object was found.

### Mail queue (both editions)

Source: docs/mta/management.md, docs/ref/object/queued-message.md.

- `x:QueuedMessage` fields: `createdAt`, `nextRetry`, `nextNotify`, `blobId`, `returnPath`, `recipients` (map of address to QueuedRecipient), `receivedFromIp`, `receivedViaPort`, `flags`, `envId`, `priority`, `size`.
- Query filters: `text`, `to`, `due`, `queueName`, `returnPath`.
- Retry: set `nextRetry`, or `recipients/<address>/retryDue` for one recipient.
- A permanently failed recipient needs two updates: first `recipients/<address>/status = {"@type":"Scheduled"}`, then reschedule.
- Cancel one recipient: patch `recipients/<address>` to `null`. Cancel the whole message: destroy it.
- Pause and resume: `x:Action/set` create `{"@type":"PauseMtaQueue"}` / `ResumeMtaQueue`. The paused state is in memory per node.

### DMARC, TLS and ARF reports (both editions)

- Received: `x:DmarcExternalReport`, `x:TlsExternalReport`, `x:ArfExternalReport`. View and delete only. Kept 30 days by default (`holdMtaReportsFor`).
- Pending outgoing: `x:DmarcInternalReport`, `x:TlsInternalReport`. "Deleting a pending report discards it".
- The compare page lists "Report visualization (DMARC, TLS-RPT, ARF)" for both editions.
- The report field lists were not extracted.

### Accounts, groups, lists, domains, DKIM (both editions)

- `x:Account`, `@type: "User"` fields: `name`, `domainId`, `emailAddress`, `credentials`, `createdAt`, `memberGroupIds`, `memberTenantId` (Enterprise), `roles`, `permissions`, `quotas`, `usedDiskQuota`, `aliases`, `externalId` (Enterprise), `description`, `locale`, `timeZone`, `encryptionAtRest`.
- `Group` is similar. Query filters: `text`, `name`, `domainId`, `memberTenantId`, `memberGroupIds`.
- "Creating, changing or deleting an account requires holding every permission of that account, and superusers have no exception". The error is `forbidden`: "You are not authorized to grant permissions: …".
- Suspend an account by disabling its `authenticate` permission.
- `x:MailingList`: `name`, `domainId`, `emailAddress`, `description`, `aliases`, `recipients`.
- `x:Domain`: `name`, `aliases`, `enable`, `description`, `certificateManagement`, `dkimManagement`, `dnsManagement`, `dnsZoneFile` (server-set), `catchAllAddress`, `subAddressing`, `allowRelaying`, `reportAddressUri`.
- `x:DkimSignature` variants: `Dkim1Ed25519Sha256`, `Dkim1RsaSha256`, `Dkim2Ed25519Sha256`, `Dkim2RsaSha256`, `Dkim2MlDsa44`. Fields include `selector`, `publicKey` (server-set), `stage`, `nextTransitionAt`, `domainId`. Filter `domainId`.

### Credentials, 2FA, passkeys (both editions)

- `x:ApiKey` and `x:AppPassword` fields: `description`, `secret` (server-set, shown once), `createdAt`, `expiresAt`, `permissions` (`Inherit`, `Disable` or `Replace`), `allowedIps`.
  - With a `security`-scoped token, the default `Inherit` "is refused whenever the account has a permission outside the token's scopes"; use `Replace` with a list.
- `x:AccountPassword` singleton: `secret`, `currentSecret`, `otpAuth` (`otpUrl`, `otpCode`).
  - Changing or removing 2FA "requires a current code".
  - Not available for accounts of external directories: `forbidden` "Operation not allowed."
  - A password change signs out every OAuth application of the account.
- Admin recovery of 2FA: `x:Account/set` update `{"credentials/0/otpUrl": null}`.
- Accepted TOTP URLs: `otpauth://totp/` only; secret at least 128 bits; SHA1, SHA256 or SHA512; 6 to 8 digits.
- Passkeys: `x:Passkey` fields `registration`, `description` (at most 64 characters), `createdAt`.
  - Registration is `x:Passkey/registrationOptions` with `{accountId, currentSecret, otpCode?}`, which returns WebAuthn creation options (`publicKey`). Then `x:Passkey/set` create with `registration: {clientDataJson, attestationObject}` (`webui/src/components/forms/widgets/PasskeyRegistrationField.tsx`).
  - "Registration is accepted on the public URL and on any HTTPS origin whose host is the relying party ID or one of its subdomains."
  - Whether an Android app can register one was not found; it would need the server's host to vouch for the app. Listing, renaming and deleting are plain JMAP.
  - "An administrator cannot register a passkey for another account". Admins can list and delete with `impersonate`.
  - Limit: `maxPasskeys`, default 10.
- Connected applications: `x:OAuthGrant` fields `clientId`, `clientName`, `clientKind`, `scopes`, `remoteIp`, `createdAt`, `lastUsedAt`. Destroy to revoke.

### At-rest encryption (both editions)

Source: docs/encryption/at-rest.md.

- `x:PublicKey`: `key` (OpenPGP public key or S/MIME certificates as text; the server detects which) and `description`.
  - Server-set: `kind`, `keyType`, `fingerprints`, `emailAddresses`, `expiresAt`, `capabilities`, `warnings`.
  - S/MIME switches: `strictKeyWrapEncoding`, `authenticatedEncryption`.
  - Limit 5 (`maxPublicKeys`).
- Turn on: `x:AccountSettings/set` with `encryptionAtRest: {"@type":"Enabled","publicKey":"<id>", headerProtection, encryptOnAppend, allowSpamTraining}`. Turn off: `{"@type":"Disabled"}`.
- Server-wide switch: `atRestEnable` on `x:Encryption` (default on).
- Caveats for the app:
  - JMAP `$draft` messages are not encrypted.
  - "a search for a word in the body or an attachment of an encrypted message finds nothing".
  - If the key is unusable, mail is stored unencrypted.
  - Deleting a selected key fails with `objectIsLinked`.

### Spam training and settings (both editions; AI classifier Enterprise)

Source: docs/spamfilter/classifier/training.md.

- Samples from JMAP: `Email/set` adding the Junk mailbox or `$junk` gives spam; removing it or adding `$notjunk` gives ham. Moving to Trash creates none.
- `x:SpamTrainingSample` fields: `from`, `subject`, `blobId`, `isSpam`, `accountId`, `expiresAt`, `deleteAfterUse`. Create and destroy only. Admin upload: upload a blob, then create with `blobId`, `isSpam` and optional `accountId`.
- Training: `x:Task/set` create `@type: "SpamFilterMaintenance"` with `maintenanceType` `train`, `retrain`, `abort`, `reset` or `updateRules`.
- Needs 100 spam and 100 ham samples by default.
- `ClassifySpam` action input: raw `message`, `remoteIp`, `ehloDomain`, `mailFrom`, `rcptTo`, `isTls`, `authenticatedAs`. Output: `score`, `tags`, `result` (`spam`, `ham`, `reject`, `discard`). With an Enterprise licence also `aiGate`, `aiFacts`, `aiVerdict`.

### Troubleshooting (both editions)

Source: docs/management/troubleshoot.md, docs/http/api.md, docs/management/tasks-actions/actions.md.

- Delivery test: `GET /api/live/delivery/{target}?timeout=30`, SSE.
  - Each frame is `event: event` with a JSON array of one step, e.g. `{"type":"mxLookupSuccess",…}`. The last is `{"type":"completed"}`.
  - Steps: MX lookup, MTA-STS, TLSA/DANE, connect, EHLO, STARTTLS, SMTP transaction.
  - It always connects to the MX hosts on port 25 and ignores relay routes.
  - Token from `GET /api/token/delivery`.
- `TroubleshootDmarc`: `x:Action/set` create with `remoteIp`, `ehloDomain`, `mailFrom`, `spfEhloDomain`, `spfMailFromDomain`, optional `rcptTo` and raw `message`. Returns `spfEhloResult`, `spfMailFromResult`, `dkimPass`, `dkim2Pass`, `dmarcPass`, `dmarcPolicy`, `elapsed`.
- Action results exist only in the `created` part of the response.

### Logs (both editions)

- `x:Log/query` filter `text`; fields `timestamp`, `level`, `event`, `details`.
- Read from the log files of the node that serves the request.
- Fails with "No log tracers configured on the server" when no file tracer is enabled.

### Licence status

- `GET /api/account` field `edition` is the only status found.
- `x:Enterprise` singleton holds `licenseKey`, `apiKey` (both secrets) and `logo`. No field for expiry date, licensed domain or mailbox count was found.
- Those appear only in the server log ("Stalwart Enterprise Edition license key is valid") and in `warnings` of the `ReloadSettings` action result.
- Installing a key: `x:Enterprise/set`, then `ReloadSettings`, then sign in again.

## Not found or not verified

- Any announcement, issue or commit saying masked email moves to the open edition. The only evidence is the compare page and the missing Enterprise note in the docs.
- Whether a released server already answers `x:MaskedEmail/*` without a licence. By the v1.1.0 source it does not.
- Which scope carries `sysPasskey*`. The scopes page names neither `security` nor `settings` for it.
- Whether a native Android app can register a passkey against Stalwart.
- An API for licence expiry, licensed domain or account count.
- A client endpoint to send a prompt to the server's AI model (none in docs or OpenAPI).
- An alert-history or "alert fired" object.
- APIs for the compliance and lawful-interception add-ons.
- The JSON name of the event-type filter on `x:Trace/query`, and the exact shape of `dashboards` in `/api/schema`.
- Field lists of the report objects, and when the server sets the `$maskedemail` keyword.
- Whether `GET /logo` needs authentication.
- The WebUI README says "SELv1" while the server ships SELv2 text; the WebUI's own licence file was not compared.
- GitHub issue search returned nothing through the anonymous API (possibly rate-limited), so discussions were not searched. WebSearch and WebFetch were not used.
