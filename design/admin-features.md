# Admin and Enterprise features: design draft

Status: a proposal for the owner (2026-11-14). Nothing here is built. It rests on Stalwart's public
docs and source at v1.1.0 and on a read-only probe of the local test server; the reports are in
`dev/notes/stalwart-admin-features.md`, `stalwart-webui.md`, `stalwart-admin-api-local.md` and
`stalwart-admin-critic.md`. No call was ever made against a licensed server, so every shape of an
Enterprise answer is from documentation, not observation.

## What decides the design

1. **One API.** All management is JMAP on `/jmap/` with methods named `x:<Object>/get|set|query`.
   Only live streams and a few helpers are plain HTTP (`/api/account`, `/api/schema`,
   `/api/live/...`). The core already speaks JMAP, so this is one new module, not a second client.
2. **The sign-in decides what the app can reach.** The app's sign-in today asks for mail,
   calendars, contacts and files. Two more scopes can be asked for by any app:
   - `settings`: masked email, at-rest encryption keys, account settings;
   - `security`: password, two-factor codes, app passwords, API keys, passkeys, connected apps.

   Everything an administrator does needs the scope `full`, and the server gives `full` only to an
   app that a server administrator registered by hand, never to an app that registers itself as
   this one does. The other way in is an API key made in the web UI. So admin features need a
   one-time setup step on the server, whichever way is chosen.
3. **What a server offers must be asked, not assumed.** `GET /api/account` gives the user's
   permissions and the edition: `enterprise`, `community` (Enterprise build, no licence) or `oss`
   (built without it). A permission alone proves nothing: an unlicensed server lists
   `sysMaskedEmail*` and still refuses the call. A feature is shown only when the permission is
   there and a harmless read of that object succeeds.
4. **Licence.** The Enterprise licence binds whoever runs the server. An app that calls the APIs
   is not covered by it, but must not copy Enterprise code: the web UI's dashboard and tracing
   screens are under that licence, so the app's dashboard is drawn from scratch from the server's
   own data. (My reading, not legal advice.)

## Tier 1: for every user (no admin setup)

| Feature | Needs | Server |
|---|---|---|
| **Masked email** (must) | scope `settings` | Enterprise today; open in the next release |
| Password, two-factor codes | scope `security` | open |
| App passwords, API keys | scope `security` | open |
| Connected apps: list and revoke | scope `security` | open |
| Passkeys: list, rename, delete | scope `security` | open |
| At-rest encryption key upload | scope `settings` | open (belongs with M9) |

**Masked email**: a screen "Masked addresses" in Settings: the list (address, what it is for,
on or off, when made), New (a note, optionally the site it is for and a prefix), switch off, switch
on, delete. The server's quota (5 by default) is shown and its refusal put in plain words. Masked
addresses only receive; they cannot be sent from, so compose is not involved. On a server that
refuses the feature the row is not shown.

## Tier 2: for administrators (needs the setup step)

| Feature | Server | Why on a phone |
|---|---|---|
| **Mail queue**: list, look, retry, cancel, pause | open | the thing an admin is paged for |
| **Accounts**: find, create, suspend, reset password, quota; groups, mailing lists | open | everyday requests |
| Blocked addresses: list, unblock | open | "I am locked out" |
| Server log search | open | first look at a problem |
| DMARC and TLS reports | open | read-only browsing |
| Reload settings, certificates; maintenance tasks | open | behind a confirmation |
| Domains and DKIM: state and DNS records | open | read-mostly |
| Edition and licence state | all | shown in the admin area |

## Tier 3: Enterprise

| Feature | Notes |
|---|---|
| **Dashboard** (optional) | live tiles (queue size, users, connections) and history charts; the app reads the chart definitions the server publishes and draws them itself |
| **Delivery history** ("where is my mail") | search by address, time or queue id |
| **Deleted-item recovery** | list and restore deleted mail, events, contacts, files; needs `full`, so it sits in the admin tier although it is each user's own data |
| Alerts | list, switch on and off. Alerts arrive as mail or webhooks, so the phone already notifies for one that is mailed |
| Tenants | list, quotas, used space |
| AI filter condition | "when the server's AI says ..." in the filter editor, for users with the AI permission |
| Server or tenant logo | on the sign-in screen and in the drawer |

Not for a client: read replicas, sharding, SCIM, compliance add-ons.

## How an administrator gets in (settled on the local server, 2026-11-14)

The admin's account is an ordinary mail account too, and it is added to the app like any other.
What the ordinary sign-in gives it is its mail only: the server keeps admin objects away from any
app that registered itself, whoever signs in. Tried on the local server (`dev/notes/stalwart-admin-access.md`):

- With the app's ordinary sign-in, every admin call is refused, also for an admin account.
- Asking for `settings` and `security` too adds the user's own things (masked email, password,
  app passwords, API keys, passkeys, connected apps) and nothing administrative.
- Asking for `full` as a self-registered app is refused.

So the admin area of an account is unlocked by one of these, in this order of building (owner:
API key first):

1. **API key.** Made in the web UI (or later in the app itself, under the `security` scope) and
   pasted once; kept in the Android Keystore; used only for admin calls of that account. Verified:
   a key made with inherited permissions carries exactly the account's permissions, also reaches
   the mail API, can be limited to chosen permissions, can expire, and five per account is the
   server's limit. An app password of the admin account works the same way.
2. **Registered app.** A superuser creates an OAuth client for this app with `full` (and the
   mail scopes) and its redirect address; the app then needs only that client id, typed in once,
   and signs in through it. Verified: the token then reaches every admin object the account's
   role allows, and `full` does not lift a normal user above their role.

The app can also create its own key: with the `security` scope a signed-in admin can make an API
key from inside the app, so "paste a key" can become "tap to allow admin access" after a normal
sign-in. Not tried yet.

## Proposed order

1. **Foundation and Tier 1**: ask for `settings` and `security` at sign-in where offered, feature
   detection, masked email, the account security screens.
2. **Admin access and the open admin features**: the key, queue, accounts, blocked addresses, logs.
3. **Enterprise**: dashboard, delivery history, recovery, the rest.

Each step is usable on its own. Step 1 can be built and tested on the local server except masked
email itself, which the local server refuses until the release that opens it.

## Seen on a licensed server (2026-11-14)

Read-only requests on the owner's server with an administrator's API key
(`dev/notes/stalwart-enterprise-observed.md` has the shapes, no values):

- The key works as the design assumes: edition `enterprise`, all 662 permissions, the admin
  objects and the mail API reachable.
- Masked email answers (an empty list). The object itself is still unseen: creating one is a
  write, which was not done.
- Metrics (stored rows and the live stream), delivery traces with their events, AI models (keys
  masked), tenants, alerts, the queue and the cluster node are all readable in the shapes the
  documentation gives. The live stream is plain server-sent events, one frame per interval.
- Deleted-item recovery is switched off on that server (`archiveDeletedItemsFor` is null), so
  the recovery screens can only be tested after it is switched on somewhere.
- The licence does not show in the `x:Enterprise` object there; the edition in `/api/account`
  is the only sign of it, so that is what the app goes by.
- The server's logo is served at `/logo` without sign-in, so the sign-in screen can show it.

## What is not known yet

A masked address as the server stores it (needs one to be created on a licensed server), an
archived item (needs recovery switched on), a fired alert, and a build with edition `oss`.
