<p align="center"><img src="branding/ward/png/ward-128.png" width="96" alt=""></p>

<h1 align="center">kweb mail for Stalwart</h1>

<p align="center">A native Android client for <b>Stalwart</b>: mail, calendar, contacts and files in one app.<br>
Simple to use, with everything there when you need it.</p>

<p align="center"><a href="https://github.com/Kaan88/kweb-mail-for-stalwart/releases/latest"><b>Download the latest release</b></a></p>

---

Made for Stalwart 1.1 and later. It speaks JMAP only: no IMAP, POP3 or SMTP. Other JMAP servers are not tested and may not work. Android 10 or later.

## What it does

**Mail**
- Conversations, folders and labels, search with highlights, star, move, swipes you choose.
- Send with Undo, send later, drafts, reply and forward, attachments, signatures and several senders.
- Unsubscribe, report spam, block a sender, a check of who really sent a mail.
- Pictures off until you allow them, per mail or per sender.
- Signed and encrypted mail with OpenPGP and S/MIME (off until you turn it on).

**Notifications that stay true**
- New mail shows within seconds and disappears when you read, archive or delete it anywhere else.
- Through a permanent connection, or through Google push with no relay in between.
- Per account, per folder and per receiving address, with its own sound.

**Calendar, tasks, contacts, files**
- Agenda with repeating events, invitations and tasks; your calendars can also appear in the phone's own calendar.
- Contacts and the away message.
- Your files on the server, folders shared with you, and public links on servers that offer them.
- Shared mail folders, calendars and address books from other users.

**Organised for you**
- Mail filters with a visual editor (the same format as Bulwark webmail) and a raw editor.
- An optional Home tab that points out overlapping events, open invitations, overdue tasks and dates in mail, without any AI.

**An assistant, if you want one**
- Bring your own AI service (OpenAI-compatible or Anthropic). Off by default.
- Summaries, reply ideas, to-dos and dates, a chat that can look things up in your mail, and Catch me up.
- Nothing is sent without your agreement to that service and model, and no change is made without your tap unless you switch that on for one chat.

**Also**
- Several accounts, themes and colours, server administration with an API key, Enterprise features on a licensed server.
- Updates: a weekly check against this page, a download on Wi-Fi, and Android's own prompt to install. You see what changed first.

## Privacy

No tracking, no analytics, no servers of ours. The app talks to your Stalwart server, to GitHub for the update check (which sends nothing about you) and, only if you set one up, to your AI service. Keys and passwords live in Android's keystore.

## Building

```
cargo test --workspace                                  # core and cli
cd android && ./gradlew assembleDebug                   # needs the Android NDK and cargo-ndk
cd android && ./gradlew assembleDebug -PskipNative      # Kotlin side only
```

A Rust core (`core/`) does all the work: sign-in, sync, push, notifications, sending, calendar, filters, encryption. The Kotlin and Compose app (`android/`) shows it and never talks to the server itself. `cli/` drives the core from a desktop.

- [PLAN.md](PLAN.md): decisions, architecture and milestones.
- [CHANGELOG.md](CHANGELOG.md): what changed in each release.
- [dev/README.md](dev/README.md): the local test server, the emulator and releases.

## Credits

The filter format follows [Bulwark webmail](https://github.com/bulwarkmail/webmail) and the device calendar design follows Bulwark Mobile; no code was copied. The full list of libraries and licences is in the app under Settings > About.

This is an independent project. It is not made, endorsed or supported by Stalwart Labs or the Bulwark project.

Licensed under the [MIT licence](LICENSE).
