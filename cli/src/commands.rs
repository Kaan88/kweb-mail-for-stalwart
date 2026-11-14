//! What each command does with the `Core`. Everything here runs on the main thread (or the
//! listener's callback thread), never inside an async runtime.

use std::{process, sync::Arc, thread};

use kweb_core::{
    ffi::{Core, Shade},
    model::{
        Account, Action, Address, AwayMessage, Calendar, ConnectionStatus, ContactRow, DraftKind, Identity,
        MessageBody, MessageHeader, Occurrence, Quota, Reminder,
    },
    notify::Plan,
    push::Listener,
};

use crate::{
    args::{Away, Command},
    timefmt,
};

pub fn run(core: &Arc<Core>, command: Command) -> Result<(), String> {
    match command {
        Command::Help => Ok(()), // handled before the core is opened
        Command::SignIn { server_url } => sign_in(core, server_url),
        Command::Status => status(core),
        Command::Inbox { count } => inbox(core, count),
        Command::Mailboxes => mailboxes(core),
        Command::Show { id } => show(core, id),
        Command::Search { query } => search(core, query),
        Command::Watch => watch(core),
        Command::Read { id } => act(core, Action::MarkRead { message_id: id }, "marked read"),
        Command::Unread { id } => act(core, Action::MarkUnread { message_id: id }, "marked unread"),
        Command::Archive { id } => act(core, Action::Archive { message_id: id }, "archived"),
        Command::Delete { id } => act(core, Action::Delete { message_id: id }, "moved to Trash"),
        Command::Destroy { id } => act(core, Action::Destroy { message_id: id }, "destroyed"),
        Command::Agenda { days } => agenda(core, days),
        Command::Calendars => calendars(core),
        Command::Identities => identities(core),
        Command::Quota => quota(core),
        Command::Contacts { query } => contacts(core, query),
        Command::Away(what) => away(core, what),
        Command::Send { to, subject, text } => send(core, to, subject, text),
        Command::SignOut => sign_out(core),
    }
}

fn sign_in(core: &Core, server_url: String) -> Result<(), String> {
    let url = core.begin_sign_in(server_url).map_err(|e| e.to_string())?;
    println!("Open this address to sign in:\n{url}");
    if let Err(e) = open_browser(&url) {
        println!("(could not open the browser: {e}; open the address yourself)");
    }
    println!("Waiting for the browser (up to 5 minutes)...");
    let account = core.finish_sign_in().map_err(|e| e.to_string())?;
    println!("Signed in.");
    print_account(&account);
    Ok(())
}

/// Hands the URL to the system's opener as a single argument, never through a shell: the
/// authorize URL contains `&`, which `cmd` would take as a command separator.
fn open_browser(url: &str) -> Result<(), String> {
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err("not an http(s) address".into());
    }
    let mut command = if cfg!(windows) {
        let mut c = process::Command::new("rundll32");
        c.args(["url.dll,FileProtocolHandler", url]);
        c
    } else if cfg!(target_os = "macos") {
        let mut c = process::Command::new("open");
        c.arg(url);
        c
    } else {
        let mut c = process::Command::new("xdg-open");
        c.arg(url);
        c
    };
    command
        .stdin(process::Stdio::null())
        .stdout(process::Stdio::null())
        .stderr(process::Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

fn status(core: &Core) -> Result<(), String> {
    match core.account() {
        Some(account) => {
            println!("Signed in.");
            print_account(&account);
        }
        None => println!("Not signed in. Run: kweb sign-in <server-url>"),
    }
    Ok(())
}

fn print_account(account: &Account) {
    println!("  user:      {}", account.username);
    println!("  server:    {}", account.server_url);
    println!("  account:   {}", account.id);
    println!("  signed in: {} UTC", timefmt::utc(account.signed_in_at));
}

fn inbox(core: &Core, count: u32) -> Result<(), String> {
    core.check_once(Shade::Unknown).map_err(|e| e.to_string())?;
    let boxes = core.mailboxes().map_err(|e| e.to_string())?;
    let inbox = boxes.iter().find(|m| m.role.as_deref() == Some("inbox")).ok_or("the server has no Inbox")?;
    let messages = core.messages(inbox.id.clone(), count, 0).map_err(|e| e.to_string())?;
    if messages.is_empty() {
        println!("The Inbox is empty.");
    }
    for message in &messages {
        println!("{}", message_line(message));
    }
    Ok(())
}

/// `* 2026-01-02 09:30  Sender  Subject  [id]`; the star marks unread mail. The id is there
/// so it can be pasted into `kweb read` and friends.
fn message_line(m: &MessageHeader) -> String {
    let sender = if m.from_name.is_empty() { &m.from_email } else { &m.from_name };
    let marker = if m.is_unread { '*' } else { ' ' };
    format!(
        "{marker} {}  {}  {}  [{}]",
        timefmt::utc(m.received_at),
        one_line(sender),
        one_line(&m.subject),
        one_line(&m.id)
    )
}

fn mailboxes(core: &Core) -> Result<(), String> {
    core.check_once(Shade::Unknown).map_err(|e| e.to_string())?;
    let boxes = core.mailboxes().map_err(|e| e.to_string())?;
    println!("{:<10} {:<24} {:>6} {:>7}  id", "role", "name", "total", "unread");
    for m in boxes {
        let role = m.role.as_deref().unwrap_or("-");
        println!("{:<10} {:<24} {:>6} {:>7}  {}", role, m.name, m.total_emails, m.unread_emails, m.id);
    }
    Ok(())
}

/// How many matches `kweb search` lists.
const SEARCH_LIMIT: u32 = 20;

fn show(core: &Core, id: String) -> Result<(), String> {
    // The header is only for the subject and the arrival time; the body is fetched if needed.
    let header = core.message(id.clone()).map_err(|e| e.to_string())?;
    let body = core.message_body(id).map_err(|e| e.to_string())?;
    print!("{}", message_text(header.as_ref(), &body));
    Ok(())
}

fn search(core: &Core, query: String) -> Result<(), String> {
    let result = core.search(query, None, SEARCH_LIMIT).map_err(|e| e.to_string())?;
    if result.from_cache {
        println!("(the server could not be reached: only cached mail was searched, by sender, subject and preview)");
    }
    if result.messages.is_empty() {
        println!("No matches.");
    }
    for message in &result.messages {
        println!("{}", message_line(message));
    }
    Ok(())
}

/// What `kweb show` prints. Mail is written by strangers, so control characters (a terminal
/// escape sequence) are left out of everything taken from it.
fn message_text(header: Option<&MessageHeader>, body: &MessageBody) -> String {
    let mut out = String::new();
    let mut line = |label: &str, value: String| {
        if !value.is_empty() {
            out.push_str(&format!("{label:<9}{}\n", one_line(&value)));
        }
    };
    if let Some(h) = header {
        let from = Address { name: h.from_name.clone(), email: h.from_email.clone() };
        line("From:", addresses(&[from]));
    }
    line("To:", addresses(&body.to));
    line("Cc:", addresses(&body.cc));
    let sent = body.sent_at.or(header.map(|h| h.received_at));
    line("Date:", sent.map(|t| format!("{} UTC", timefmt::utc(t))).unwrap_or_default());
    line("Subject:", header.map(|h| h.subject.clone()).unwrap_or_default());
    out.push('\n');
    match (&body.text, &body.html) {
        (Some(text), _) => {
            out.push_str(&printable(text));
            out.push('\n');
        }
        (None, Some(_)) => out.push_str("(This message has only an HTML version, which kweb does not show.)\n"),
        (None, None) => out.push_str("(This message has no text.)\n"),
    }
    if body.is_truncated {
        out.push_str("(The text is longer than kweb fetches: the end is missing.)\n");
    }
    let files: Vec<_> = body.attachments.iter().filter(|a| !a.is_inline).collect();
    if !files.is_empty() {
        out.push_str("\nAttachments:\n");
        for a in files {
            let name = if a.name.is_empty() { "(no name)" } else { &a.name };
            out.push_str(&format!("  {}  {}  {}\n", one_line(name), size_text(a.size), one_line(&a.mime_type)));
        }
    }
    out
}

/// `Name <address>`, or just the address.
fn addresses(list: &[Address]) -> String {
    let one = |a: &Address| if a.name.is_empty() { a.email.clone() } else { format!("{} <{}>", a.name, a.email) };
    list.iter().map(one).collect::<Vec<_>>().join(", ")
}

/// For a value that must stay on its line (a name, a subject): no control characters at all,
/// so mail cannot add lines of its own or steer the terminal.
fn one_line(text: &str) -> String {
    text.chars().filter(|c| !c.is_control() && !matches!(c, '\u{2028}' | '\u{2029}')).collect()
}

/// For the text of a mail: keeps new lines and tabs, drops other control characters.
fn printable(text: &str) -> String {
    text.chars().filter(|c| !c.is_control() || matches!(c, '\n' | '\t')).collect()
}

/// `512 B`, `1.5 KB`, `3.2 MB`.
fn size_text(bytes: u64) -> String {
    match bytes {
        0..1024 => format!("{bytes} B"),
        1024..1_048_576 => format!("{:.1} KB", bytes as f64 / 1024.0),
        _ => format!("{:.1} MB", bytes as f64 / 1_048_576.0),
    }
}

fn watch(core: &Core) -> Result<(), String> {
    let account = core.account().ok_or("not signed in. Run: kweb sign-in <server-url>")?;
    println!("Watching {} (Ctrl+C to stop).", account.username);
    core.start(Arc::new(Printer));
    // The core works on its own threads; Ctrl+C ends the process.
    loop {
        thread::park();
    }
}

/// Runs the action against the cache, then one pass so it reaches the server before we exit.
fn act(core: &Core, action: Action, done: &str) -> Result<(), String> {
    let id = match &action {
        Action::MarkRead { message_id }
        | Action::MarkUnread { message_id }
        | Action::Archive { message_id }
        | Action::Delete { message_id }
        | Action::Destroy { message_id }
        | Action::Flag { message_id }
        | Action::Unflag { message_id }
        | Action::Move { message_id, .. } => message_id.clone(),
    };
    core.submit(action).map_err(|e| e.to_string())?;
    core.check_once(Shade::Unknown).map_err(|e| {
        format!("{id} is changed locally but not sent yet (the next kweb command or watch will send it): {e}")
    })?;
    println!("{id}: {done}.");
    Ok(())
}

fn calendars(core: &Core) -> Result<(), String> {
    let list = core.calendars().map_err(|e| e.to_string())?;
    if list.is_empty() {
        println!("No calendars.");
    }
    for c in &list {
        println!("{}", calendar_line(c));
    }
    Ok(())
}

/// `Personal  default  shown  writable  #3a7bd5  [id]`
fn calendar_line(c: &Calendar) -> String {
    let flags = [
        if c.is_default { "default" } else { "-" },
        if c.is_visible { "shown" } else { "hidden" },
        if c.may_write { "writable" } else { "read-only" },
    ];
    let color = c.color.as_deref().map_or_else(|| "-".to_string(), one_line);
    format!("{}  {}  {}  [{}]", one_line(&c.name), flags.join("  "), color, one_line(&c.id))
}

fn agenda(core: &Core, days: u32) -> Result<(), String> {
    let zone = timefmt::zone_from_env(std::env::var("TZ").ok().as_deref());
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_err(|e| e.to_string())?;
    let (from, to) = timefmt::span(now.as_secs() as i64, days, zone);
    let agenda = core.agenda(from, to, zone.name().to_string()).map_err(|e| e.to_string())?;
    if agenda.from_cache {
        println!("(offline: this is what the cache holds, and may be incomplete)");
    }
    if agenda.entries.is_empty() {
        println!("Nothing in the next {days} days.");
    }
    let mut last_day = String::new();
    for entry in &agenda.entries {
        let day = timefmt::day(entry.starts_at, zone);
        if day != last_day {
            println!("{day}");
            last_day = day;
        }
        println!("  {}", agenda_line(entry, zone));
    }
    Ok(())
}

/// `09:00-10:30  Title  @ Place  [id]`; an all-day entry says `all day`, a task `[ ]` or `[x]`
/// and its due time. A repeating entry's id carries the occurrence after a `#`.
fn agenda_line(o: &Occurrence, zone: chrono_tz::Tz) -> String {
    let when = if o.all_day {
        "all day".to_string()
    } else if o.is_task {
        format!("{} due", timefmt::clock(o.starts_at, zone))
    } else if o.ends_at > o.starts_at {
        format!("{}-{}", timefmt::clock(o.starts_at, zone), timefmt::clock(o.ends_at, zone))
    } else {
        timefmt::clock(o.starts_at, zone)
    };
    let task = match (o.is_task, o.is_done) {
        (false, _) => "",
        (true, false) => "[ ] ",
        (true, true) => "[x] ",
    };
    let place = if o.location.is_empty() { String::new() } else { format!("  @ {}", one_line(&o.location)) };
    let id = match &o.recurrence_id {
        Some(r) => format!("{}#{}", one_line(&o.event_id), one_line(r)),
        None => one_line(&o.event_id),
    };
    format!("{when}  {task}{}{place}  [{id}]", one_line(&o.title))
}

fn identities(core: &Core) -> Result<(), String> {
    core.check_once(Shade::Unknown).map_err(|e| e.to_string())?;
    let list = core.identities().map_err(|e| e.to_string())?;
    if list.is_empty() {
        println!("The server lists no identities.");
    }
    for identity in &list {
        println!("{}", identity_line(identity));
    }
    Ok(())
}

/// `[id] Name <address>`, with a note when there is a signature.
fn identity_line(i: &Identity) -> String {
    let who = addresses(&[Address { name: i.name.clone(), email: i.email.clone() }]);
    let sig = if i.signature.is_empty() { "" } else { "  (has a signature)" };
    format!("[{}] {}{sig}", one_line(&i.id), one_line(&who))
}

fn quota(core: &Core) -> Result<(), String> {
    let list = core.quota().map_err(|e| e.to_string())?;
    if list.is_empty() {
        println!("The server reports no quota.");
    }
    for q in &list {
        println!("{}", quota_line(q));
    }
    Ok(())
}

/// `Storage: 1.5 KB of 9.8 KB`, `Messages: 7`, with "no limit" when the server sets none.
fn quota_line(q: &Quota) -> String {
    let amount = |n: u64| if q.resource == "octets" { size_text(n) } else { n.to_string() };
    match q.limit {
        Some(limit) => format!("{}: {} of {}", one_line(&q.name), amount(q.used), amount(limit)),
        None => format!("{}: {} (no limit)", one_line(&q.name), amount(q.used)),
    }
}

fn contacts(core: &Core, query: String) -> Result<(), String> {
    let list = core.contacts(query).map_err(|e| e.to_string())?;
    if list.from_cache {
        println!("(offline: this is what the cache holds, and may be out of date)");
    }
    if list.rows.is_empty() {
        println!("No contacts.");
    }
    for row in &list.rows {
        println!("{}", contact_line(row));
    }
    Ok(())
}

/// `Name  <address>  Organization  [id]`; what a contact does not have is left out.
fn contact_line(r: &ContactRow) -> String {
    let address = if r.email.is_empty() { String::new() } else { format!("  <{}>", one_line(&r.email)) };
    let organization = if r.organization.is_empty() || r.organization == r.name {
        String::new()
    } else {
        format!("  {}", one_line(&r.organization))
    };
    format!("{}{address}{organization}  [{}]", one_line(&r.name), one_line(&r.id))
}

/// Shows the away message, or switches it on (from now on, with the given text; the subject
/// stays as it is) or off (everything else stays as it is).
fn away(core: &Core, what: Away) -> Result<(), String> {
    let current = core.away_message().map_err(|e| e.to_string())?;
    let message = match what {
        Away::Show => {
            println!("{}", away_text(&current));
            return Ok(());
        }
        Away::On { text } => AwayMessage { is_enabled: true, from: None, until: None, text, ..current },
        Away::Off => AwayMessage { is_enabled: false, ..current },
    };
    core.set_away_message(message).map_err(|e| e.to_string())?;
    let now = core.away_message().map_err(|e| e.to_string())?;
    println!("{}", away_text(&now));
    Ok(())
}

/// What the away message says, a line for the state and then the subject and text.
fn away_text(m: &AwayMessage) -> String {
    let state = if m.is_enabled { "Away message: on" } else { "Away message: off" };
    let when = match (m.from, m.until) {
        (None, None) => String::new(),
        (from, until) => {
            let at = |t: Option<i64>| t.map_or("-".to_string(), |t| format!("{} UTC", timefmt::utc(t)));
            format!(" (from {} until {})", at(from), at(until))
        }
    };
    let mut out = format!("{state}{when}");
    if !m.subject.is_empty() {
        out.push_str(&format!("\n  subject: {}", one_line(&m.subject)));
    }
    if !m.text.is_empty() {
        out.push_str(&format!("\n\n{}", printable(&m.text)));
    }
    out
}

/// Writes a plain message, sends it without the undo delay and runs one pass. Says whether
/// the server took it, still has it, or sent it back.
fn send(core: &Core, to: String, subject: String, text: String) -> Result<(), String> {
    // A fresh cache does not know yet which addresses the user can send as: one check brings them.
    if core.identities().map_err(|e| e.to_string())?.is_empty() {
        core.check_once(Shade::Unknown).map_err(|e| e.to_string())?;
    }
    let mut draft = core.new_draft(DraftKind::New).map_err(|e| e.to_string())?;
    draft.to = vec![Address { name: String::new(), email: to.trim().to_string() }];
    draft.subject = subject;
    // Above the signature the identity brings.
    draft.body = format!("{text}{}", draft.body);
    let id = draft.id.clone();
    if let Err(e) = core.send(draft, 0) {
        // Nothing was queued: do not leave the draft behind.
        let _ = core.discard_draft(id);
        return Err(e.to_string());
    }
    core.check_once(Shade::Unknown)
        .map_err(|e| format!("{id} is queued but not sent yet (the next pass will send it): {e}"))?;
    if let Some(back) = core.drafts().map_err(|e| e.to_string())?.into_iter().find(|d| d.id == id) {
        return Err(format!(
            "the server refused it: {}",
            one_line(back.send_error.as_deref().unwrap_or("no reason given"))
        ));
    }
    if core.pending_sends().map_err(|e| e.to_string())?.iter().any(|p| p.draft_id == id) {
        println!("{id}: queued; the server did not take it yet.");
    } else {
        println!("{id}: sent.");
    }
    Ok(())
}

fn sign_out(core: &Core) -> Result<(), String> {
    let was_signed_in = core.account().is_some();
    core.sign_out().map_err(|e| e.to_string())?;
    println!("{}", if was_signed_in { "Signed out. The cache is wiped." } else { "Was not signed in." });
    Ok(())
}

/// Prints what the live connection reports. Callbacks must return quickly: println does.
struct Printer;

impl Listener for Printer {
    fn status(&self, status: ConnectionStatus) {
        println!("status: {}", describe_status(&status));
    }

    fn notifications(&self, plan: Plan) {
        for n in &plan.post {
            let quiet = if n.silent { " (silent)" } else { "" };
            println!("+ {}: {}{quiet}  [{}]", n.from, n.subject, n.message_id);
        }
        for c in &plan.cancel {
            println!("- {}", c.message_id);
        }
    }

    fn cache_changed(&self) {
        println!("cache changed");
    }

    fn reminder(&self, reminder: Reminder) {
        let kind = if reminder.is_task { "task" } else { "event" };
        let title = if reminder.title.is_empty() { "(no details)" } else { &reminder.title };
        let when = reminder.local_start.as_deref().unwrap_or("time unknown");
        println!("reminder: {kind} \"{title}\" at {when} [{}]", reminder.event_id);
    }
}

fn describe_status(status: &ConnectionStatus) -> String {
    match status {
        ConnectionStatus::SignedOut => "signed out".into(),
        ConnectionStatus::Connecting => "connecting".into(),
        ConnectionStatus::Live { last_event_at } => format!("live (last event {} UTC)", timefmt::utc(*last_event_at)),
        ConnectionStatus::Offline { reason } => format!("offline: {reason}"),
        ConnectionStatus::SignInNeeded => "sign-in needed: run kweb sign-in <server-url>".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header(from_name: &str, unread: bool) -> MessageHeader {
        MessageHeader {
            id: "e7".into(),
            thread_id: "t7".into(),
            mailbox_ids: vec!["mb1".into()],
            from_name: from_name.into(),
            from_email: "a@example.test".into(),
            to: Vec::new(),
            subject: "Hello".into(),
            preview: String::new(),
            received_at: 0,
            is_unread: unread,
            is_flagged: false,
            has_attachment: false,
            size: 1,
        }
    }

    #[test]
    fn a_line_shows_marker_time_sender_and_subject() {
        let line = message_line(&header("Ann", true));
        assert_eq!(line, "* 1970-01-01 00:00  Ann  Hello  [e7]");
        assert!(message_line(&header("Ann", false)).starts_with("  1970"));
    }

    fn occurrence() -> Occurrence {
        Occurrence {
            event_id: "ev1".into(),
            recurrence_id: None,
            calendar_id: "cal1".into(),
            title: "Standup".into(),
            location: String::new(),
            starts_at: 1_767_427_200, // 2026-01-03 08:00 UTC
            ends_at: 1_767_429_000,
            all_day: false,
            is_task: false,
            is_done: false,
            is_recurring: false,
            my_status: None,
        }
    }

    #[test]
    fn an_agenda_line_shows_the_time_range_in_the_zone() {
        let o = occurrence();
        assert_eq!(agenda_line(&o, chrono_tz::Tz::UTC), "08:00-08:30  Standup  [ev1]");
        assert_eq!(agenda_line(&o, chrono_tz::Tz::Europe__Berlin), "09:00-09:30  Standup  [ev1]");
    }

    #[test]
    fn an_agenda_line_for_special_entries() {
        let zone = chrono_tz::Tz::UTC;
        let all_day = Occurrence { all_day: true, ..occurrence() };
        assert_eq!(agenda_line(&all_day, zone), "all day  Standup  [ev1]");
        let task = Occurrence { is_task: true, ends_at: 1_767_427_200, location: "Desk".into(), ..occurrence() };
        assert_eq!(agenda_line(&task, zone), "08:00 due  [ ] Standup  @ Desk  [ev1]");
        let done = Occurrence { is_done: true, ..task };
        assert!(agenda_line(&done, zone).contains("[x] Standup"));
        let point = Occurrence { ends_at: 1_767_427_200, ..occurrence() };
        assert_eq!(agenda_line(&point, zone), "08:00  Standup  [ev1]");
        let nth = Occurrence { recurrence_id: Some("2026-01-03T08:00:00".into()), ..occurrence() };
        assert!(agenda_line(&nth, zone).ends_with("[ev1#2026-01-03T08:00:00]"));
    }

    #[test]
    fn an_agenda_line_cannot_carry_control_characters() {
        let o = Occurrence { title: "a\u{1b}[31mb\nc".into(), ..occurrence() };
        assert!(!agenda_line(&o, chrono_tz::Tz::UTC).contains(['\u{1b}', '\n']));
    }

    #[test]
    fn a_calendar_line_shows_its_flags() {
        let c = Calendar {
            id: "cal1".into(),
            name: "Personal".into(),
            color: Some("#3a7bd5".into()),
            is_default: true,
            is_visible: true,
            may_write: true,
        };
        assert_eq!(calendar_line(&c), "Personal  default  shown  writable  #3a7bd5  [cal1]");
        let other = Calendar { is_default: false, is_visible: false, may_write: false, color: None, ..c };
        assert_eq!(calendar_line(&other), "Personal  -  hidden  read-only  -  [cal1]");
    }

    fn body() -> MessageBody {
        MessageBody {
            message_id: "e7".into(),
            blob_id: "b".into(),
            to: vec![Address { name: "Bo".into(), email: "bo@example.test".into() }],
            cc: vec![Address { name: String::new(), email: "cy@example.test".into() }],
            bcc: Vec::new(),
            reply_to: Vec::new(),
            sent_at: Some(0),
            html: None,
            text: Some("Hello\nthere".into()),
            attachments: Vec::new(),
            is_truncated: false,
        }
    }

    fn attachment(name: &str, size: u64, is_inline: bool) -> kweb_core::model::Attachment {
        kweb_core::model::Attachment {
            blob_id: "x".into(),
            name: name.into(),
            mime_type: "application/pdf".into(),
            size,
            content_id: None,
            is_inline,
        }
    }

    #[test]
    fn show_prints_headers_then_the_text() {
        let text = message_text(Some(&header("Ann", true)), &body());
        assert_eq!(
            text,
            "From:    Ann <a@example.test>\nTo:      Bo <bo@example.test>\nCc:      cy@example.test\n\
             Date:    1970-01-01 00:00 UTC\nSubject: Hello\n\nHello\nthere\n"
        );
    }

    #[test]
    fn show_says_when_there_is_only_html_or_nothing() {
        let mut b = body();
        b.text = None;
        b.html = Some("<p>x</p>".into());
        assert!(message_text(None, &b).contains("only an HTML version"));
        b.html = None;
        assert!(message_text(None, &b).contains("no text"));
        // Without the header there is no From or Subject line, and the sent time still shows.
        assert!(!message_text(None, &b).contains("From:") && message_text(None, &b).contains("Date:"));
    }

    #[test]
    fn show_lists_files_but_not_inline_pictures_and_says_when_the_text_is_cut() {
        let mut b = body();
        b.attachments =
            vec![attachment("report.pdf", 1536, false), attachment("", 12, false), attachment("logo.png", 9, true)];
        b.is_truncated = true;
        let text = message_text(None, &b);
        assert!(text.contains("Attachments:\n  report.pdf  1.5 KB  application/pdf\n  (no name)  12 B  "));
        assert!(!text.contains("logo.png"));
        assert!(text.contains("the end is missing"));
    }

    #[test]
    fn terminal_escapes_in_mail_are_not_printed() {
        let mut b = body();
        b.text = Some("a\u{1b}[31mred\tb\r".into());
        let text = message_text(Some(&header("An\u{7}n", true)), &b);
        assert!(!text.contains('\u{1b}') && !text.contains('\u{7}') && !text.contains('\r'));
        assert!(text.contains("a[31mred\tb"));
    }

    #[test]
    fn sizes_are_short() {
        assert_eq!(size_text(0), "0 B");
        assert_eq!(size_text(1023), "1023 B");
        assert_eq!(size_text(1024), "1.0 KB");
        assert_eq!(size_text(5 * 1_048_576), "5.0 MB");
    }

    #[test]
    fn the_address_stands_in_for_a_missing_name() {
        assert!(message_line(&header("", false)).contains("a@example.test"));
    }

    #[test]
    fn identity_lines_show_the_address_and_a_signature_note() {
        let mut i = Identity {
            id: "i1".into(),
            name: "Alice".into(),
            email: "a@example.test".into(),
            signature: String::new(),
        };
        assert_eq!(identity_line(&i), "[i1] Alice <a@example.test>");
        i.signature = "Alice".into();
        i.name.clear();
        assert_eq!(identity_line(&i), "[i1] a@example.test  (has a signature)");
    }

    #[test]
    fn quota_lines_use_bytes_for_octets_and_plain_counts_otherwise() {
        let q = |resource: &str, used, limit| Quota { name: "Q".into(), resource: resource.into(), used, limit };
        assert_eq!(quota_line(&q("octets", 1536, Some(10_240))), "Q: 1.5 KB of 10.0 KB");
        assert_eq!(quota_line(&q("octets", 5, None)), "Q: 5 B (no limit)");
        assert_eq!(quota_line(&q("count", 7, None)), "Q: 7 (no limit)");
        assert_eq!(quota_line(&q("count", 7, Some(100))), "Q: 7 of 100");
    }

    #[test]
    fn contact_lines_leave_out_what_a_contact_does_not_have() {
        let row = |name: &str, email: &str, organization: &str| ContactRow {
            id: "c1".into(),
            name: name.into(),
            email: email.into(),
            organization: organization.into(),
        };
        assert_eq!(contact_line(&row("Ann Lee", "ann@x.test", "Acme")), "Ann Lee  <ann@x.test>  Acme  [c1]");
        assert_eq!(contact_line(&row("Ann Lee", "", "")), "Ann Lee  [c1]");
        // A company is named by its organization: it is not said twice.
        assert_eq!(contact_line(&row("Kweb", "info@kweb.test", "Kweb")), "Kweb  <info@kweb.test>  [c1]");
        assert_eq!(contact_line(&row("A\u{1b}[31mB", "", "")), "A[31mB  [c1]");
    }

    #[test]
    fn the_away_message_is_shown_with_its_dates_subject_and_text() {
        let m = AwayMessage {
            is_enabled: true,
            from: Some(1_796_083_200),
            until: None,
            subject: "Away".into(),
            text: "Back soon.\nAnn".into(),
        };
        assert_eq!(
            away_text(&m),
            "Away message: on (from 2026-12-01 00:00 UTC until -)\n  subject: Away\n\nBack soon.\nAnn"
        );
        let off =
            AwayMessage { is_enabled: false, from: None, until: None, subject: String::new(), text: String::new() };
        assert_eq!(away_text(&off), "Away message: off");
    }

    #[test]
    fn urls_that_are_not_http_are_not_opened() {
        assert!(open_browser("file:///etc/passwd").is_err());
    }
}
