# Stalwart 1.1.0: domains, reports, tasks, cluster (probed 2026-11-24)

Probed on the local test server (Windows, Stalwart 1.1.0, admin credentials from `~/tools/stalwart-test/etc/admin.txt`)
with JMAP `x:<Object>/get|query|set`. Writes were made only on objects created for the probe (the domain
`example-admin-test.test`, a test account, tasks) and removed again; the server holds only `kweb.test` afterwards.
`GET /api/schema` (gzip) is the server's own description of every object: fields, enums, list columns and filters. The
report and task shapes below that the server had no example of (it had no report) are from that schema.

## Domains (`x:Domain`)

- Get: `name`, `enable`, `aliases` (a set: an object `{ "alias.test": true }`, empty `{}`), `description`, `dkimManagement`
  (`@type` `Automatic` with `algorithms`, `selectorTemplate`, `rotateAfter`, `retireAfter`, `deleteAfter`; or `Manual`),
  `dnsManagement` (`Manual`...), `certificateManagement`, `dnsZoneFile` (server-made, read only), `createdAt`, and the
  Enterprise fields.
- `dnsZoneFile` is one text: records one per line (`name. IN TYPE value`), a long TXT (an RSA DKIM key) over several lines
  between `(` and `)`. For a new domain it holds SPF, MX, DMARC, SRV records for the services, MTA-STS, TLS-RPT, autoconfig
  and, for `Automatic` DKIM, one TXT per DKIM key.
- Query sorts by `name` without an error but returns ids in creation order (the app sorts the names itself); no filter was
  needed. `x:Domain/query` filter `text` and `name` exist in the schema.
- Create `{ "name": "Example-Admin-Test.test" }` gives an id; the name is stored **lower case**. A name with a space is
  `invalidPatch` ("Invalid domain name", properties `["name"]`); a name that exists is `primaryKeyViolation` with the
  existing object's id.
- A new domain gets **two DKIM keys at once** (Ed25519 and RSA, selector like `v1-ed25519-20261124`), because `dkimManagement`
  defaults to `Automatic`. (The old `kweb.test` has `Manual` and none.)
- `enable` can be patched (`{"id": {"enable": false}}`), answer `updated: { id: null }`.
- Destroy of a domain that anything points at is `notDestroyed: { type: "objectIsLinked", objectId, linkedObjects: [
  { object: "Account", id }, { object: "DkimSignature", id }, ... ] }`. **The domain's own DKIM keys count as linked
  objects**, so a domain with keys cannot be deleted until its keys are destroyed (`x:DkimSignature/set destroy`). After
  the account and the keys were destroyed the domain went. Whether an alias counts as a link was not probed (the app checks
  aliases itself).
- `x:Account/query` takes the filter `domainId` (applied: 3 accounts for `kweb.test`, 0 for the new domain).
  `x:MailingList/query` does **not** take `domainId` as a filter (`unsupportedFilter`).
- Task note: creating and deleting domains makes the server run tasks (DKIM and DNS management) that finish at once.

## DKIM keys (`x:DkimSignature`)

- Query filter `domainId` works (others in the schema: `memberTenantId`). `calculateTotal` is allowed.
- Get fields: `@type` (`Dkim1Ed25519Sha256`, `Dkim1RsaSha256`, `Dkim2Ed25519Sha256`, `Dkim2RsaSha256`, `Dkim2MlDsa44`; only
  in the answer when asked for as a property), `selector`, `stage` (`active`, `pending`, `retiring`, `retired`),
  `nextTransitionAt`, `createdAt`, `domainId`, `publicKey` (base64, also in the zone file), `privateKey`
  (`{ "@type": "Text", "secret": "****" }`: masked by the server, and never asked for by the app), `canonicalization`,
  `headers`, `expire`, `report`, `thirdParty`.
- A create without a private key is refused (`Property 'secret' is required`): the app does not make keys; the server does,
  per domain.

## Received reports (`x:DmarcExternalReport`, `x:TlsExternalReport`, `x:ArfExternalReport`)

- The local server had none, and one cannot be made through the API. Shapes are from `GET /api/schema`:
  - all three: `from` (address), `to` (set), `subject`, `receivedAt`, `expiresAt`, `memberTenantId`, `report` (an object).
  - DMARC `report`: `orgName`, `email`, `reportId`, `dateRangeStart/End`, `policyDomain`, `policyDisposition`
    (`none|quarantine|reject|unspecified`), `policyAdkim`, `policyAspf`, `policyTestingMode`, `records` (list). A record:
    `sourceIp`, `count`, `evaluatedDisposition`, `evaluatedDkim`, `evaluatedSpf` (`pass|fail|unspecified`), `headerFrom`,
    `envelopeFrom`, `dkimResults` (`domain`, `selector`, `result`), `spfResults` (`domain`, `scope`, `result`),
    `policyOverrideReasons` (`overrideType`, `comment`).
  - TLS `report`: `organizationName`, `contactInfo`, `reportId`, `dateRangeStart/End`, `policies` (list: `policyDomain`,
    `policyType` `tlsa|sts|noPolicyFound|other`, `mxHosts`, `totalSuccessfulSessions`, `totalFailedSessions`, `failureDetails`:
    `resultType`, `failedSessionCount`, `receivingMxHostname`, `receivingIp`, ...).
  - ARF `report`: `feedbackType`, `reportingMta`, `userAgent`, `incidents`, `sourceIp`, `reportedDomains`, `reportedUris`,
    `deliveryResult`, `authFailure`, `dkimDomain`, `originalMailFrom`, `originalRcptTo`, `authenticationResults`, and
    **`message`, `headers`**, the original mail's headers and content (the app shows none of them).
- Query: **no sort** (`receivedAt` is `unsupportedSort`: "Property receivedAt is not supported for sorting"), so the app
  reads the ids, then `receivedAt`, and orders and pages itself. The DMARC list filters in the schema (`domain`,
  `totalFailedSessions`, ...) were not used.
- Lists and sets may come as JSON arrays or as objects keyed `"0"`, `"1"`: the app reads both.
- The permissions are `sys<Kind>ExternalReportQuery|Get|Destroy` (no Update), so a report can only be deleted.

## Tasks (`x:Task`)

- A query sorts by `due` (the schema's only sort) and filters by `status` (`Pending`, `Retry`, `Failed`, `Stalled`),
  `@type` and `due`; a filter on `domainId` is `unsupportedFilter`.
- Get: `@type` (the variant, e.g. `DkimManagement`, `DmarcReport`, `TlsReport`, `DestroyAccount`, `CalendarItipMessage`,
  `AcmeRenewal`, `StoreMaintenance`...), `due`, `status` (an object with `@type` `Pending` (`createdAt`, `due`), `Retry` /
  `Stalled` (`attemptNumber`, `createdAt`, `due`, `failureReason`, `firstFailedAt`) or `Failed` (`createdAt`, `failedAt`,
  `failedAttemptNumber`, `failureReason`)), and the variant's own fields (`domainId`, `accountId`, `reportId`...).
  A property the variant does not have is simply left out when asked for. A calendar message task holds the whole
  invitation (`iCalendarData`), so the app asks only for `@type`, `due`, `status` and `domainId`.
- `due` is **server set**: a create with `due` is `invalidPatch` ("Cannot modify server set property"); the due time is
  `status.due`, which can be given at create (`status: { "@type": "Pending", "due": "2027-06-01T00:00:00Z" }`).
- A task is run again by replacing `status`: `x:Task/set update { id: { "status": { "@type": "Pending", "due":
  "2000-01-01T00:00:00Z" } } }` (or patching `status/due`). The server picked such a task up within seconds and it
  vanished (it was done). A task being run at that moment: `notUpdated: { type: "forbidden", description: "Task is
  currently being processed and cannot be updated" }`.
- Destroy works like any object; one that is gone is `notFound`.
- Failed and stalled tasks could not be produced on the local server (the server finished every task made), so their
  display is from the schema and the mock only.

## Cluster nodes (`x:ClusterNode`)

- `{ id, nodeId: 0, hostname: "127.0.0.1", status: "active", lastRenewal: "2026-11-24T07:35:13Z" }`; `status` is `active`,
  `stale` or `inactive`. Query and get work with no arguments. Single node here: the node id is 0 (the schema's default is 1).

## Queue pause and resume

- `x:Action/set create { "@type": "PauseMtaQueue" | "ResumeMtaQueue" }` (already used by the app). **The paused state is
  not readable**: `x:MtaVirtualQueue` (local, remote, dsn, report) has only name, description and threads; there is no
  queue status object (`x:QueuedMessage` has no "paused" flag). The app therefore cannot show the state from the server.

## Differences from the earlier notes

- `dev/notes/stalwart-admin-features.md` said `x:Domain` fields were "Open"; confirmed. It listed `DkimSignature` create/rotate
  through `x:Task/set` `DkimManagement`: a task of that kind is accepted (given a `domainId`) and runs at once.
- A domain's own DKIM keys block its deletion (not in the notes).
