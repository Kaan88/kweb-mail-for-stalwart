# Changelog

## 0.2.3

- Public links to your files: a link anyone can download from until it expires, with Copy, Share and Revoke. Only on servers that offer share links (the Stalwart fork at Kaan88/stalwart); hidden elsewhere.
- Library updates (base64, GitHub Actions).

## 0.2.2

- When an update is found, a popup says what changed, with Install or Later; the notes are also in the update notices and in About.
- Maintenance: weekly dependency updates and a security audit of the libraries.

## 0.2.1

- Updates download by themselves on Wi-Fi and ask to install; a manual check offers Download and install.

## 0.2.0

- Mail filters that work with Bulwark's: a visual editor with every field, comparator and action, a switch per rule, reorder, delete with Undo, a raw editor, and the old filters moved over. Credit to Bulwark webmail for the format.
- Fixed: repeating events did not repeat on the server; a send the server refused looked sent; sending could stop after 500 messages. Bulwark's tags show as labels.
- Your own calendars in the phone's calendar (two-way for single events, series read-only).
- Invitations, changes and shares from the server as notifications; "Not delivered to" in a thread.
- An optional Home tab and a sparkle when something deserves a look (overlaps, open invitations, overdue tasks, dates in mail), with assistant actions only on your tap.
- Sender check from your server's own results; earlier assistant chats; Catch me up.
- A weekly update check against the release page, on by default (switch it off in About).
- Six more colour themes, crimson by default, a sound button per receiving address, "Show with subfolders" kept per folder, your own instructions for the assistant.
- The assistant asks before an automatic change outside the conversation in view; a full resync when the server's state goes backwards.

## 0.1.0

First release.

- M0 to M2: sign-in with a passkey, live notifications, actions from a notification, settings.
- M3: reading, conversations, folders, search, star and move.
- M4: sending with Undo, drafts, reply and forward, attachments.
- M5: calendar with repeating events, invitations and tasks.
- M6: contacts and an away message.
- M7: AI assistant (bring your own service), files, shared folders, calendars and address books.
- M8: mail filters as one Sieve script.
- M9: signed and encrypted mail (OpenPGP and S/MIME), off by default.
- M10: notify per folder, folder management, spam and blocking, unsubscribe, labels, send later, several accounts, administration, Google push.
- M11: settings and polish after the design review.
