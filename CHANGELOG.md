# Changelog

## 0.3.0

- Reading: a mail with a table much wider than the screen now shows its text at full size; only the table is made smaller, as in Gmail, and a wide table in a folded quote is fitted when the quote is opened.
- Assistant: Anthropic as a protocol of its own is gone; every service speaks the OpenAI protocol, and you can keep several, each with its own name, address, model, key and agreement, and pick the one in use. Your OpenAI-compatible service is carried over with its key; an Anthropic setup is removed, and if it was in use the assistant is switched off.
- Assistant: the chat works the same with Gemini, Mistral, DeepSeek, Groq and OpenRouter: a refused request is no longer taken for a model without tools, what a service sends with its tool calls (Gemini's signatures, reasoning) goes back to it, and the model no longer claims it can only read when it can suggest changes.
- Fixes from a review of the week: Undo stays for the mail already marked when Mark all as read fails part-way, and mail in the oldest second of the loaded part is marked too; deleting a domain removes only the DKIM keys the server says are that domain's; the Files bar counts exactly what Delete and Move act on; a key typed for one assistant service is not sent to another address.

## 0.2.9

- Reading: a mail whose table of short cells grew with the text no longer wraps its running text at two thirds of the screen; tables without a width of their own are drawn back at their size. Web addresses written as plain text are links, in HTML and plain-text mail.
- Files: choose several entries with a long press (as in the mail list), then move or delete them together.
- Server administration: an Actions screen, as in Stalwart's own: reload settings, certificates, lookup stores and blocked addresses, update apps, empty caches, pause and resume the outgoing queue, a DMARC check and a spam check.
- Assistant: models that cannot chat are left out of the model list, and "the service does not know this model or address" now says which of four things went wrong; the effort levels come from the service itself, for OpenAI-compatible services and Anthropic alike, and are sent only where the service offers them.

## 0.2.8

- Writing: the assistant's "Draft a reply" has nine choices to tap before it starts (accept, decline, ask for details, propose a meeting, thanks, short, formal, friendly, a named language), at most one of each kind; what you type still wins.
- Menus: a long press in the folder drawer, the Files list and the Delivery and Queue screens opens the menu at your finger, not at the far edge of the row.
- Reading: a mail with one wide fixed element (an Outlook-style signature table) is no longer shrunk to unreadable text; the page is laid out again at the mail's width with the text size scaled back up.

## 0.2.7

- Writing: "Draft a reply" in the assistant menu of a reply or forward, with an optional hint; "Put in reply" puts it above the quote, with Undo. Translate lists every language by its own name, with search and the recent ones first.
- Reading: PDF attachments open in the app (pinch zoom, "Open with"); a drag that starts inside a zoomed mail stops at its edge, only one that starts at the edge changes conversation; a long forwarded message is no longer folded for its length.
- Server administration: domains (with DKIM keys and a guarded delete), background tasks, reports and the cluster.
- Push through Google: after switching it on for one account, the app offers to switch it on for the other accounts that still keep the permanent connection (one of them keeps the status-bar entry).
- Icons in every context menu.
- Fixes from a review of everything since 0.2.4: Mark all as read no longer marks unseen new mail on the server; opening at the first unread stops following after 3 s; very tall PDF pages no longer crash; task delete and retry are guarded in the core; a race in the download service.

## 0.2.6

- Reading a mail: a mail laid out for a wider screen (most newsletters) keeps its own layout and is scaled down until it fits, instead of having its tables squeezed; pinch zoom works, only the mail scales, and the zoomed mail moves inside the frame it had when fitted, handing a drag at its edge on to the pager or the page.
- Mark all as read in a folder (the folder's menu in the drawer, also with its subfolders when they are shown), with Undo; unread mail older than the cached part is marked on the server too.
- A conversation opens at the first message that was unread.
- Home and attention: a finding for a mail you sent that asks something and has had no answer for 3 to 14 days, with Open; it needs no assistant and has its own switch.
- The account menu always opens fully. url 2.5.9 (RUSTSEC-2026-0592).

## 0.2.5

- Reading a mail: the sender check, Unsubscribe, labels and the encryption mark share one compact row; quoted text folds in long conversations (only a quote with nothing after it, or a long one); mail is fitted to the screen (tables, pictures, long words) and only the mail content zooms; a pinch and a swipe at the edge of a wide mail no longer fight the pager.
- Notifications: a Copy code button for verification codes, first among the buttons, on an unlocked phone only; it can be switched off in Notifications.
- Writing: the recipient's name after a greeting and the rest of a sign-off are offered as a suggestion (switch in Swipes and actions).
- Settings is in the account menu; the Home unread count can cover chosen folders; the assistant's "Your instructions" can be 8000 characters.

## 0.2.4

- The app no longer contains a pre-built library: the part that checks server certificates is built from source (copied from rustls-platform-verifier, unchanged).
- A second build without the built-in updater, for F-Droid, signed with the same key. Release builds are reproducible on Linux.

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
