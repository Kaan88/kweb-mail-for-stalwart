# Audit brief: everything since 0.1.0 (M12)

Written 2026-11-16 for a future Fable audit. The range is `v0.1.0..HEAD`: about 35 commits, 232 files, 25,000 lines added. Almost all of it was written by Sonnet agents (stage 8's phase 1 core by an Opus agent) and reviewed by the main session from the agents' reports and a few diffs, **not line by line**. Treat every claim below as the author's report until you have checked it.

PLAN.md, M12, has a "Stage N built" section per stage with what was decided and what was and was not checked. CLAUDE.md "Where it stands" has the rules to keep per stage. This note says where to look hardest.

## How to audit

1. Read CLAUDE.md, then PLAN.md M12 in full.
2. For each stage below, read the code against its rules, not against its tests: the tests were written by the same agent as the code.
3. Run the core suite once (`cargo test --workspace -j 4 -- --test-threads=4`; never twice within minutes on Windows), clippy, and the Kotlin build, lint and tests. Never two builds at once; the owner's other projects build on the same PC.
4. Open every screen named under "never seen" on the emulator `kweb35` against the local Stalwart (`dev/README.md`, Windows notes in the memory file).
5. Write findings ordered by harm. Change nothing until the owner has picked.

## Where to look hardest

These are the places where a mistake loses or leaks data, or acts without the user. Check each one against the rule, with a test of your own where the agent's tests look thin.

| Area | Rule | Files |
|---|---|---|
| Sending | Never twice, never lost. Stage 3 changed the send path: refused recipients move the message back to Drafts and destroy the record; submissions are destroyed after success and pruned on `overQuota`. A cancelled record must stay (it stops an undone message from going out). | `core/src/jmap/send.rs`, `core/src/compose/flush.rs`, `later.rs` |
| Device calendar | A phone edit is never lost silently, an event is never made twice, the deletion guard stops a run before any delete, nothing of a shared account reaches the provider, sync ids carry the app account. Stage 5 changed switch-off to keep unsynced edits. | `core/src/jmap/devcal/`, `core/src/ffi/devcal.rs`, android `devcal/`, `ui/devcal/` |
| Assistant changes | A change only through `ai_apply` on the user's tap (or the allow-list in a chat with automatic changes on). Stage 5 added `Chat::in_view`; stage 8 added `add_task` to the allow-list and `ai_apply_all` with one undo; stage 6 added earlier chats. Check that a reopened chat cannot carry automatic changes or consent to another service or model, and that mail or tool output can never trigger a change. | `core/src/assistant/` (`changes.rs`, `chat.rs`, `sparkle.rs`, `catchup.rs`), `core/src/ffi/ai_chat.rs`, `ffi/sparkle.rs` |
| Sparkle runs | A run sends exactly what the finding names, after consent; "Ask before each run" asks for runs that send mail. Look in mail has read tools only and a step limit. | `core/src/assistant/sparkle.rs`, android `ui/attention/` |
| Filters | The app writes only a script it can read as rules, keeps the external section and unknown keys byte for byte, never touches `vacation`, needs a read before a write. Block sender must not switch a script off. | `core/src/filters/bulwark/`, `core/src/ffi/filter_rules.rs`, android `ui/filters/` |
| State going backwards | A full fetch must never drop drafts, the outbox or held rows. | `core/src/jmap/state.rs`, the sync code that calls it |
| Notices | Shown once, then destroyed on the server; only notices newer than the first look. Check nothing of a shared account's content leaks into a notification beyond what the notice itself says. | `core/src/notices.rs`, `core/src/jmap/notices.rs` |
| Sender check | Only the topmost `Authentication-Results` from the receiving server counts; a header the sender could add must never produce "verified". | stage 6 commit 3484096, `dev/notes/stalwart-sender-check.md` |
| Update check | No personal data in the request, never downloads or installs by itself. | stage 6 commit 51699d8 |
| Secrets and logs | No key, token, mail text, chat text, event title or description in a log or an error. Grep every new `log::`, `Log.` and error string in the range. | whole range |

## Per stage

- **Stage 1** (d18d0e3): crimson default, lasting "with subfolders", one row per address, the assistant's "Your instructions". Low risk. Check the instructions frame cannot lift any rule.
- **Stage 2a and 2b** (9b62003 to c465115): Bulwark-compatible filters, core and editor. The generator was never run against Bulwark's own code; its strings come from reading `generator.ts`. A useful check: generate a script in Bulwark from the same rules and diff. The owner's own script has never been opened in the app; open it read-only first.
- **Stage 3** (6a5c6b4 to 0f1c125): the three bugs and `$label:` tags. Bugs were real on the local server. Not built: a one-off repair of old repeating events (repaired on next save only).
- **Stage 4** (835c185 to 9e94e34): device calendar. Design from Bulwark Mobile (AGPL), no code copied: confirm no file in `devcal/` resembles theirs closely. Checked through the provider only, never in a calendar app; series edits, attendees and Google push mode not covered.
- **Stage 5** (b9c2d7e to c1b7729): `Chat::in_view`, state going backwards, notices, small checks, switch-off keeping edits, "Not delivered to". The switch-off dialog and the not-delivered line were never seen on a screen.
- **Stage 6** (3484096, 51699d8, 8bf0633, 9dd191f, 09a5513): sender check, update check, earlier chats (migration 14, `ai_chats`) and Catch me up. **No screen of this stage was opened on the emulator** (it did not boot after a freeze); tests and the Gradle build only. Check first: the sender check believes only the topmost `Authentication-Results` whose authserv-id matches the topmost `Received` host with no sender header above it (Stalwart does not strip forged copies; probe mails "SMTP probe ..." are in the tester's Inbox); a reopened earlier chat never carries automatic changes or another service's consent; Catch me up sends only headers and cached first lines of at most 30 unread Inbox mails and parses the answer strictly; the update request carries nothing about the user. Commits 3484096 and 51699d8 were not compiled on their own. The local Stalwart got an extra loopback SMTP bind on port 25 for the probe.
- **Stage 7** (77bdb92): six palettes. Seen on Appearance only; not on other screens or with pure black.
- **Stage 8** (736439b to 6c0bc06): attention layer, Home tab, sparkle actions. 13 rules with English and Turkish patterns. Checked against a scripted stub service only, never a real model. Several rules are approximations (see PLAN.md "Stage 8 built").

## Never seen on any screen

Every stage 6 screen (sender chip and sheet, About update rows, Earlier chats, Catch me up); the "Remove anyway" device-calendar dialog; the "Not delivered to" line; the Home status chips; first-use consent for sparkle actions; the Calendar box and thread menu in dark; share notices; every M9 screen; the Service dropdown; the new palettes outside Appearance. Nothing of M12 has been seen on the owner's phone.

## Never run against the real thing

A real AI service for sparkle actions, Catch me up and earlier chats (only the mock and a stub); a real calendar app editing the device calendar; Bulwark's own generator; a partial refusal through the app; Google push with the new notice types.

## Licences to confirm

Bulwark webmail and Bulwark Mobile are AGPL-3.0 with an app store permission; the app is MIT. Only design and format were taken. Check the About credits match, and check the licences of the OpenPGP and S/MIME crates before any store release.
