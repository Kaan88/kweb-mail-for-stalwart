# What a licensed Stalwart server answers (observed 2026-11-14)

Read-only requests against the owner's licensed server, with an API key of an administrator
account (inherited permissions). Only shapes are recorded here: no addresses, names or values.
One write was made, with the owner's permission: a test masked address was created, switched off and on, renamed and deleted again (none is left).

- `GET /api/account`: `{"edition": "enterprise", "locale": ..., "permissions": [662 names]}`. The
  administrator holds `sysMaskedEmail*`, `sysArchivedItem*`, `sysMetric*`, `sysTrace*`,
  `liveMetrics`, `liveTracing`, `sysQueuedMessage*`, `sysAccount*`, `sysEnterprise*`,
  `sysTenant*`, `sysAlert*`, `sysAiModel*`, `interactAi`, `impersonate`, `sysLog*`.
- Session with the key: the usual mail capabilities; `urn:stalwart:jmap` is in the account's
  capabilities.
- `GET /logo` without sign-in: 200, `image/svg+xml`.
- `x:MaskedEmail/query {}`: works, `{"queryState", "canCalculateChanges": true, "position", "ids": []}`
  (no addresses yet). Create, switch and delete were not tried: that is a write.
- `x:DataRetention/get singleton`: has `archiveDeletedItemsFor` and `archiveDeletedAccountsFor`
  (both null here, so deleted-item recovery is switched off on this server), `holdTracesFor`,
  `holdMetricsFor`, `holdMtaReportsFor`, `metricsCollectionInterval`, the expunge settings.
- `x:ArchivedItem/query {}`: works, empty (recovery is off).
- `x:Metric/query` with `{"filter": {"timestampIsGreaterThanOrEqual": "<UTC time>", "metric": ["queue.message-queued"]}}`
  works. Rows: `{"@type": "Counter", "metric", "count", "nodeId", "timestamp", "id"}` and
  `{"@type": "Histogram", "metric", "count", "sum", "nodeId", "timestamp", "id"}`.
- `x:Trace/query` works with no filter, with `{"event": "smtp.connection-start"}` and with
  `{"text": ...}`, and `calculateTotal` gives `total`. A trace: `{"id", "timestamp", "from", "to",
  "size", "events"}`; `events` is a map "0", "1", ... of `{"event", "timestamp", "keyValues"}`;
  `keyValues` is a map of `{"key", "value": {"@type": "UnsignedInt" | "String" | "List", "value"}}`.
  Events seen in one delivery: `delivery.attempt-start` (spanId, queueId, queueName, from, to,
  size, total), `delivery.domain-delivery-start` (spanId, domain), `message-ingest.ham` (spanId,
  accountId, documentId, mailboxId, blobId, changeId, messageId, size, elapsed),
  `delivery.dsn-success` (spanId, to, hostname, code, details), `delivery.completed` and
  `delivery.attempt-end` (spanId, elapsed).
- Live metrics: `GET /api/live/metrics?metrics=queue.count,user.count,server.memory&interval=5`
  with the `Authorization` header gives `text/event-stream`; every 5 s one frame
  `event: metrics` / `data: [{"metric": "server.memory", "@type": "Gauge", "count": N}, ...]`.
- `x:AiModel`: two models; `{"@type": "OpenAi", "name", "model", "url", "profile", "reasoning",
  "maxOutputTokens", "temperature", "timeout", "maxConcurrentRequests", "rateLimit",
  "apiKey": {"@type": "Value", "secret": "****"}, ...}`: the key comes back masked.
- `x:Tenant`, `x:Alert`, `x:QueuedMessage`: readable, empty here. `x:ClusterNode`: one node,
  `{"nodeId", "hostname", "lastRenewal", "status", "id"}`.
- `x:Enterprise/get singleton`: `{"licenseKey": {"@type": "None"}, "apiKey": {"@type": "None"},
  "logo", "id"}`. So the licence does not show in this object on this server; the edition in
  `/api/account` is the only sign of it.

- **Masked address, the full round trip** (as the administrator, own account):
  - Create: `x:MaskedEmail/set {"create": {"n": {"description": "...", "forDomain": "https://example.com", "emailPrefix": "kwebtest"}}}`
    gives `{"created": {"n": {"id": "<12 chars>", "email": "<the new address>"}}}`. The address is
    the prefix followed by generated characters (33 characters before the @ for an 8 character prefix).
  - Get: `{"id", "accountId", "email", "enable": true, "description", "forDomain", "url": null,
    "createdBy": null, "createdAt": "<time>", "expiresAt": null}`. The on/off property is `enable`.
  - Switch off and on: `{"update": {id: {"enable": false}}}` gives `{"updated": {id: null}}`; a get
    then shows `enable: false`. Changing `description` the same way works.
  - Delete: `{"destroy": [id]}` gives `{"destroyed": [id]}`; the query is empty again.
  - The limit: `maxMaskedAddresses` on the `x:Email` singleton, 500 on this server.

Still not seen anywhere: an archived item, a fired alert.
