//! `kweb admin history|deliveries|delivery|deleted|alerts|tenants`: the reads of what a
//! licensed server adds. They need the account's admin key in the secret store; this file only
//! parses the words and prints what the core gives.

use kweb_core::ffi::{Core, DeletedKind, DeliveryStatus};

use crate::timefmt;

#[derive(Debug, Clone, PartialEq)]
pub enum Enterprise {
    History { hours: u32 },
    Deliveries { text: String },
    Delivery { id: String },
    Deleted,
    Alerts,
    Tenants,
}

/// Whether `kweb admin <rest>` is one of these commands, so another `admin` command may take
/// the rest.
pub fn owns(rest: &[String]) -> bool {
    matches!(
        rest.first().map(String::as_str),
        Some("history" | "deliveries" | "delivery" | "deleted" | "alerts" | "tenants")
    )
}

pub fn parse(rest: &[String]) -> Result<Enterprise, String> {
    let (name, args) = rest.split_first().ok_or("admin needs a command")?;
    let none =
        |c: Enterprise| if args.is_empty() { Ok(c) } else { Err(format!("kweb admin {name} takes no argument")) };
    match name.as_str() {
        "history" => match args {
            [] => Ok(Enterprise::History { hours: 24 }),
            [h] => {
                h.parse().map(|hours| Enterprise::History { hours }).map_err(|_| format!("not a number of hours: {h}"))
            }
            _ => Err("kweb admin history takes one number of hours".into()),
        },
        "deliveries" => Ok(Enterprise::Deliveries { text: args.join(" ") }),
        "delivery" => match args {
            [id] => Ok(Enterprise::Delivery { id: id.clone() }),
            _ => Err("kweb admin delivery needs one delivery id".into()),
        },
        "deleted" => none(Enterprise::Deleted),
        "alerts" => none(Enterprise::Alerts),
        "tenants" => none(Enterprise::Tenants),
        other => Err(format!("unknown admin command: {other}")),
    }
}

fn status(s: DeliveryStatus) -> &'static str {
    match s {
        DeliveryStatus::Delivered => "delivered",
        DeliveryStatus::Retrying => "retrying",
        DeliveryStatus::Failed => "failed",
        DeliveryStatus::Unknown => "unknown",
    }
}

pub fn run(core: &Core, what: Enterprise) -> Result<(), String> {
    let fail = |e: kweb_core::Error| e.to_string();
    match what {
        Enterprise::History { hours } => {
            let h = core.admin_history(hours).map_err(fail)?;
            println!("{:<18} {:>9} {:>9} {:>10}", "from (UTC)", "received", "delivered", "avg secs");
            for ((r, d), t) in h.received.iter().zip(&h.delivered).zip(&h.delivery_seconds) {
                println!("{:<18} {:>9} {:>9} {:>10.2}", timefmt::utc(r.at), r.value, d.value, t.value);
            }
        }
        Enterprise::Deliveries { text } => {
            let page = core.admin_deliveries(text, None, 0, 50).map_err(fail)?;
            for d in &page.deliveries {
                println!(
                    "{}  {}  {:<9} {} -> {}  ({} bytes)",
                    d.id,
                    timefmt::utc(d.at),
                    status(d.status),
                    d.from,
                    d.to.join(", "),
                    d.size
                );
            }
            println!("{} of {}", page.deliveries.len(), page.total);
        }
        Enterprise::Delivery { id } => {
            let d = core.admin_delivery(id).map_err(fail)?;
            println!("{} -> {}  {}", d.delivery.from, d.delivery.to.join(", "), status(d.delivery.status));
            for s in d.steps {
                let more = if s.detail.is_empty() { String::new() } else { format!(": {}", s.detail) };
                println!("  {}  {}{more}", timefmt::utc(s.at), s.title);
            }
        }
        Enterprise::Deleted => {
            let state = core.admin_recovery_state().map_err(fail)?;
            match (state.is_on, state.keeps_days) {
                (true, Some(d)) => println!("the server keeps deleted items for {d} days"),
                (true, None) => println!("the server keeps deleted items"),
                _ => println!("the server does not keep deleted items"),
            }
            let page = core.admin_deleted_items(None, 0, 50).map_err(fail)?;
            for i in &page.items {
                let kind = match i.kind {
                    DeletedKind::Mail => "mail",
                    DeletedKind::Event => "event",
                    DeletedKind::Contact => "contact",
                    DeletedKind::File => "file",
                    DeletedKind::Other => "other",
                };
                let when = i.deleted_at.map(timefmt::utc).unwrap_or_default();
                println!("{}  {:<7} {}  {}", i.id, kind, when, i.title);
            }
            println!("{} of {}", page.items.len(), page.total);
        }
        Enterprise::Alerts => {
            for a in core.admin_alerts().map_err(fail)? {
                println!("{}  {}  {}\n    {}", a.id, if a.is_enabled { "on " } else { "off" }, a.name, a.summary);
            }
        }
        Enterprise::Tenants => {
            for t in core.admin_tenants().map_err(fail)? {
                let used = t.used.map_or("?".to_string(), |u| u.to_string());
                let quota = t.quota.map_or("no limit".to_string(), |q| q.to_string());
                println!("{}  {}  used {used} of {quota} bytes", t.id, t.name);
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(words: &[&str]) -> Result<Enterprise, String> {
        parse(&words.iter().map(|w| w.to_string()).collect::<Vec<_>>())
    }

    #[test]
    fn parses() {
        assert_eq!(p(&["history"]), Ok(Enterprise::History { hours: 24 }));
        assert_eq!(p(&["history", "168"]), Ok(Enterprise::History { hours: 168 }));
        assert!(p(&["history", "x"]).is_err());
        assert_eq!(p(&["deliveries", "a", "b"]), Ok(Enterprise::Deliveries { text: "a b".into() }));
        assert!(p(&["delivery"]).is_err());
        assert_eq!(p(&["alerts"]), Ok(Enterprise::Alerts));
        assert!(p(&["tenants", "x"]).is_err());
    }
}
