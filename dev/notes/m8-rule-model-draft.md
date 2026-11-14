# M8 filters: data model, Sieve, round trip, dangers, core API

Nothing was edited and no server was contacted. I read PLAN.md (M8 section), CLAUDE.md, `core/src/jmap/vacation.rs`, the away functions in `core/src/ffi.rs` and `AwayMessage` in `core/src/model.rs`, and the `ui/away` folder layout. Everything below about what Stalwart 1.1 accepts is unverified. Section 7 lists what to probe on the local server before building.

## 0. Departures from PLAN.md

1. Rules are stored as one JSON line per rule, next to that rule's Sieve, not as one metadata comment at the top. This keeps a hand edit local, and it makes the check in section 3 exact.
2. "Delete" files the mail into Trash. It does not `discard`. A rule that is wrong must not lose mail silently. The `discard` text is given in section 2 if you overrule this.
3. Size and body tests go in the first version. They are cheap: one line each.
4. The spam guard is one global stop at the top of the script, not a guard inside each rule.
5. There is no `copy` extension. "Also keep a copy in the Inbox" is an explicit `keep;` (section 2).

## 1. Rule model

The Rust side uses `uniffi::Record` and `uniffi::Enum`. Enums with fields are fine in UniFFI 0.32. `serde` derives also carry `#[serde(deny_unknown_fields)]`.

```rust
pub struct FilterSet { pub rules: Vec<FilterRule>, pub settings: FilterSettings }   // order = run order, top first

pub struct FilterSettings {
    pub skip_spam: bool,                 // default true: mail the server marked as spam is left alone
    pub folder_naming: FolderNaming,     // Path or Id, fixed when the script is saved (section 2.4)
    pub trash: Option<FolderRef>,        // resolved at save time; needed only if a rule deletes
}
pub enum FolderNaming { Path, Id }

pub struct FolderRef {
    pub mailbox_id: String,              // the JMAP id: the identity of the folder
    pub path: String,                    // last known path "Parent/Child", names joined with "/"; shown and used in Sieve
    pub gone: bool,                      // set by the folder refresh when the id no longer exists
}

pub struct FilterRule {
    pub id: String,                      // 10 random chars [a-z0-9], made by the core, never reused
    pub name: String,                    // 1 to 100 chars
    pub enabled: bool,
    pub match_mode: MatchMode,           // All = "all of", Any = "any of"
    pub conditions: Vec<FilterCondition>,   // 0 to 10; empty means "every message" (see 1.2)
    pub actions: FilterActions,
}
pub enum MatchMode { All, Any }
pub enum FilterCondition {
    Text { field: TextField, op: TextOp, value: String },
    Size { larger: bool, bytes: u64 },   // larger = size :over; otherwise :under. 1 to 2^40.
    Body { value: String },              // body text contains value (substring, not whole word)
}
pub enum TextField { From, Recipient /* To or Cc */, Subject, Header(String) }
pub enum TextOp { Contains, Is, BeginsWith, EndsWith }

pub struct FilterActions {
    pub move_to: Option<FolderRef>,
    pub keep_in_inbox: bool,             // "also keep a copy in the Inbox": meaningful with move_to or forward_to
    pub mark_read: bool,
    pub star: bool,
    pub forward_to: Option<String>,
    pub delete: bool,                    // = file into settings.trash
    pub stop: bool,
}
```

### 1.1 Limits

| What | Limit |
|---|---|
| Rules per script | 50 |
| Conditions per rule | 10 |
| Text value | 1 to 200 characters |
| Header name | 1 to 64 characters, from `!` to `~` except `:` |
| Rule name | 1 to 100 characters |

Every text field is trimmed, then refused if it contains a control character (U+0000 to 001F, 007F to 009F), U+2028, U+2029, or a bidi control (U+202A to 202E, 2066 to 2069). This is how a multi-line value is handled: the UI fields are single-line and the core refuses a newline. Header values are unfolded by Sieve and a subject has no newline. A multi-line body "word" would not match anything useful.

Rules saved by the app are always in canonical form: trimmed, `Header` names compared case-insensitively but stored as typed.

### 1.2 Refused combinations (`Error::Invalid` with a plain sentence, rule named)

| Combination | Why |
|---|---|
| no action and not `stop` | the rule does nothing |
| `delete` with `move_to`, `keep_in_inbox`, `forward_to`, `mark_read` or `star` | contradictory or pointless. "Forward and remove" is `forward_to` without a copy. |
| `delete` without `stop` | later rules could file the mail again. The UI shows Stop ticked and locked. |
| `delete` with no conditions | deletes all mail |
| `keep_in_inbox` with neither `move_to` nor `forward_to` | nothing to keep a copy of |
| `move_to` is Inbox or Drafts | no-op, or nonsense. Moving to Trash is the Delete action. |
| `move_to.gone` or the folder has no id in the account | the folder is gone |
| `move_to.path` contains "/" inside one name and `folder_naming` is `Path` | the path is ambiguous. Id mode has no such limit. |
| empty or whitespace-only value in a test | `contains ""` is true for every message |
| `Size { larger: true, bytes: 0 }` or `Size { larger: false, bytes: 0 }` | matches everything, or nothing |
| a `forward_to` that is not one plain address | see 1.3 |
| `forward_to` equal (case-insensitive) to the account's own address or any of its identities | loop to self |
| more enabled rules with `forward_to` than `maxNumberRedirects` | the server stops the script at run time (Stalwart: 1 per run). Counted statically, as the conservative bound. |
| a rule needing an extension the server does not list | see 2.1 |
| duplicate rule ids, empty name, over the limits in 1.1 | |
| script text larger than `maxSizeScript` | see section 4 |

Allowed but returned as warnings by `filters_check` (the save is not blocked):
- No conditions on a rule that moves or forwards without a copy ("this rule applies to all mail").
- A rule with no conditions and `stop` is followed by more rules ("the rules below can never run").
- Any forward rule: the privacy, spam-check and loop text from section 4.

### 1.3 Forward address

Exactly one `local@domain`. Local part: ASCII letters, digits and ``!#$%&'*+/=?^_`{|}~.-``. Domain: ASCII labels with at least one dot. No display name, angle brackets, quotes, spaces or commas. Internationalised addresses are refused for now. Length is at most 254.

## 2. The Sieve the core generates

Format: UTF-8, LF line endings, 4-space indent, one blank line between blocks. The generator is a pure function of `FilterSet`. All server-dependent facts (resolved folder paths, naming mode, trash) are stored inside the `FilterSet`, so the text can be compared byte for byte later (section 3).

### 2.1 `require`

The list is computed from the enabled rules that are not skipped, sorted, one line, and omitted when empty:

| Extension | Needed when |
|---|---|
| `"body"` | a `Body` condition |
| `"comparator-i;unicode-casemap"` | a `Text` or `Body` value contains a non-ASCII character |
| `"fileinto"` | `move_to` or `delete` |
| `"imap4flags"` | `mark_read` or `star` |
| `"mailboxid"` | `folder_naming` is `Id` and a `fileinto` is used |

`redirect`, `keep`, `stop`, `size`, `address`, `header`, `allof`, `anyof` and `if` are base RFC 5228 and are never listed. `discard`, `variables` and `copy` are not used.

The core checks the list against `sieveExtensions` from the session capability before any network write. If one is missing it refuses with a sentence naming the feature, for example "This server does not support filtering on the message text."

### 2.2 Strings

Two functions, and nothing else writes user text into the script.

- `quote(s)`: `"` + `s` with `\` becoming `\\` and `"` becoming `\"` + `"`. No other escape exists in Sieve quoted strings. Non-ASCII is written as UTF-8.
- `quote_pattern(s, begins, ends)`, used for `:matches` (begins with and ends with): first escape `\` to `\\`, `*` to `\*`, `?` to `\?` (this makes the pattern literal); then add `*` at the end (begins with) or at the start (ends with); then pass the result through `quote`.

Example: a "begins with" of `50%*off` gives the pattern `50%\*off*` and the source `"50%\\*off*"`. A subject `say "hi" \ now` with Contains gives `"say \"hi\" \\ now"`.

Header names go through `quote` too, after the token check in 1.1. Numbers are formatted from `u64`. Rule names and ids never appear as raw text outside JSON (2.5), so no value can reach a comment line.

### 2.3 Tests

| Rule condition | Sieve |
|---|---|
| From contains | `address :contains "from" "v"` |
| From is | `address :is "from" "v"` |
| From begins / ends with | `address :matches "from" "v*"` / `"*v"` (patterns escaped as in 2.2) |
| Recipient (To or Cc) | `address :contains ["to", "cc"] "v"` (the same with `:is` or `:matches`) |
| Subject | `header :contains "subject" "v"` (and `:is`, `:matches`) |
| Any header | `header :contains "X-Foo" "v"` (and `:is`, `:matches`) |
| Size larger / smaller | `size :over 1048576` / `size :under 10240` (bytes, plain number) |
| Body | `body :text :contains "v"` |

When the value is non-ASCII, `:comparator "i;unicode-casemap"` goes first in the argument list: `header :comparator "i;unicode-casemap" :contains "subject" "über"`. Otherwise the default `i;ascii-casemap` is used. All matches are therefore case-insensitive, and the UI says so.

Combining:
- Two or more conditions: `if allof (T1, T2) {` or `if anyof (T1, T2) {`.
- One condition: `if T1 {`.
- Zero conditions: `if true {`.

### 2.4 Actions, in this fixed order inside the block

1. `addflag "\\Seen";` for mark read, then `addflag "\\Flagged";` for star. In Sieve source `\\` is one backslash, so the server sees `\Seen`. The flags are cumulative across rules. A later rule's `fileinto` or `keep` carries them too, which is the intent: the message stays read or starred wherever it ends up. This is why `addflag` is used and not `:flags`.
2. `redirect "addr";` for forward.
3. `fileinto "Path/Name";` for move, or for delete (Trash).
4. `keep;` when `keep_in_inbox`.
5. `stop;`.

Because `keep` is an explicit action, a later rule's `fileinto` cannot cancel the Inbox copy. That is why `:copy` is not used. A move without `keep` leaves the implicit keep cancelled. Flags alone: the implicit keep stays.

Folder naming:

- **`Path` (default)**: `fileinto "Receipts/2026"`. Non-ASCII goes in as UTF-8, because RFC 5228 mailbox names are UTF-8 strings. A folder at the top level has just its name. Subfolders are joined with "/". Is the Inbox subfolder written `Inbox/Sub`? Verify (section 7).
- **`Id`**: `fileinto :mailboxid "k3x" ;`, used only if `sieveExtensions` lists `mailboxid` and the probe in section 7 shows it works on this Stalwart.
  - A renamed folder keeps working.
  - A deleted folder makes `fileinto` fail, which by RFC 5228 section 2.10.6 means implicit keep, so the mail stays in the Inbox.
  - So the core prefers `Id` when it works, and `Path` otherwise.
- Folder naming is isolated in one function `sieve_target(&FolderRef, FolderNaming) -> String`, so the probe result changes one place.

Consequences in `Path` mode:

- **Renamed or moved folder**: the stored `path` goes stale and mail would file into the old name. Stalwart may even create it. So the core runs `refresh_folders(set, mailboxes)`: for each `FolderRef`, look up `mailbox_id` in the current mailbox list and rebuild `path`; if the id is gone, set `gone = true`.
  - On opening the filter screen it only reports the difference.
  - After each mailbox sync, if the state is `Managed`, it should rewrite the script silently (best effort). I recommend that; it is the owner's call, because it is a network write caused by sync.
  - A folder that cannot be rebuilt stays stale and is shown in `folder_notes`.
- **Deleted folder** (`gone = true`): the rule is generated as if disabled (data line only, no Sieve), and the UI shows "Folder gone, pick another". Mail then stays in the Inbox, which is the fail-safe direction. In `Id` mode the server does the same by itself.
- **Folder name with "/" in it**: refused as a target in `Path` mode, allowed in `Id` mode.

### 2.5 Complete example

Rules: (1) "Receipts": any of from contains `paypal.com`, subject contains `receipt` → mark read, move to `Receipts`, stop. (2) "Boss": from is `boss@work.com` → star. (3) "Forward invoices": subject begins with `Rechnung ä*` → forward to `me@other.org`, keep a copy. All enabled, Path mode, skip_spam on.

```
# kweb-filters v1
# Written by kweb mail. Change the rules in the app; a hand edit makes this script read-only there.
# settings {"skip_spam":true,"folder_naming":"path","trash":null}
require ["comparator-i;unicode-casemap", "fileinto", "imap4flags"];

if header :contains "X-Spam-Status" "Yes" {
    stop;
}

# rule a1b2c3d4e5 begin
# data {"id":"a1b2c3d4e5","name":"Receipts","enabled":true,"match_mode":"any","conditions":[{"text":{"field":"from","op":"contains","value":"paypal.com"}},{"text":{"field":"subject","op":"contains","value":"receipt"}}],"actions":{"move_to":{"mailbox_id":"m7","path":"Receipts","gone":false},"keep_in_inbox":false,"mark_read":true,"star":false,"forward_to":null,"delete":false,"stop":true}}
if anyof (address :contains "from" "paypal.com", header :contains "subject" "receipt") {
    addflag "\\Seen";
    fileinto "Receipts";
    stop;
}
# rule a1b2c3d4e5 end

# rule f6g7h8i9j0 begin
# data {...}
if address :is "from" "boss@work.com" {
    addflag "\\Flagged";
}
# rule f6g7h8i9j0 end

# rule k1l2m3n4o5 begin
# data {...}
if header :comparator "i;unicode-casemap" :matches "subject" "Rechnung ä\\**" {
    redirect "me@other.org";
    keep;
}
# rule k1l2m3n4o5 end
```

The last test shows pattern escaping: the begins-with value `Rechnung ä*` becomes the pattern `Rechnung ä\**` and the source `"Rechnung ä\\**"`.

Notes on the layout:
- With `skip_spam` off, the spam block is absent.
- A disabled or skipped rule is its `begin`, `data` and `end` lines only.
- A rule that moves mail to a folder whose name contains `"` goes through `quote` as usual, for example `"Q\"1"`.
- The `X-Spam-Status` header name is a belief about Stalwart, not a verified fact (section 7).
- If the owner overrules the Trash choice, "delete" becomes `discard;` plus `stop;` and needs no `fileinto`.

## 3. Round trip and states

### 3.1 Format, and the exact check

The script must be `generate(parse_data_lines(text))` byte for byte (after normalising CR LF to LF and a missing final newline). The `# data` JSON lines are the source of truth. The Sieve beside them is only ever checked, never read for meaning. So there is no Sieve parser at all.

Grammar the core accepts (lines are separated by `\n`; matching is exact):

```
script   := line1 line2 settings [require] blank guard* block*
line1    := "# kweb-filters v" N                       ; N = 1 now
line2    := "# Written by kweb mail. Change the rules in the app; a hand edit makes this script read-only there."
settings := "# settings " JSON                          ; FilterSettings, compact, field order as declared
require  := "require [" string ("," SP string)* "];"
guard    := "if header :contains \"X-Spam-Status\" \"Yes\" {" ... "}"    ; only if skip_spam
block    := blank "# rule " ID " begin" NL "# data " JSON NL body "# rule " ID " end"
body     := ""                                           ; rule disabled or skipped
          | canonical Sieve of that rule
```

The parser:

1. Checks line 1 for the marker and the version.
2. Splits into lines, parses the `# settings` line and every `# data` line with serde (`deny_unknown_fields`).
3. Regenerates the whole text from the parsed set.
4. Compares.

Equal means the app's script. Not equal means hand-edited. Nothing in a block is trusted other than the JSON. A value such as `# rule x end` inside a rule is harmless because it sits inside a JSON string on a `# data ` line.

Versions: when the generator's output changes in any way, `v` goes up and the old generator stays in the code under a golden test. A script with an older known version is read with its own generator and offered as "update" (rewrite at the current version). A newer version gives `Newer` (read-only, "update the app"). Without that rule an app update would make every saved script look hand-edited.

### 3.2 The script's name

The app's script is named `kweb-filters`. It is found by name only; other scripts are listed but never downloaded, except the active foreign one. `vacation` is reserved: it is the away message (M6), never shown as a script, never touched. One backup name is used, `kweb-filters-backup`, only when the user confirms replacing something.

### 3.3 States

```rust
pub enum FiltersView {
    Unavailable,                                   // no urn:ietf:params:jmap:sieve: the row is hidden
    Empty   { other: Option<ScriptInfo>, limits: FilterLimits, state: String },
        // no kweb-filters script; nothing foreign is active (a foreign inactive script is ignored)
    Managed { set: FilterSet, active: bool, paused_by: Option<String>, folder_notes: Vec<FolderNote>,
              upgrade: bool, limits: FilterLimits, state: String },
    Edited  { text: String, readable: Option<FilterSet>, active: bool, limits: FilterLimits, state: String },
        // our marker, but text != regenerated text, or the data lines do not parse
    Newer   { version: u32, text: String, active: bool, state: String },
    Foreign { name: String, text: String, is_ours_by_name: bool, active: bool,
              limits: FilterLimits, state: String },
        // a script that is active and not ours; or kweb-filters without our marker
}
pub struct ScriptInfo { pub id: String, pub name: String, pub is_active: bool }
pub struct FilterLimits { pub max_script_bytes: u64, pub max_redirects: u32, pub max_scripts: u32,
                          pub extensions: Vec<String> }
pub struct FolderNote { pub rule_id: String, pub kind: FolderNoteKind /* Renamed | Gone */, pub path: String }
```

`state` is the JMAP `SieveScript` state string. `paused_by` is `Some(name)` when ours is not active because another script is: usually the reserved `vacation`. The UI then says "Your filters are paused while 'vacation' is on." and does not re-activate (that would switch the away message off). Verify how Stalwart really handles this (section 7).

What the user can do in each state:

| State | Allowed |
|---|---|
| `Empty` | build rules; the first save creates and activates `kweb-filters` |
| `Managed` | edit, reorder, toggle, delete; the whole set is rewritten on each change |
| `Edited` | read the text, copy it; "Restore my rules" (when `readable` is `Some`: rewrites the canonical form; the edited text goes to the backup); "Start over". Both behind a confirmation. |
| `Newer` | read only |
| `Foreign` | read the text (read-only, monospace, copy button); "Replace with rules" behind a confirmation |

Replace with rules for a foreign script that has another name: create `kweb-filters` and activate it. That deactivates the foreign script. It is not deleted, and the UI says "Your script 'X' stays in your account, switched off" and offers "Switch it back on" (`filters_activate_other`). For `kweb-filters` without our marker, or an `Edited` script: the old script is first renamed to `kweb-filters-backup` (an older backup is deleted, and the confirmation says so), then the new one is created. If the account is at `maxNumberScripts`, the replace is refused.

The confirmation is enforced in the core, not in the UI: a save carries a `SaveIntent` (section 5). The core re-reads the server state and refuses to write when the state found does not match the intent. A UI bug therefore cannot overwrite a foreign script.

## 4. Dangers and how the design deals with them

| Danger | What the design does |
|---|---|
| Forward-all and loops | A rule with no conditions that forwards or moves gets a warning. Forwarding to your own address or an identity of the account is refused. Enabled forward rules above `maxNumberRedirects` are refused (Stalwart: 1). The global spam stop keeps spam from being forwarded. Two accounts that forward to each other cannot be detected; the UI says forwarding can loop and can be rejected by the target (SPF, DMARC), and the server's hop limit is the net (unverified). When `max_redirects` is 0 the forward action is hidden. |
| A rule that deletes everything | A delete rule with no conditions is refused. A delete goes to Trash, so it can be undone. No `discard` is offered. |
| Injection | No user text is concatenated into Sieve outside `quote` or `quote_pattern`. Header names are checked against the token set, addresses against 1.3, numbers are formatted from `u64`. Control characters, line separators and bidi controls are refused. Names and ids appear in comments only inside JSON strings, so a name cannot break out of a comment. A property test generates hostile strings (quotes, backslashes, `*`, `?`, `#`, `}`, `;`, `"`, a Sieve fragment) and checks the script still has the same number of `if` blocks and the same actions as the rules say. |
| Script too large | The generated text, JSON lines included, is measured in bytes and compared to `maxSizeScript`. The JSON about doubles the size; that is accepted for readability. The limits in 1.1 cap it at roughly 50 rules times a few KB. Over the limit: `Error::Invalid("The filters are too big for this server (N of M bytes). Remove a rule.")`. |
| Rule order | Rules run top to bottom, and every matching rule runs unless it has `stop`. This is the standard Sieve behaviour, and the UI says so once. A new move or delete rule starts with Stop ticked. A rule with no conditions plus stop makes the later ones unreachable: warning. Reorder is by up/down buttons. Flags are cumulative (2.4). |
| Notifications | The notification rule looks only at unread Inbox mail. Mail a rule files elsewhere never reaches the Inbox, so there is no notification; mail the rule marks read is not notified either. The UI shows a fixed note at the top of the editor and a small badge on every rule that moves, deletes or marks read. Wording: "Mail these rules move out of the Inbox gives no notification." A rule that keeps a copy in the Inbox still notifies, if it is not marked read. |
| Concurrent change | The save passes the `state` from the load. The core uses `ifInState` on `SieveScript/set`; a mismatch is `Error::Temporary("The filters were changed somewhere else. Open them again.")` and nothing is written. |
| Overwriting someone else's script | See section 3: intents, backup, confirmation. |
| A server that accepts a script and then does something else | Save validates with `SieveScript/validate` first, and the server's description is shown. A save that fails leaves the old script in place (the update is one `SieveScript/set`). After a successful save, the core re-reads the script and checks it is classified `Managed` with the same rules; otherwise it reports that the server changed the text. |

## 5. Functions the core offers

New files: `core/src/filters/{model.rs, check.rs, sieve.rs, parse.rs}`, pure and testable without a network. `core/src/jmap/sieve.rs` holds the JMAP calls, following `vacation.rs`: `SieveScript/get`, `/set` (create, update, destroy, `onSuccessActivateScript`), `/validate`, and `Blob/upload` and download for the text. `ffi.rs` gets a section `// ---- M8: filters ----` with the same `with_connection` wrappers. There is a `needs_sieve()` check like `needs_vacation()`: no capability gives `Error::invalid("the server does not support filters")`. Errors are the existing `Error::Invalid` (a refusal, text safe to show), `Error::Temporary` (network and conflict), and `Error::SignInNeeded`; nothing of a server's text goes into a log.

```rust
// ---- M8: filters ----

/// Whether the section should be shown (no network).
pub fn filters_available(&self) -> bool

/// What the account has (network): see FiltersView. Also refreshes folder paths against the cached mailboxes.
pub fn filters_view(&self) -> Result<FiltersView>

/// The refusals and warnings for a rule set (no network; reads session limits and cached mailboxes).
/// Empty means fine. The UI shows these live while editing.
pub fn filters_check(&self, set: FilterSet) -> Vec<FilterFinding>
pub struct FilterFinding { pub rule_id: Option<String>, pub severity: Severity /* Error | Warning */,
                           pub code: String, pub message: String }

/// The Sieve text for a set (no network): "show script". Error::Invalid if the set is refused.
pub fn filters_script(&self, set: FilterSet) -> Result<String>

/// A new rule with a fresh id, name and one condition from a message (cache; for "create a rule from this message").
pub fn filters_rule_from_message(&self, message_id: String, kind: SeedKind /* Sender | Domain | ListId */) -> Result<FilterRule>
pub fn filters_blank_rule(&self) -> FilterRule

/// Saves the set (network): refuse, validate on the server, write, activate. The intent says what the user agreed to.
pub fn filters_save(&self, set: FilterSet, intent: SaveIntent, state: String) -> Result<FiltersSaved>
pub enum SaveIntent {
    Update,                                   // the script is Managed
    Create,                                   // no script, nothing foreign active
    TakeOver { script_id: String },           // deactivates that foreign script; it stays in the account
    ReplaceEdited { script_id: String },      // the old text goes to kweb-filters-backup
}
pub struct FiltersSaved { pub state: String, pub set: FilterSet }   // normalised, folder paths refreshed

/// Switches our script on or off (network). Switching on while another script is active is refused unless intent is given.
pub fn filters_set_active(&self, active: bool) -> Result<String>
/// Puts a foreign script back (network): activates it, which deactivates ours.
pub fn filters_activate_other(&self, script_id: String) -> Result<String>
```

The read of a script is made by `filters_view`; no separate "get text" is needed. A "delete my filters" action is not included in the first version (switch off is enough; the user can delete the script with another client).

How the save works:
1. Normalise the set and run `check`; refuse on errors.
2. Resolve folders (`refresh_folders`).
3. Generate.
4. `Blob/upload`, then `SieveScript/validate`; a server `invalidScript` or `invalidSieve` becomes `Error::Invalid("The server did not accept the filters: <its description>")`.
5. `SieveScript/set` with `ifInState`: create with `onSuccessActivateScript: "#c"` or update the `blobId`.
6. Re-read, classify, compare.

PLAN.md's note stands: accept both `invalidScript` and `invalidSieve`, and both `scriptIsActive` and `sieveIsActive`.

Android: `ui/filters/{FiltersScreen, FilterEditScreen, FiltersViewModel, FiltersRules.kt}`, as `ui/away` is built. `FiltersRules.kt` holds wording and the pure helpers, with a unit test like `AwayRulesTest`. The Settings row sits next to Away and is hidden when `filters_available()` is false. The UI never builds Sieve; it calls `filters_check` on every edit and `filters_save` per change, with Undo being a save of the previous set.

## 6. Tests

All of these are core tests against the mock server in `core/tests/support/`, except where marked.

**Generator**
1. Golden file per feature, one rule each: each test field and operator, each size direction, body, each action, each combination in 1.2 that is allowed. Plus the full example in 2.5 (byte for byte).
2. Quoting: a value with `"`, `\`, `\"`, `*`, `?`, `#`, `;`, `}`; a non-ASCII value (`ä`, an emoji, a CJK folder); a folder with "/" in `Path` mode (refused) and in `Id` mode (fine); a folder named `Q"1`.
3. `require` computed: the empty set has none; each feature adds only its own extension; a disabled rule adds none; the output is sorted and has no duplicates; `unicode-casemap` appears only with non-ASCII.
4. Disabled and `gone` rules give a data line only. `skip_spam` on and off.
5. Pattern escaping: begins and ends with, with a value containing `*`, `?` and `\`.
6. Refusals in the table in 1.2, one test each, plus the limits in 1.1 (boundary values) and the script size limit.
7. The forward count against `max_redirects` 0, 1, 2; forward to the account's own address and to an identity.
8. Capability gating: missing `body`, `imap4flags`, `mailboxid`; the sentence names the feature.

**Parser and round trip**
9. Property: `parse(generate(set)) == set` for generated sets: random rules, random Unicode strings (including `"`, `\`, `\n` that must be refused earlier, emoji, RTL), random order and enabled state.
10. Property: `generate(parse(generate(set))) == generate(set)`.
11. Tamper: take a generated script and apply a one-byte change, delete a line, swap two blocks, change a `# data` value, change a Sieve line, add a trailing space, change CRLF to LF and back. Property: the classification is `Managed` only if the text is equal to the original after the allowed normalisation (CR LF; final newline); else `Edited`, `Foreign` or `Newer`. A `Managed` result must never carry rules other than the originals.
12. First-line variants: missing marker, `v0`, `v2`, `v1x`, BOM, leading blank line, wrong case. None may be `Managed`.
13. Old-version golden: a stored `v1` text still parses after the generator changes (the frozen generator).
14. A `# rule x end` text inside a value, a `# data` text inside a value, and a very long line, all round trip.

**State machine and JMAP (mock server)**
15. `filters_view` for each state: no capability; no script; ours Managed active; ours inactive because `vacation` is active (`paused_by`); ours Edited; ours without marker; foreign active; `Newer`.
16. `filters_save` refusals by intent: `Update` on a foreign script, `Create` when another script is active, `TakeOver` with a wrong script id, `ReplaceEdited` with a stale state. None may write.
17. TakeOver keeps the foreign script and deactivates it; `filters_activate_other` reverses it. `ReplaceEdited` creates the backup and refuses at `maxNumberScripts`.
18. `ifInState` conflict gives `Temporary` and no change. A server `invalidScript` or `invalidSieve` becomes `Invalid` with the description. A failed save leaves the old script.
19. Folder refresh: renamed gives a new path and a `Renamed` note; deleted gives `gone`, which skips the rule in the script and gives a `Gone` note; a moved parent changes the child's path.

**Real server (local Stalwart; marked `#[ignore]`, run by hand as the other real-server checks are)**
20. `SieveScript/validate` accepts the whole golden corpus, and rejects a corrupted script (check the real error names).
21. Delivery: deliver test mail with the seed script and check where it lands for move, mark read, star, keep a copy, delete, stop, begins and ends with, size, body, non-ASCII subject, a folder with a non-ASCII name and a subfolder.
22. Rename and delete a target folder, and see where mail lands in `Path` and `Id` mode.

**Optional.** Use the `sieve-rs` crate (Stalwart's own Sieve compiler and runtime) as a dev-dependency. Compile every generated script offline in CI, and run it against sample messages for the delivery cases. Check its licence and its version against the server's before adding it.

## 7. Facts to probe on the local Stalwart before building (none are verified)

1. The path separator for `fileinto` and whether the Inbox subfolder is written `Inbox/Sub`.
2. Whether `fileinto :mailboxid` works (`mailboxid` listed in `sieveExtensions`), and where mail lands when the id does not exist.
3. What `fileinto` does with a folder that does not exist: error, create, or Inbox.
4. Whether UTF-8 folder names work in `fileinto`.
5. Whether `i;unicode-casemap` and `body` are supported.
6. Whether spam mail gets the `X-Spam-Status: Yes` header, and whether user scripts run on mail the server already filed as spam.
7. Whether the script text comes back as it was written (no rewrite, no CRLF change, no stripped comments).
8. The capability values: `maxSizeScript`, `maxNumberRedirects`, `maxNumberScripts`, `sieveExtensions`.
9. What the `vacation` script does to our active script, in both directions (PLAN.md says it displaces it).
10. The exact `SetError` types and descriptions for `SieveScript/validate` and `/set`.
11. Whether `redirect` also fails the run when the limit is exceeded, or only skips the extra ones.
