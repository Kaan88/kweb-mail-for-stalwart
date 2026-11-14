//! `kweb admin ...`: the admin features every server has, read with the admin API key the
//! account has stored (see `admin_connect`). The key is put in with `kweb admin connect`, from
//! `$KWEB_ADMIN_KEY`, never as an argument that would stay in the shell history.

use std::sync::Arc;

use kweb_core::ffi::{AdminAccess, BlockedAddress, Core, LogLine, QueuedMessage, ServerAccount, ServerAccountKind};

use crate::timefmt;

/// How many lines a list shows.
const SHOWN: u32 = 20;

/// What `kweb admin` does.
#[derive(Debug, Clone, PartialEq)]
pub enum Admin {
    Status,
    /// Stores the key in `$KWEB_ADMIN_KEY`.
    Connect,
    Forget,
    Queue,
    Accounts {
        text: String,
    },
    Blocked,
    Logs {
        text: String,
    },
}

/// `rest` is what follows `admin`.
pub fn parse(rest: &[String]) -> Result<Admin, String> {
    let words = || rest[1..].join(" ");
    let none = |admin: Admin| match rest.get(1) {
        None => Ok(admin),
        Some(extra) => Err(format!("unexpected argument '{extra}'")),
    };
    match rest.first().map(String::as_str) {
        None | Some("status") => none(Admin::Status),
        Some("connect") => none(Admin::Connect),
        Some("forget") => none(Admin::Forget),
        Some("queue") => none(Admin::Queue),
        Some("blocked") => none(Admin::Blocked),
        Some("accounts") => Ok(Admin::Accounts { text: words() }),
        Some("logs") => Ok(Admin::Logs { text: words() }),
        Some(other) => Err(format!("unknown admin command '{other}'")),
    }
}

pub fn run(core: &Arc<Core>, what: Admin) -> Result<(), String> {
    let failed = |e: kweb_core::Error| e.to_string();
    match what {
        Admin::Status => {
            let access = core.admin_access().map_err(failed)?;
            println!("{}", access_text(&access));
            if access.connected {
                let summary = core.admin_summary().map_err(failed)?;
                let figure = |n: Option<u32>| n.map_or("not allowed".to_string(), |n| n.to_string());
                println!("Queue: {}", figure(summary.queue_waiting));
                println!("Accounts: {}", figure(summary.accounts));
                println!("Blocked addresses: {}", figure(summary.blocked));
            }
            Ok(())
        }
        Admin::Connect => {
            let key = std::env::var("KWEB_ADMIN_KEY").map_err(|_| "set $KWEB_ADMIN_KEY to the admin API key first")?;
            let access = core.admin_connect(key).map_err(failed)?;
            println!("The key is stored.\n{}", access_text(&access));
            Ok(())
        }
        Admin::Forget => {
            core.admin_disconnect().map_err(failed)?;
            println!("The admin key is forgotten on this device.");
            Ok(())
        }
        Admin::Queue => {
            let page = core.admin_queue(0, SHOWN).map_err(failed)?;
            println!("{} waiting.", page.total);
            for m in &page.messages {
                println!("{}", queue_line(m));
            }
            Ok(())
        }
        Admin::Accounts { text } => {
            let page = core.admin_accounts(text, 0, SHOWN).map_err(failed)?;
            println!("{} found.", page.total);
            for a in &page.accounts {
                println!("{}", account_line(a));
            }
            Ok(())
        }
        Admin::Blocked => {
            let blocked = core.admin_blocked(0, SHOWN).map_err(failed)?;
            if blocked.is_empty() {
                println!("No address is blocked.");
            }
            for b in &blocked {
                println!("{}", blocked_line(b));
            }
            Ok(())
        }
        Admin::Logs { text } => {
            for line in core.admin_logs(text, SHOWN).map_err(failed)? {
                println!("{}", log_line(&line));
            }
            Ok(())
        }
    }
}

/// No control characters: a name or a log line from the server cannot steer the terminal.
fn clean(text: &str) -> String {
    text.chars().filter(|c| !c.is_control()).collect()
}

fn when(at: Option<i64>) -> String {
    at.map_or("-".to_string(), timefmt::utc)
}

fn access_text(a: &AdminAccess) -> String {
    if a.key_refused {
        return "The stored admin key is refused by the server.".into();
    }
    if !a.connected {
        return "No admin key is stored.".into();
    }
    let areas = [
        (a.may_queue, "queue"),
        (a.may_accounts, "accounts"),
        (a.may_blocked, "blocked"),
        (a.may_logs, "logs"),
        (a.may_maintenance, "maintenance"),
        (a.may_dashboard, "dashboard"),
        (a.may_history, "history"),
        (a.may_recovery, "recovery"),
        (a.may_alerts, "alerts"),
        (a.may_tenants, "tenants"),
    ];
    let open: Vec<&str> = areas.iter().filter(|(on, _)| *on).map(|(_, name)| *name).collect();
    format!("Admin key stored ({} edition). Opens: {}.", clean(&a.edition), open.join(", "))
}

fn queue_line(m: &QueuedMessage) -> String {
    let to: Vec<String> = m.recipients.iter().map(|r| format!("{} ({})", clean(&r.address), r.status)).collect();
    let mut line = format!(
        "{}  from {}  to {}  {} B  tries {}  next {}",
        clean(&m.id),
        clean(&m.from),
        to.join(", "),
        m.size,
        m.tries,
        when(m.next_try_at)
    );
    if !m.error.is_empty() {
        line.push_str(&format!("  error: {}", clean(&m.error)));
    }
    line
}

fn account_line(a: &ServerAccount) -> String {
    let kind = match a.kind {
        ServerAccountKind::User => "user",
        ServerAccountKind::Group => "group",
    };
    let mut flags = Vec::new();
    if a.is_admin {
        flags.push("admin");
    }
    if a.is_suspended {
        flags.push("suspended");
    }
    let flags = if flags.is_empty() { String::new() } else { format!("  [{}]", flags.join(", ")) };
    format!("{}  {}  {}  {}{}", clean(&a.id), kind, clean(&a.email), clean(&a.description), flags)
}

fn blocked_line(b: &BlockedAddress) -> String {
    format!("{}  {}  since {}  until {}", clean(&b.id), clean(&b.address), when(b.blocked_at), when(b.expires_at))
}

fn log_line(l: &LogLine) -> String {
    format!("{}  {:<7}  {}", when(l.at), clean(&l.level), clean(&l.text))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(line: &str) -> Vec<String> {
        line.split_whitespace().map(str::to_string).collect()
    }

    #[test]
    fn the_admin_commands_are_read() {
        assert_eq!(parse(&words("")), Ok(Admin::Status));
        assert_eq!(parse(&words("status")), Ok(Admin::Status));
        assert_eq!(parse(&words("queue")), Ok(Admin::Queue));
        assert_eq!(parse(&words("blocked")), Ok(Admin::Blocked));
        assert_eq!(parse(&words("accounts")), Ok(Admin::Accounts { text: String::new() }));
        assert_eq!(parse(&words("accounts ann lee")), Ok(Admin::Accounts { text: "ann lee".into() }));
        assert_eq!(parse(&words("logs auth failed")), Ok(Admin::Logs { text: "auth failed".into() }));
        assert_eq!(parse(&words("connect")), Ok(Admin::Connect));
        assert!(parse(&words("queue now")).is_err());
        assert!(parse(&words("status extra")).is_err());
        assert!(parse(&words("explode")).is_err());
    }

    #[test]
    fn a_line_cannot_carry_control_characters() {
        let line = log_line(&LogLine { at: Some(0), level: "info".into(), text: "a\u{1b}[31mb\nc".into() });
        assert!(!line.contains('\u{1b}') && !line.contains('\n'));
    }
}
