# Google push: the device check that still has to be finished

The emulator run of 2026-11-14 was stopped part-way (see PLAN.md for what it did show). These are the steps it was given and the audit that was to follow. Emulator `emulator-5554` with Play services, local Stalwart at http://127.0.0.1:18080, app signed in as tester and second (`dev/README.md`).

## Steps

Go through, as the tester account (second stays on the permanent connection unless a step says otherwise), and record evidence (log lines, screenshots under ~/.cache/kweb-mail/push-check/, server-side PushSubscription/get):
1. Settings: the row "Push through Google", its confirmation with the warning text (read it: is every point of the warning there and plain?), and the Delivery check screen before switching.
2. Switch on: registration with Play services, the subscription on the server (url on fcm.googleapis.com, emailPush with urgency high, the types), the verification arriving and being written back, the state "registered until ...". With second still on the connection, the service entry stays; note what it says. How long did switching on take?
3. New mail with the app on screen, in the background, swiped from recents and after "am kill": the notification appears (how many seconds after the mail was created), only once, with sound as a new one; reading the mail elsewhere (flag it $seen by JMAP) removes it; swiping it away keeps it away after the next push.
4. Switch second to Google mode too: the foreground service and its "Connected" entry must go away and stay away (wait through one 15-minute periodic check if you can, or trigger the worker; check after a reboot of the emulator too: "adb reboot", then the app must still receive without being opened). Mail for each account lands under the right account.
5. Forced idle: "adb shell dumpsys battery unplug", screen off, "adb shell dumpsys deviceidle force-idle", app process killed: a new mail must still notify within seconds; then "deviceidle unforce" and "battery reset". Note what happens to a read-elsewhere removal sent while idle (expected: late, on wake).
6. Nothing the user did waits: in Google mode, send a mail with the 10-second Undo and leave the app at once (Home): it must be sent after its 10 seconds, once (check the server); archive a mail from its notification with the app closed: done on the server within seconds; the same archive in forced idle.
7. Calendar reminder: create an event with an alert one or two minutes ahead for tester (JMAP), app closed: does the reminder arrive, once? Then the same with the app on screen (the live loop and the push both deliver it): once.
8. Failures and fallback: switch on with Play services unavailable ("adb shell pm disable-user --user 0 com.google.android.gms" may be refused on this image; if so use another way, such as airplane mode for "unreachable", and say what could not be forced): the account must end on the permanent connection with the reason shown in the Delivery check, never with no road. Destroy the subscription on the server behind the app's back: the next renew step (trigger the worker) must notice and repair or fall back.
9. Switch off: subscription destroyed on the server, unregister sent, the connection and its entry back, with no gap (send a mail during the switch). Sign-out is not to be tested (the accounts are needed).
10. The Delivery check in each state, in light and dark.

Fix what is wrong (core or Android; small and exact; a unit or mock-server test for each fix where one is possible), rebuild, install, and repeat the step. After your fixes run cargo fmt --all, cargo clippy --workspace --all-targets -- -D warnings, cargo test --workspace and dev/gradle.sh assembleDebug lintDebug testDebugUnitTest.

At the end: both accounts back on the permanent connection (the default), no PushSubscription of this app left on the server for either user, test events and mails you created removed where practical, emulator awake and on AC, the app signed in as both users.

At the end: both accounts back on the permanent connection, no PushSubscription of this app left on the server, the emulator awake and on AC.

## Audit afterwards

Audit, as a critical reviewer: (1) read every change it made (git diff, and compare with the copy /home/kaan/.cache/kweb-mail/g for what was there before the device run) and decide for each whether it is right, complete and does not weaken a rule: the notification rule has one place (a pass over the cache), sending is never twice and never lost, everything is per account, keys and message bodies never reach a log, an error or the cache, an account is never left with no road, the service entry does not come back for Google-mode accounts; (2) look for debug leftovers (extra logging of tokens, keys or bodies, test switches, disabled secure flags, widened exported components); (3) check that the manifest receivers are guarded by com.google.android.c2dm.permission.SEND and that a broadcast from another app cannot trigger more than a harmless pass; (4) check the claims "pass" in its report against its evidence: name any step where the evidence does not show what is claimed.

Fix real faults minimally (then run the four checks).
