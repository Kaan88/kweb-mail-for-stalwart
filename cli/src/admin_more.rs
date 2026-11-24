//! `kweb admin domains|domain|domain-add|domain-on|domain-off|domain-delete|reports|report|
//! report-delete|tasks|task-retry|task-delete|cluster`: the open-edition areas added after the
//! first ones. They need the account's admin key in the secret store; this file only parses the
//! words and prints what the core gives.

use kweb_core::ffi::{ClusterNode, Core, ReportFact, ReportKind, TaskRow, TaskState};

use crate::timefmt;

#[derive(Debug, Clone, PartialEq)]
pub enum More {
    Domains,
    Domain { id: String },
    DomainAdd { name: String },
    DomainEnable { id: String, enabled: bool },
    DomainDelete { id: String },
    Reports { kind: ReportKind },
    Report { kind: ReportKind, id: String },
    ReportDelete { kind: ReportKind, id: String },
    Tasks { state: Option<TaskState> },
    TaskRetry { id: String },
    TaskDelete { id: String },
    Cluster,
}

/// Whether `kweb admin <rest>` is one of these commands.
pub fn owns(rest: &[String]) -> bool {
    matches!(
        rest.first().map(String::as_str),
        Some(
            "domains"
                | "domain"
                | "domain-add"
                | "domain-on"
                | "domain-off"
                | "domain-delete"
                | "reports"
                | "report"
                | "report-delete"
                | "tasks"
                | "task-retry"
                | "task-delete"
                | "cluster"
        )
    )
}

fn kind(word: &str) -> Result<ReportKind, String> {
    match word {
        "dmarc" => Ok(ReportKind::Dmarc),
        "tls" => Ok(ReportKind::Tls),
        "arf" => Ok(ReportKind::Arf),
        other => Err(format!("not a report kind (dmarc, tls or arf): {other}")),
    }
}

fn state(word: &str) -> Result<TaskState, String> {
    match word {
        "waiting" => Ok(TaskState::Waiting),
        "retrying" => Ok(TaskState::Retrying),
        "stalled" => Ok(TaskState::Stalled),
        "failed" => Ok(TaskState::Failed),
        other => Err(format!("not a task state (waiting, retrying, stalled or failed): {other}")),
    }
}

pub fn parse(rest: &[String]) -> Result<More, String> {
    let (name, args) = rest.split_first().ok_or("admin needs a command")?;
    let one = |what: &str| match args {
        [value] => Ok(value.clone()),
        _ => Err(format!("kweb admin {name} needs one {what}")),
    };
    let none = |c: More| if args.is_empty() { Ok(c) } else { Err(format!("kweb admin {name} takes no argument")) };
    match name.as_str() {
        "domains" => none(More::Domains),
        "domain" => Ok(More::Domain { id: one("domain id")? }),
        "domain-add" => Ok(More::DomainAdd { name: one("domain name")? }),
        "domain-on" => Ok(More::DomainEnable { id: one("domain id")?, enabled: true }),
        "domain-off" => Ok(More::DomainEnable { id: one("domain id")?, enabled: false }),
        "domain-delete" => Ok(More::DomainDelete { id: one("domain id")? }),
        "reports" => match args {
            [] => Ok(More::Reports { kind: ReportKind::Dmarc }),
            [k] => Ok(More::Reports { kind: kind(k)? }),
            _ => Err("kweb admin reports takes one kind: dmarc, tls or arf".into()),
        },
        "report" | "report-delete" => match args {
            [k, id] if name == "report" => Ok(More::Report { kind: kind(k)?, id: id.clone() }),
            [k, id] => Ok(More::ReportDelete { kind: kind(k)?, id: id.clone() }),
            _ => Err(format!("kweb admin {name} needs a kind (dmarc, tls or arf) and a report id")),
        },
        "tasks" => match args {
            [] => Ok(More::Tasks { state: None }),
            [s] => Ok(More::Tasks { state: Some(state(s)?) }),
            _ => Err("kweb admin tasks takes one state: waiting, retrying, stalled or failed".into()),
        },
        "task-retry" => Ok(More::TaskRetry { id: one("task id")? }),
        "task-delete" => Ok(More::TaskDelete { id: one("task id")? }),
        "cluster" => none(More::Cluster),
        other => Err(format!("unknown admin command: {other}")),
    }
}

/// No control characters: a name or a text from the server cannot steer the terminal.
fn clean(text: &str) -> String {
    text.chars().filter(|c| !c.is_control() || *c == '\n').collect()
}

fn when(at: Option<i64>) -> String {
    at.map_or("-".to_string(), timefmt::utc)
}

fn facts(facts: &[ReportFact], indent: &str) {
    for f in facts {
        let mut lines = clean(&f.value);
        lines = lines.replace('\n', &format!("\n{indent}    "));
        println!("{indent}{}: {lines}", clean(&f.label));
    }
}

fn state_word(s: TaskState) -> &'static str {
    match s {
        TaskState::Waiting => "waiting",
        TaskState::Retrying => "retrying",
        TaskState::Stalled => "stalled",
        TaskState::Failed => "failed",
    }
}

fn task_line(t: &TaskRow) -> String {
    let mut line = format!("{}  {:<8}  {}  due {}", t.id, state_word(t.state), clean(&t.kind), when(t.due));
    if !t.subject.is_empty() {
        line.push_str(&format!("  {}", clean(&t.subject)));
    }
    if t.attempts > 0 {
        line.push_str(&format!("  tried {}", t.attempts));
    }
    if !t.reason.is_empty() {
        line.push_str(&format!("  {}", clean(&t.reason)));
    }
    line
}

fn node_line(n: &ClusterNode) -> String {
    format!(
        "{}  node {}  {}  {}  renewed {}",
        n.id,
        n.node_id,
        clean(&n.hostname),
        clean(&n.status),
        when(n.last_renewal)
    )
}

pub fn run(core: &Core, what: More) -> Result<(), String> {
    let fail = |e: kweb_core::Error| e.to_string();
    match what {
        More::Domains => {
            for d in core.admin_domain_rows().map_err(fail)? {
                let aliases =
                    if d.aliases.is_empty() { String::new() } else { format!("  aliases {}", d.aliases.join(", ")) };
                println!("{}  {}  {}{}", d.id, clean(&d.name), if d.enabled { "on" } else { "off" }, clean(&aliases));
            }
        }
        More::Domain { id } => {
            let d = core.admin_domain(id).map_err(fail)?;
            println!("{}  {}  {}", d.id, clean(&d.name), if d.enabled { "on" } else { "off" });
            println!("DNS records to publish:\n{}", clean(&d.dns_zone));
            if !d.dkim_allowed {
                println!("DKIM keys: not allowed for this key");
            }
            for k in &d.dkim {
                println!(
                    "DKIM {}  {}  {}  next change {}",
                    clean(&k.selector),
                    clean(&k.algorithm),
                    clean(&k.stage),
                    when(k.next_change_at)
                );
            }
        }
        More::DomainAdd { name } => {
            let d = core.admin_domain_add(name).map_err(fail)?;
            println!("Added {} ({}).", clean(&d.name), d.id);
        }
        More::DomainEnable { id, enabled } => {
            core.admin_domain_set_enabled(id, enabled).map_err(fail)?;
            println!("Mail for the domain is {}.", if enabled { "on" } else { "off" });
        }
        More::DomainDelete { id } => {
            core.admin_domain_delete(id).map_err(fail)?;
            println!("The domain is deleted.");
        }
        More::Reports { kind } => {
            let page = core.admin_reports(kind, 0, 20).map_err(fail)?;
            for r in &page.reports {
                let figures = match (r.passed, r.failed) {
                    (Some(p), Some(f)) => format!("  {p} ok, {f} failed"),
                    (None, Some(f)) => format!("  {f} incidents"),
                    _ => String::new(),
                };
                println!(
                    "{}  {}  {}  {}{}  {}",
                    r.id,
                    when(r.received_at),
                    clean(&r.from),
                    clean(&r.domain),
                    figures,
                    clean(&r.note)
                );
            }
            println!("{} of {}", page.reports.len(), page.total);
        }
        More::Report { kind, id } => {
            let d = core.admin_report(kind, id).map_err(fail)?;
            println!("{}  from {}  received {}", clean(&d.subject), clean(&d.from), when(d.received_at));
            if d.period_start.is_some() {
                println!("covers {} to {}", when(d.period_start), when(d.period_end));
            }
            facts(&d.summary, "");
            for g in &d.groups {
                println!("\n{}", clean(&g.title));
                facts(&g.facts, "  ");
            }
        }
        More::ReportDelete { kind, id } => {
            core.admin_report_delete(kind, id).map_err(fail)?;
            println!("The report is deleted.");
        }
        More::Tasks { state } => {
            let page = core.admin_tasks(state, 0, 50).map_err(fail)?;
            for t in &page.tasks {
                println!("{}", task_line(t));
            }
            println!("{} of {}", page.tasks.len(), page.total);
        }
        More::TaskRetry { id } => {
            core.admin_task_retry(id).map_err(fail)?;
            println!("The server will run the task now.");
        }
        More::TaskDelete { id } => {
            core.admin_task_delete(id).map_err(fail)?;
            println!("The task is deleted.");
        }
        More::Cluster => {
            for n in core.admin_cluster().map_err(fail)? {
                println!("{}", node_line(&n));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(words: &[&str]) -> Result<More, String> {
        parse(&words.iter().map(|w| w.to_string()).collect::<Vec<_>>())
    }

    #[test]
    fn the_commands_are_read() {
        assert_eq!(p(&["domains"]), Ok(More::Domains));
        assert_eq!(p(&["domain-add", "example.org"]), Ok(More::DomainAdd { name: "example.org".into() }));
        assert_eq!(p(&["domain-off", "b"]), Ok(More::DomainEnable { id: "b".into(), enabled: false }));
        assert_eq!(p(&["reports"]), Ok(More::Reports { kind: ReportKind::Dmarc }));
        assert_eq!(p(&["reports", "tls"]), Ok(More::Reports { kind: ReportKind::Tls }));
        assert_eq!(p(&["report", "arf", "r1"]), Ok(More::Report { kind: ReportKind::Arf, id: "r1".into() }));
        assert_eq!(
            p(&["report-delete", "dmarc", "r1"]),
            Ok(More::ReportDelete { kind: ReportKind::Dmarc, id: "r1".into() })
        );
        assert_eq!(p(&["tasks", "failed"]), Ok(More::Tasks { state: Some(TaskState::Failed) }));
        assert_eq!(p(&["task-retry", "t1"]), Ok(More::TaskRetry { id: "t1".into() }));
        assert_eq!(p(&["cluster"]), Ok(More::Cluster));
    }

    #[test]
    fn a_wrong_command_is_refused() {
        assert!(p(&["domains", "x"]).is_err());
        assert!(p(&["domain"]).is_err());
        assert!(p(&["reports", "spam"]).is_err());
        assert!(p(&["report", "dmarc"]).is_err());
        assert!(p(&["tasks", "stuck"]).is_err());
        assert!(owns(&["tasks".to_string()]) && !owns(&["queue".to_string()]));
    }

    #[test]
    fn a_text_from_the_server_cannot_steer_the_terminal() {
        assert_eq!(clean("a\u{1b}[31mb\nc"), "a[31mb\nc");
    }
}
