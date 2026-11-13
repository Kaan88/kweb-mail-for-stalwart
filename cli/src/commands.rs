//! What each command does with the `Core`. Everything here runs on the main thread (or the
//! listener's callback thread), never inside an async runtime.

use std::{process, sync::Arc, thread};

use kweb_core::{
    ffi::{Core, Shade},
    model::{Account, Action, ConnectionStatus, MessageHeader},
    notify::Plan,
    push::Listener,
};

use crate::{args::Command, timefmt};

pub fn run(core: &Arc<Core>, command: Command) -> Result<(), String> {
    match command {
        Command::Help => Ok(()), // handled before the core is opened
        Command::SignIn { server_url } => sign_in(core, server_url),
        Command::Status => status(core),
        Command::Inbox { count } => inbox(core, count),
        Command::Mailboxes => mailboxes(core),
        Command::Watch => watch(core),
        Command::Read { id } => act(core, Action::MarkRead { message_id: id }, "marked read"),
        Command::Unread { id } => act(core, Action::MarkUnread { message_id: id }, "marked unread"),
        Command::Archive { id } => act(core, Action::Archive { message_id: id }, "archived"),
        Command::Delete { id } => act(core, Action::Delete { message_id: id }, "moved to Trash"),
        Command::Destroy { id } => act(core, Action::Destroy { message_id: id }, "destroyed"),
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
    format!("{marker} {}  {sender}  {}  [{}]", timefmt::utc(m.received_at), m.subject, m.id)
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
        | Action::Destroy { message_id } => message_id.clone(),
    };
    core.submit(action).map_err(|e| e.to_string())?;
    core.check_once(Shade::Unknown).map_err(|e| {
        format!("{id} is changed locally but not sent yet (the next kweb command or watch will send it): {e}")
    })?;
    println!("{id}: {done}.");
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

    fn calendar_alert(&self, event_id: String, alert_id: String, recurrence_id: Option<String>) {
        let again = recurrence_id.map(|r| format!(", occurrence {r}")).unwrap_or_default();
        println!("calendar alert: event {event_id}, alert {alert_id}{again}");
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

    #[test]
    fn the_address_stands_in_for_a_missing_name() {
        assert!(message_line(&header("", false)).contains("a@example.test"));
    }

    #[test]
    fn urls_that_are_not_http_are_not_opened() {
        assert!(open_browser("file:///etc/passwd").is_err());
    }
}
