REPORT: Stalwart 1.1.0 admin-object probe on the local server http://127.0.0.1:18080. Done 2026-11-14. Scripts are in /tmp/claude-1000/-mnt-C-Users-kaank-Desktop-kweb-mail-for-stalwart/1ec96980-4871-4ff3-b43b-06ef4006739f/scratchpad/adm/ (lib.py is the helper, battery.py the call battery, p*.py the probes, schema.json a copy of /api/schema). Repository not touched. Secrets are shown as <secret>.

LEFT BEHIND: nothing on the server. The probe user, the group, all API keys, app passwords, the OAuth client and every OAuth grant I made are destroyed and verified. I deleted my probe-*.txt key files too, so none remain. tester is unchanged: grant "c" is the app's own. No keys or app passwords, no changes to its mail, folders, filters, shares or identities. Server settings were not changed, and no reload, restart or mail send was run. One slip, repaired: my first four tester flows made 3 OAuth grants (client name "probe client"). I destroyed exactly those with tester's Basic login. The app's own grant "c" was untouched.

## 0. How the flow was driven
- Registration: POST /auth/register with JSON as in core/src/auth/mod.rs gives 201 with a signed stateless client_id ("swc1.<...>"). Nothing is stored on the server for it, and the client name only shows up in the user's OAuth grant list.
- Login (the web page's endpoint): POST /api/auth (JSON). The body is {"type":"authCode","accountName":...,"accountSecret":...,"query":"<the authorize query string>"}.
- Answers are {"type":"consentRequired","token":..}, {"type":"authenticated","location":"ovh.kweb.mail:/oauth/callback?code=..&state=..&iss=.."}, or {"type":"redirect","location":"...?error=invalid_scope&..."}.
- After consentRequired send {"type":"authConsent","token":..,"query":..,"consent":true} and get authenticated.
- Then POST /auth/token (form: grant_type, code, redirect_uri, client_id, code_verifier). The answer has access_token (about 93 chars), refresh_token, scope and username, plus id_token with openid. expires_in is 3600.
- GET /api/auth/client?<authorize query> returns {name, logo, clientHost, redirectHost, scopes:[labels], verified, consentRequired}. With a bad scope it returns {"error":"invalid_request","redirect":true,"location":"...error=invalid_scope"}.

## 1. Scopes (normal user tester, self-registered client)
App scope set = openid offline_access + mail, calendars, contacts, files. The server grants exactly what is asked and registered. Token scope echoed = requested.

| Token | GET /api/account (perm count) | JMAP session | x: calls |
|---|---|---|---|
| app scopes | 200, 216 perms, edition "community" | no `urn:stalwart:jmap` | every x: call below gives HTTP 200 plus method error `{"type":"forbidden","description":"You are not authorized to perform this action"}`: MaskedEmail/query, AppPassword/query, AccountPassword/get, ApiKey/query, OAuthGrant/query, Passkey/query, PublicKey/query, QueuedMessage/query, Account/query |
| + settings | 228 perms (adds sysMaskedEmail*) | `urn:stalwart:jmap` appears in the account capabilities (not the server ones) | MaskedEmail/query: `forbidden` "This feature is only available in the Enterprise edition. Obtain your trial license at https://license.stalw.art/trial." PublicKey/query: OK, empty (own account). The rest forbidden as above. |
| + security | 236 perms (adds sysAccountPassword*, sysApiKey*, sysAppPassword*, sysOAuthGrant*, sysPasskey*) | same as settings | AppPassword/query OK, AccountPassword/get OK (`{"secret":"****","otpAuth":{},"id":"singleton"}`), ApiKey/query OK, OAuthGrant/query OK (lists the user's grants), Passkey/query OK. MaskedEmail and PublicKey: forbidden. QueuedMessage and Account: forbidden. |
| + full, self-registered | no token | none | The registration answer silently drops full: it echoes the scope without it (201). Authorize then redirects with `error=invalid_scope&error_description=The requested scope is not allowed for this client.`. |

- Admin objects were never reachable with security or settings: QueuedMessage and Account stayed `forbidden`.
- The MaskedEmail Enterprise error (not a plain forbidden) appears only with settings. With app scopes the plain forbidden comes first.
- Scope drops are silent in the session. No unknownMethod appeared, because I called x: methods without them in `using`.

## 2. API key as admin credential (admin@kweb.test, account "b")
- Create: `["x:ApiKey/set",{"create":{"k":{"description":"..","permissions":{"@type":"Inherit"}}}}]` returns `{"created":{"k":{"id":"b","secret":"<secret>"}}}`.
- The secret is 42 chars, prefix "API_", shown once. Later gets show "****".
- Use: `Authorization: Bearer <secret>`.
- Does it carry all of the account's permissions? Yes with `Inherit`. /api/account gives 664 perms, identical to admin's Basic login.
- Calls that worked with the key:
  - session (accounts only "b", same capabilities as Basic)
  - x:QueuedMessage/query (empty)
  - x:Account/query (ids g,f,e,d,c,b) and /get
  - x:Log/query, x:BlockedIp/query, x:Domain/query and /get
  - x:ApiKey/query, x:AppPassword/query, x:AccountPassword/get, x:OAuthGrant/query, x:Passkey/query
  - Mailbox/get and Email/query on the ordinary mail JMAP
  - x:MaskedEmail/query: the Enterprise forbidden
- Restricting at creation, with the permissions field:
  - `{"@type":"Replace","permissions":{"authenticate":true,"sysQueuedMessageQuery":true,"sysQueuedMessageGet":true}}` gives a key whose /api/account lists exactly those 3. QueuedMessage/query works. Account, Log and Domain give `forbidden` "You are not authorized to perform this action".
  - `{"@type":"Disable","permissions":{"sysAccountDestroy":true,...}}` gives 661 perms (the three removed). A write then fails with `{"type":"forbidden","description":"You are not authorized to create, update or destroy objects of this type"}`.
  - The field inside Disable is `permissions`, not `disabledPermissions` (that gives invalidPatch "Invalid property").
  - Over-reach: a probe user's key with Replace holding a permission the user lacks fails with `notCreated {"type":"forbidden","description":"You are not authorized to grant permissions: sysQueuedMessageQuery"}`.
  - Permissions can be updated after creation (a key was switched from Inherit to Replace). Not re-checked with a working key.
- Expiry: `expiresAt` (ISO UTC) is honoured. After it passes, GET /api/account gives 401 `{"type":"about:blank","status":401,"title":"Unauthorized","detail":"You have to authenticate first."}`. Creating with a past date succeeds and the key is simply dead (401). Null means no expiry.
- Other limits: `allowedIps` ({"10.9.9.9/32":true}, stored as "10.9.9.9") gives 403 "You do not have enough permissions to access this resource." from 127.0.0.1. A destroyed key gives 401.
- There is a quota of 5 API keys per account: `notCreated {"type":"overQuota","description":"You have exceeded your quota of 5 API keys."}`.
- The same applies to app passwords. An admin app password ("app_" prefix, 40 chars) with Basic `admin@kweb.test:<app password>` also works: /api/account 664 perms, x:Account/query and x:QueuedMessage/query work.

## 3. Registered client with full
- Create as recovery admin (Basic admin). `clientId` is required: omitting it gives `validationFailed [{"type":"Required","property":"clientId"}]`.
- Working shape: `["x:OAuthClient/set",{"create":{"c":{"clientId":"kweb-probe","description":"..","redirectUris":{"ovh.kweb.mail:/oauth/callback":true},"scopes":{"full":true},"grantTypes":{"authorizationCode":true,"refreshToken":true}}}}]`.
- The recovery admin's accountId is "d333333", and the result is `created {"c":{"id":"b"}}`. No secret means a public client.
- GET /api/auth/client for it shows verified:true and the label "Full access to your account". Consent is still required.
- The scopes list of the OAuthClient is also the allowlist of what the client may ask for. Asking mail+calendars+contacts+files+full from a client registered only with `full` gives `invalid_scope`.
  - Either the app asks for `openid offline_access urn:stalwart:oauth:scope:full` alone, or the admin also registers mail, contacts, calendars and files (not tested together).
- Run as admin@kweb.test asking `openid offline_access full`: token scope "openid offline_access urn:stalwart:oauth:scope:full", username admin@kweb.test, and a refresh token.
  - /api/account gives 664 perms.
  - Session capabilities show the mail, calendars, contacts, filenode, sieve and similar ones, but not `urn:stalwart:jmap` (not listed even with full).
  - ALL point-2 calls work: QueuedMessage, Account (query and get and set), Log, BlockedIp, Domain, ApiKey, AppPassword, AccountPassword, OAuthGrant, Passkey, PublicKey. Mailbox/get on mail JMAP also worked, so full also covers mail.
  - MaskedEmail and ArchivedItem give the Enterprise forbidden.
- A normal user (probe) with the same client and full gets a full token, but only its own 253 perms. QueuedMessage and Account stay `forbidden`. So full does not elevate; the user's own role limits it.
- The same admin with only `openid offline_access` gets 2 perms, and Account/query is `forbidden`.
- What the admin must give the app: the client id only (here "kweb-probe", any string they choose). The app needs no secret or key. They register this client_id and its redirect URI in the server. The app must use that client_id instead of dynamic registration for admin sign-in. The client id appears in the user's OAuthGrant list ("clientName" = the description).

## 4. Writes on open objects (own probe data)
- Create user: `x:Account/set create {"n1":{"name":"probe","domainId":"b","@type":"User","roles":{"@type":"User"},"credentials":{"0":{"@type":"Password","secret":"<secret>"}},"description":".."}}` gives `created {"n1":{"id":"g"}}`.
- Fields that came back: emailAddress probe@kweb.test, locale, quotas {}, permissions {"@type":"Inherit"}, memberGroupIds {}, credentials with credentialId "a" and secret "****".
- Quota and description: patch `{"quotas/maxDiskQuota":5000000,"description":".."}` gives `updated {"g":null}`. The whole map `{"quotas":{"maxDiskQuota":7000000}}` also works.
- Rename: `{"name":"probe2"}` works and emailAddress follows (probe2@kweb.test). Renaming back worked, and a login with the unchanged password worked after both.
- Admin sets a password: patch `credentials/0/secret`. The old password gives 401 and the new one 200. Passwords shorter than 8 chars give `invalidProperties "Password must be at least 8 characters long."`, properties ["secret"].
- The user resets their own password with `x:AccountPassword/set {"update":{"singleton":{"currentSecret":"..","secret":".."}}}` as probe (accountId "g"). A wrong currentSecret gives notUpdated `forbidden "Current secret is incorrect."`. A missing one gives `forbidden "Current secret must be provided to change the password or OTP auth."`.
- Disable: there is no enabled flag. Setting `{"permissions":{"@type":"Merge","disabledPermissions":{"authenticate":true}}}` makes the user's Basic login and session answer 403 "You do not have enough permissions to access this resource."
- Enable: `{"permissions":{"@type":"Inherit"}}` and login works again.
- A credential `expiresAt` in the past does NOT block login: /api/account answers 200 with only 5 perms (emailReceive, authenticateWithAlias, authenticate, sysAccountPasswordUpdate, sysAccountPasswordGet), meaning the password must be changed. `expiresAt: null` restores it.
- Group: create `{"@type":"Group","name":"probegrp","domainId":"b"}` gives id "h". Add the user with `memberGroupIds {"h":true}`, remove with `{}` (patch `memberGroupIds/h: null` also works).
- Destroying a group that still has a member fails: `notDestroyed {"h":{"type":"objectIsLinked","objectId":{"object":"Account","id":"h"},"linkedObjects":[{"object":"Account","id":"g"}]}}`. After removal the destroy works.
- Errors:
  - Duplicate name: `notCreated {"type":"primaryKeyViolation","properties":["email"],"objectId":{"object":"Account","id":"g"}}`.
  - Name "bad@name": `invalidPatch "Invalid email local part"`, properties ["name"].
  - Empty name: `invalidPatch "Invalid value for property."`.
  - Unknown domain: `invalidForeignKey {"object":"Domain","id":"zzz"}`.
  - Missing domainId: `validationFailed [{"type":"Required","property":"domainId"}]`.
  - Surprise: the name "Bad Name" was ACCEPTED and normalised to "badname". The alias "bad alias!" was accepted as "badalias!". So the server does not reject bad-looking local parts except "@" and empty, and the app must validate itself.
  - tester (no permission): x:Account/set (create, update or destroy) gives method error `forbidden "You are not authorized to create, update or destroy objects of this type"`. x:Account/get and /query give `forbidden "You are not authorized to perform this action"`.
- Destroy user: `{"destroy":["g"]}` gives `destroyed ["g"]`.

## 5. Queue
- The queue is empty. x:QueuedMessage/query gives `ids [], total 0` (with calculateTotal). /get {"ids":null} gives empty `list`, and a fake id gives `notFound ["zzz"]`.
- /set destroy of a fake id gives `notDestroyed {"zzz":{"type":"notFound"}}` (no real message touched).
- Filters accepted: text, to, due, queueName, returnPath (all accepted without error). Sort is only by `due`; anything else gives `unsupportedSort "Only sorting by 'due' is supported for queued messages"`. /changes gives `unknownMethod`.
- Entry shape (schema, not seen live): blobId, createdAt, envId, flags (set), nextNotify, nextRetry, priority, receivedFromIp, receivedViaPort, returnPath, size, recipients (map address to {queueName, orcpt, retryCount, retryDue, notifyCount, notifyDue, flags, expires, status}).
  - `expires` is {"@type":"Ttl","expiresAt"} or {"@type":"Attempts","expiresAttempts"}.
  - `status` has @type Scheduled, Completed, TemporaryFailure or PermanentFailure.
  - Mutable fields are envId, nextRetry, priority, recipients.
- What the web UI does, per the schema lists:
  - Retry now = set nextRetry to "2000-01-01T00:00:00Z".
  - Cancel = destroy.
  - Delivery history = x:Trace/OutboundDelivery filtered by queueId (Enterprise).
  - Message view = x:QueuedMessage/MessageContents.
- Pause and resume were NOT run. They are x:Action items PauseMtaQueue and ResumeMtaQueue.

## 6. Paging, totals, sorting, text filter
- x:Account/query:
  - `calculateTotal:true` gives `total`. Without it, no total field.
  - `limit` and `position` work (limit 2 pos 2 gives ids e,d). A position beyond the end gives empty ids, with `position` echoed as 0.
  - Negative position counts from the end. `anchor` works. limit 0 gives empty ids and the total.
  - Default order is id descending, newest first.
  - Sort supports `name` and `id`. `createdAt` gives `unsupportedSort "Property createdAt is not supported for sorting"`.
  - Filters: `text` and `domainId` only. `type` or unknown properties give `{"type":"unsupportedFilter","description":"type"}` (the property name). NOT or OR gives `unsupportedFilter "Only AND is supported in filters"`.
- Text filter on Account is a whole-word, case-insensitive match on name and description words. "tester" matches, "TESTER" matches, "Test" matches the descriptions "Test user" and others.
  - Prefixes ("tes", "prob") and email text ("tester@kweb.test", "kweb.test") match nothing.
  - So a search box must fetch all accounts and filter client-side.
- x:Log/query:
  - Filter `text` only (case-sensitive substring on the raw log line; "authentication" matched nothing, "probe@kweb.test" matched). `level` and `timestamp` filters give unsupportedFilter.
  - Sort `id` only; asc and desc return the same order. `timestamp` sort gives `unsupportedSort "Only sorting by 'id' is supported for logs"`.
  - `total` is capped: it showed 5000 even with filters, so it is not a real total.
  - `position` is ignored, answered with position 0 and the same ids. Page by `anchor` = the last id of the previous page (the anchor itself is excluded); an unknown anchor gives empty ids.
  - Without a limit a query returns up to 5000 ids, and /get with ids null returns 500.
  - Ids are opaque (file number << 48 + offset, base-32 text).
  - Ordering is newest-first inside a log file, and files are ordered by reverse file name. On this server the first page was the older 2026-11-13 boot files and the 2026-11-14 entries came later, so the app should check timestamps instead of assuming newest-first.
  - Item: {timestamp, level (info/warn/...), event e.g. "auth.success", details (a "key = value, ..." string, with accountName and remoteIp), id}.

## 7. Push
- A GET /jmap/eventsource/?types=*&closeafter=no&ping=0 as admin and as probe returned 200 text/event-stream.
- Creating, updating and destroying an account, an API key and an app password produced NO event on either stream, even though the actions were done over about 45 s.
- A control showed the stream works: a Mailbox change by the probe user produced `event: state` with `data: {"@type":"StateChange","changed":{"g":{"Mailbox":"sae"}}}`.
- Asking for x: types explicitly (`types=x:Account`, or mixed with Mailbox) gives HTTP 400 `{"type":"about:blank","status":400,"title":"Invalid parameters","detail":"One or multiple parameters could not be parsed."}`.
- Result: x: objects do not produce state changes. The admin screens must poll, or refresh on demand.

## 8. The six soft Enterprise objects (admin, read-only; tester gets `forbidden` "You are not authorized..." on query for AiModel, Alert and Tenant)
- x:AiModel, x:Alert, x:Tenant:
  - /get {"ids":null} gives an empty list.
  - /get ["singleton"] gives `notFound ["singleton"]`.
  - /query gives `ids []` (queryState "n").
  - No licence error on read.
- x:SpamAi: /get ["singleton"] gives `{"@type":"Disable","id":"singleton"}`. /get null gives an empty list. /query gives `unknownMethod` "x:SpamAi/query" (singleton, no query).
- x:MetricsStore and x:TracingStore: /get ["singleton"] gives `{"@type":"Default","id":"singleton"}`. /query gives `unknownMethod`.
- No set was made.

## Extra findings for the design
- Admin ids: the recovery admin "admin" is not a real account: accountId "d333333", and per-account objects fail for it. A signed-in admin@kweb.test is account "b" and works for everything above.
- OAuthClient create worked for the recovery admin. Not tried as admin@kweb.test.
- The Enterprise error text is the same for MaskedEmail and ArchivedItem under every credential, including full tokens and API keys. Match on the description substring "only available in the Enterprise edition".
- Not tested: actual messages in the queue (none exist), pause and resume, a full-scope client that also includes mail scopes, the client_id_metadata_document flow, Passkey writes, and x:Account/changes.
