<p align="center"><img src="branding/ward/png/ward-128.png" width="96" alt=""></p>

<h1 align="center">kweb mail for Stalwart</h1>

<p align="center">A simple native Android mail client for <b>Stalwart</b> servers.<br>
Rust core, Kotlin interface, notifications that clear themselves when mail is read elsewhere.</p>

---

Built for and tested against Stalwart only. It speaks JMAP and nothing else: no IMAP, POP3 or SMTP. Other JMAP servers are not checked and may not work.

**Status: early.** The Rust core signs in, syncs, holds the push connection and decides notifications, tested against a mock server only. The Android app is a shell that shows the core's version. Nothing has run against a real Stalwart server or on a phone yet.

```
cargo test --workspace                                  # core and cli
cd android && ./gradlew assembleDebug                   # needs the Android NDK and cargo-ndk
cd android && ./gradlew assembleDebug -PskipNative      # Kotlin side only
```

- [PLAN.md](PLAN.md): decisions, architecture, milestones and the notification tests.
- [design/mockups.html](design/mockups.html): seven screens and the feature scope. Open it in a browser.
- [branding/ward](branding/ward): the Ward mark, lockups and icon sizes.

This is an independent project. It is not made, endorsed or supported by Stalwart Labs or the Bulwark project.

Licensed under the [MIT licence](LICENSE).
