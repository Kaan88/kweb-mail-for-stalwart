# Mainstream mobile mail apps: quality-of-life features (Gmail Android, Outlook mobile, Apple Mail iOS 18+, Spark, Fastmail)

Scope note: research was capped at about 15 tool calls. Coverage is good for triage (snooze, categories, unsubscribe, nudges). It is thin for reading, compose, search and notifications, and there is almost no direct user-sentiment evidence (no Reddit or forum thread was retrievable). Those parts are listed under Gaps rather than filled from memory. Where a search tool returned only a summary rather than the page, the source is marked "search snippet".

## Triage: snooze, mute, pin, follow-up, nudges, categories, unified inbox, unsubscribe

### Takeaway
Snooze is standard everywhere. Fastmail's server-side snooze is the only one with an open JMAP design (a `snoozed` mailbox role plus a `snoozed` Email property), but the draft is expired and Fastmail's own snooze does not work in third-party clients. Category tabs (Gmail, Apple Mail 18.2, Spark Smart Inbox, Outlook Focused/Other) and subscription managers (Gmail Manage subscriptions) are the big differentiators. Gmail does not have a true unified inbox.

### Cited Findings
- **Snooze, Fastmail (UX).** Presets are Later today (3 hours from the start of the current hour), This evening (6 PM), Tomorrow (8 AM), This weekend (Sat 8 AM), Next week (Mon 8 AM), plus custom. Snoozed mail goes to a Snoozed folder. If the device is offline when a message is due, it does not reappear until reconnect. With conversations on, a new reply can pull a snoozed thread back early. Not available in third-party clients, nor on Basic or small legacy plans. — [Fastmail help](https://www.fastmail.help/hc/en-us/articles/360058753634)
- **Snooze, server-side implementation (IETF draft).** JMAP capability `urn:ietf:params:jmap:mail:snooze`. A mailbox with role `snoozed` holds snoozed mail. The Email gets an immutable `snoozed` property (`SnoozeDetails`: `until`, optional `moveToMailboxId`, optional `setKeywords`). If the target is missing, the mail goes to the inbox on wake. IMAP exposes `\Snoozed` and SNOOZE/UID SNOOZE commands. Sieve gets a `snooze` action. This is an expired Internet-Draft, not a standard. — [draft-ietf-extra-email-snooze](https://datatracker.ietf.org/doc/html/draft-ietf-extra-email-snooze)
- **Snooze, standardised part.** RFC 9979 (Informational, May 2026) registers the `Snoozed` mailbox name attribute and notes the server may mark the returning message `$new`. It defines no JMAP snooze mechanism or interface. — [RFC 9979](https://www.rfc-editor.org/rfc/rfc9979.txt)
- **Snooze, Fastmail history/reliability.** Fastmail staff authored the snooze draft. Undo Send is built on the same delayed mechanism (send scheduled about 20 seconds ahead, undo cancels and reopens the draft). Fastmail notes that lost push is tolerable but a lost snooze or delayed send is not. — search snippet of [Fastmail Advent post](https://www.fastmail.com/blog/2023-advent-post-bron-gondwana/) and [IETF slides](https://datatracker.ietf.org/meeting/117/materials/slides-117-jmap-jmap-mail-snooze-00). Verify before quoting the 20-second figure.
- **Gmail Android: snooze, mute, nudges, Smart Reply.** Snooze hides mail until a chosen time, and "Snoozed" is in the menu. Mute archives the thread and suppresses notifications for further replies. Nudges remind you about unanswered mail and suggest follow-ups, toggled per account under "Replies and follow-ups". Smart Reply is per account. — search snippet of [Android Central](https://www.androidcentral.com/gmail-android-how-to), [9to5Google on nudges](https://9to5google.com/2022/05/26/how-to-disable-nudges-in-google-apps-like-gmail-and-google-messages/), [Stony Brook IT](https://it.stonybrook.edu/node/5928). Articles are mostly 2018 to 2022; menu names may have changed.
- **Gmail: per-sender swipe learning.** One roundup says Gmail learns per sender, so the same swipe can snooze mail from a boss and archive mail from a retailer. Single secondary source, not verified. — search snippet of [Zapier best Android email app](https://zapier.com/blog/best-android-email-app/)
- **Gmail Manage subscriptions.** Lists active subscriptions with the count of mails from each sender over the past few weeks, sorted by frequency. Tapping a sender shows their mails. One tap sends an unsubscribe request on your behalf. Rolling out on web, Android and iOS in select countries (2025). After unsubscribing it can take days for mail to stop. — [Google blog](https://blog.google/products-and-platforms/products/gmail/new-manage-subscriptions-unsubscribe/); menu placement varies per [Android Authority](https://androidauthority.com/gmail-android-manage-subscriptions-3546858) (search snippet)
- **Apple Mail categories (iOS 18.2).** Four tabs: Primary, Transactions, Updates, Promotions, plus an All Mail view in chronological order. Time-sensitive mail from a secondary category also shows in Primary. Categorisation is on-device. Digest View groups mail by sender inside a category. Users can recategorise a sender ("Categorize Sender"), but Primary mail can only be recategorised after replying. Opening a message and scrolling up shows more from that sender, and "Trash All" clears a sender. Users can switch back to List View. — [MacRumors](https://www.macrumors.com/2024/12/13/ios-18-2-mail-categories/), [9to5Mac](https://9to5mac.com/mail-app-ios-18/), [MacRumors 18.2 announcement](https://www.macrumors.com/2024/10/23/ios-18-2-mail-app-categories/) (snippet)
- **Apple Mail Priority Messages (Apple Intelligence, iOS 18.1).** Time-sensitive mail (invites, flight check-ins, cancellations) goes to the top. Also summaries in the list and a longer one on open, plus Smart Reply and Writing Tools. These need Apple Intelligence hardware. — [9to5Mac](https://9to5mac.com/mail-app-ios-18/)
- **Outlook mobile Focused/Other.** Messages are sorted into Focused and Other, and you can move a message with "Move to Focused Inbox" in the three-dot menu. — search snippet of [Microsoft support](https://support.microsoft.com/en-US/Outlook/training/focused-inbox-email-filter-in-outlook-mobile)
- **Outlook mobile: pinning and swipe "Schedule".** A roadmap item said pinned mail would be marked with a pin and filterable on mobile. Not confirmed as shipped. The swipe "Schedule" action defers a message back into the inbox, which is snooze and not send-later. — search snippet of [Neowin](https://neowin.net/news/youll-soon-be-able-to-see-pinned-emails-in-outlook-for-mobile) and [Microsoft support](https://support.microsoft.com/en-gb/office/quickly-triage-your-inbox-with-outlook-mobile-57093f70-ac12-4b3f-8b12-b347ab72cde2) (this page returned 404 on direct fetch)
- **Spark.** Smart Inbox surfaces important mail and helps clear the rest, with categories such as personal, notifications and newsletters. Snooze is available from customisable swipe gestures. In Spark Classic a snoozed mail is moved to a "Later" folder on the provider. Follow-up reminders bring the mail back to the top if there is no reply by the chosen time. Smart Notifications alert only on important mail. Smart Search takes natural language. — [Spark features](https://sparkmailapp.com/spark2/features), [Spark help: snooze](https://sparkmailapp.com/help/tips-tricks/snooze-emails), [Spark Classic follow-up help](https://support.readdle.com/spark/sending-emails/set-follow-up-reminders) (search snippets for the Later folder and follow-up behavior)
- **Unified inbox.** Spark's Smart Inbox works as a unified inbox across accounts. Gmail has no true unified inbox, which is a common reason people switch. — search snippet of [Zapier](https://zapier.com/blog/best-android-email-app/). Zapier is an editorial roundup.
- **Removed or changed (Gmail).** Gmail is ending Gmailify and POP fetching for third-party accounts, and "Send as" for third-party accounts. The timeline moved across Google's pages, with a newer page giving a Q3 to Q4 2026 transition and full removal in January 2027. The mobile app can still add non-Google accounts over IMAP. — [Google support](https://support.google.com/mail/answer/16604719), [9to5Google](https://9to5google.com/2026/01/23/gmail-gmailify-pop-shutting-down/) (snippets; I could not confirm which Google page is current)

### Inferences
- A Stalwart client can offer snooze with no server support only as client-side state, which risks the exact failure Fastmail warns about (a snoozed mail that does not come back). Whether Stalwart implements the JMAP snooze draft was not checked, so it should be tested before relying on it.
- Gmail's Manage subscriptions is a local ranking of senders by recent volume plus an unsubscribe request, so a client can build it from `List-Unsubscribe` headers and message counts.
- Apple's category tabs are computed on-device, so a client can do the same without server help. A sender-level override is the key control users get.

### Gaps
- Mute/ignore on Outlook and Fastmail, pinning on Gmail/Apple/Spark, and Apple's "Remind Me" and follow-up suggestion behavior: not verified from a primary source.
- Gmail "Priority Inbox" and tabs current mobile behavior: not fetched.
- Fastmail's app-side features other than snooze (follow-up, Undo Send timing, bulk unsubscribe): not retrieved.
- Auto-advance after archive/delete, batch actions and sender-level "all from this sender" actions across the five apps: not researched.

## Reading: summaries, cards, translate, avatars, safety banners, quick actions

### Takeaway
AI summaries are the main change: Gmail shows Gemini summary cards on long threads in the mobile app, and Apple shows summaries in the list. Little else on reading was verified.

### Cited Findings
- **Gmail summary cards.** Announced May 2025 for the Gmail mobile app. They appear above the message for long threads or threads with many replies, can be collapsed, and mail without a card still supports a user-triggered summary. A Gemini star icon on Android opens a Q&A on the inbox (find details, unread, filter by sender). — [Google Workspace Updates](https://workspaceupdates.googleblog.com/2025/05/gemini-summary-cards-gmail-app.html), [ThePrint](https://theprint.in/tech/google-introduces-gmail-qa-on-android-for-enhanced-inbox-search/2246487) (snippet)
- **Gmail summary eligibility.** eWeek (June 2026) reports a global rollout to free and paid accounts in a limited set of languages, while another version of the same article says advanced features stay limited to AI Pro and Ultra in the US. Contradictory; eligibility is unconfirmed. Turning off Workspace smart features removes summaries and other smart features. — [eWeek](https://www.eweek.com/news/google-gmail-ai-summaries-global-rollout/) (snippet)
- **Apple Mail summaries and Writing Tools.** Short summary in the list and a longer one on open (iOS 18.1, Apple Intelligence only). Writing Tools can rewrite, proofread or summarise. — [9to5Mac](https://9to5mac.com/mail-app-ios-18/)
- **Outlook suggested replies.** Mobile support is limited: when a meeting request is detected there are "Send Availability" and "Schedule Meeting" shortcuts, if the user and organisation allow it. — search snippet of [Microsoft support](https://support.microsoft.com/en-gb/office/create-and-reply-to-email-in-outlook-for-ios-and-android-ad6e109b-81c5-4b12-bcf5-4992fd67e35b)

### Inferences
- Summary features depend on a model the user has opted into. Gmail ties them to a switch that also disables other smart features, so a separate opt-in is the cleaner design.

### Gaps
- Package-tracking and flight/receipt cards, BIMI/Gravatar/brand logos, dangerous-message banners, translate, print/save PDF, "show original", copy-code, RSVP from mail, link previews: no source retrieved on any of the five apps.

## Compose: scheduled send, undo send, templates, signatures, aliases, warnings

### Takeaway
Send later, undo send, follow-up reminders, templates and per-account signatures are table stakes in Spark and Fastmail; Outlook mobile's status for send-later was not confirmed.

### Cited Findings
- **Spark.** Send Later offers Later Today, This Evening and Tomorrow, or a custom time, with defaults editable in Settings. Follow-up reminders are chosen at send time. Reply templates and swipe-to-pick signatures exist. — [Spark features](https://sparkmailapp.com/spark2/features), [Spark Classic scheduling help](https://support.readdle.com/spark/sending-emails/schedule-an-email-to-send-later) (snippet)
- **Gmail Android** includes undo send per a Zapier roundup. — [Zapier](https://zapier.com/blog/best-android-email-app/) (snippet)
- **Fastmail Undo Send** is a delayed submission, cancel reopens the draft (see triage section, same snippet-level source).
- **Outlook iOS send-later gap.** One App Store review asks for send-later on iPhone because it is missing. This is a single user review, not a current confirmation. — search snippet of [App Store listing](https://apps.apple.com/ae/app/id951937596)
- **Apple Mail send later and remind me.** A 9to5Mac page summary lists Send Later, remind me, follow-up and unsubscribe among the Mail updates of iOS 18, but the article body does not describe them. — [9to5Mac](https://9to5mac.com/mail-app-ios-18/)

### Inferences
- Because Fastmail builds Undo Send and delayed send on the same server mechanism, a client that uses JMAP `EmailSubmission` with `sendAt`-style delay (if the server supports it) gets all three behaviors consistently. Not verified on Stalwart here.

### Gaps
- Smart Compose, mentions (@name), forgotten-attachment and no-subject warnings, contact groups, drag-and-drop, large-file links, rich-text toolbar: no retrieved source.

## Search: operators, chips, saved searches, attachment content

### Takeaway
Only Spark's natural-language Smart Search and Gmail's Gemini Q&A were evidenced. Operators, chips and saved searches were not researched.

### Cited Findings
- Spark Smart Search finds mail with natural-language queries. — [Spark features](https://sparkmailapp.com/spark2/features)
- Gmail Android's Gemini Q&A can find details, unread mail, and mail from senders. — [ThePrint](https://theprint.in/tech/google-introduces-gmail-qa-on-android-for-enhanced-inbox-search/2246487) (snippet)

### Inferences
- None beyond the above.

### Gaps
- Operators, filter chips, suggested searches, attachment-content search and saved searches in all five apps were not researched.

## Notifications: per-account/folder, VIP, quiet hours, actions, smart notifications

### Takeaway
Spark's Smart Notifications (important mail only) is the clearest example of priority-only alerts found. Everything else in this area was not verified.

### Cited Findings
- Spark Smart Notifications alert only when important mail arrives. — [Spark features](https://sparkmailapp.com/spark2/features)
- Gmail muting a thread suppresses notifications for later replies. — search snippet of [Android Central](https://www.androidcentral.com/gmail-android-how-to)
- Fastmail's own note that push can lose events but snooze and delayed send must not (design context for notifications vs. timers). — snippet of [Fastmail Advent post](https://www.fastmail.com/blog/2023-advent-post-bron-gondwana/)

### Inferences
- A per-address or per-folder notification rule is something the five apps were not shown to offer in this research, so it should not be counted as a gap against the user's client.

### Gaps
- VIP lists, quiet hours, notification actions, bundling and per-folder settings for Gmail, Outlook, Apple, Spark and Fastmail: not retrieved.

## What reviews and forums most often praise or miss

### Takeaway
No Reddit, forum or Fastmail community thread could be retrieved, so this question is largely unanswered. Only editorial roundups gave signals.

### Cited Findings
- A 2026 Zapier review names Spark the best overall Android email app. Some features such as large attachments and full AI need a paid plan. — search snippet of [Zapier](https://zapier.com/blog/best-android-email-app/)
- The same roundup says the lack of a unified inbox in Gmail is a common reason people look elsewhere. — same source.
- Boxer and Newton are praised for unified inbox, snooze, undo send and send later (Newton is subscription). — same source.

### Inferences
- Snooze, undo send, send later, a unified inbox and per-sender behavior are the features roundups treat as differentiators, which suggests they are the ones users would miss first. This is based on editorial coverage, not user data.

### Gaps
- Direct user-sentiment evidence (r/android, r/GMail, r/Outlook, Spark reviews, Fastmail forums) was not found. A targeted pass on those sources is needed before claiming how much users value any feature.
