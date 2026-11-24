//! The server's background tasks over `x:Task` (see `dev/notes/stalwart-admin-more.md`):
//! work the server keeps to do later (calendar reminders, report sending, DKIM rotation, account
//! deletion, ...). What matters here:
//!
//! - A task is a variant (`@type`) with a `status` (`Pending`, `Retry`, `Stalled` or `Failed`)
//!   and a `due` time. A query sorts by `due` and filters by `status` and `@type`.
//! - `due` is set by the server; a task is run again by replacing its `status` with a `Pending`
//!   one that is due in the past. A task that is being run at that moment is refused
//!   (`forbidden`, "currently being processed").
//! - A task is read without the properties of its variant that hold mail or event data (a
//!   calendar message task carries the whole invitation): only `@type`, `due`, `status` and the
//!   ids that say what it is about are asked for.
//! - Deleting a task cannot be undone. The task that finishes the deletion of an account is not
//!   deleted here (the account would be left half deleted); retrying it is allowed.

use std::collections::HashMap;

use serde_json::{json, Value};

use super::{count, set_refusal, shorten, text, time, PAGE_MAX, RETRY_NOW};
use crate::{
    ffi::{TaskPage, TaskRow, TaskState},
    jmap::{
        send::{destroyed, response, updated},
        Connection,
    },
    Error, Result,
};

const GONE: &str = "that task is no longer on the server";
const PROPERTIES: [&str; 4] = ["@type", "due", "status", "domainId"];
const KEEP_TASK: &str = "this task finishes the deletion of an account: it cannot be deleted here";
const SERVER_TASK: &str =
    "the server keeps this task for its own work (it schedules the next one): it cannot be deleted here";
const NOT_FAILED: &str = "this task has not failed: it runs at its time";

/// The kinds a person may delete: one-off work for a mailbox or a report, whose loss stops nothing that comes later.
/// Anything else (maintenance, certificates, DKIM, DNS, report periods, and kinds this does not know) stays.
fn deletable(kind: &str) -> bool {
    matches!(
        kind,
        "CalendarAlarmEmail"
            | "CalendarAlarmNotification"
            | "CalendarItipMessage"
            | "MergeThreads"
            | "DmarcReport"
            | "TlsReport"
            | "RestoreArchivedItem"
    )
}

/// The task type in words.
fn kind_words(kind: &str) -> String {
    match kind {
        "CalendarAlarmEmail" => "Calendar reminder by e-mail",
        "CalendarAlarmNotification" => "Calendar reminder",
        "CalendarItipMessage" => "Calendar invitation message",
        "MergeThreads" => "Merge mail threads",
        "DmarcReport" => "Send a DMARC report",
        "TlsReport" => "Send a TLS report",
        "RestoreArchivedItem" => "Restore a deleted item",
        "DestroyAccount" => "Delete an account",
        "AccountMaintenance" => "Account maintenance",
        "TenantMaintenance" => "Tenant maintenance",
        "StoreMaintenance" => "Storage maintenance",
        "SpamFilterMaintenance" => "Spam filter maintenance",
        "AcmeRenewal" => "Renew a certificate",
        "DkimManagement" => "Rotate DKIM keys",
        "DnsManagement" => "Update DNS records",
        "ShareRevokeCleanup" => "Clean up after a share was removed",
        "DmarcReportWindow" => "Start a DMARC report period",
        "TlsReportWindow" => "Start a TLS report period",
        "ExternalPublicKeyMaintenance" => "Refresh public keys",
        "BlobRecode" => "Re-encode stored data",
        other => return spaced(other),
    }
    .to_string()
}

/// "SomeNewKind" as "Some new kind".
fn spaced(camel: &str) -> String {
    let mut out = String::new();
    for c in camel.chars() {
        if c.is_ascii_uppercase() && !out.is_empty() {
            out.push(' ');
        }
        out.push(c.to_ascii_lowercase());
    }
    let mut chars = out.chars();
    chars.next().map(|f| f.to_uppercase().chain(chars).collect()).unwrap_or_default()
}

fn state_of(status: &Value) -> TaskState {
    match status["@type"].as_str() {
        Some("Retry") => TaskState::Retrying,
        Some("Stalled") => TaskState::Stalled,
        Some("Failed") => TaskState::Failed,
        _ => TaskState::Waiting,
    }
}

fn status_filter(state: TaskState) -> &'static str {
    match state {
        TaskState::Waiting => "Pending",
        TaskState::Retrying => "Retry",
        TaskState::Stalled => "Stalled",
        TaskState::Failed => "Failed",
    }
}

fn task_row(t: &Value, domains: &HashMap<String, String>) -> Option<TaskRow> {
    let status = &t["status"];
    let kind_code = text(t, "@type");
    let state = state_of(status);
    let attempts = match state {
        TaskState::Failed => count(&status["failedAttemptNumber"]),
        TaskState::Waiting => 0,
        _ => count(&status["attemptNumber"]),
    };
    let subject = t["domainId"].as_str().and_then(|d| domains.get(d)).cloned().unwrap_or_default();
    Some(TaskRow {
        id: t["id"].as_str()?.to_string(),
        kind: kind_words(&kind_code),
        kind_code,
        due: time(&t["due"]).or_else(|| time(&status["due"])),
        state,
        attempts,
        reason: shorten(status["failureReason"].as_str().unwrap_or_default()),
        subject,
    })
}

impl Connection {
    /// The tasks, the one due first at the top; `state` narrows them to one status.
    pub(crate) async fn task_page(&self, state: Option<TaskState>, offset: usize, limit: usize) -> Result<TaskPage> {
        let account = self.account_id();
        let mut query = json!({
            "accountId": account, "sort": [{ "property": "due", "isAscending": true }],
            "position": offset, "limit": limit.min(PAGE_MAX), "calculateTotal": true,
        });
        if let Some(state) = state {
            query["filter"] = json!({ "status": status_filter(state) });
        }
        let get = json!({
            "accountId": account, "properties": PROPERTIES,
            "#ids": { "resultOf": "c0", "name": "x:Task/query", "path": "/ids" },
        });
        let responses = self.admin_request("see the tasks", &[("x:Task/query", query), ("x:Task/get", get)]).await?;
        let answer = response(&responses, "x:Task/query")?;
        let ids: Vec<&str> = answer["ids"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
        let list = response(&responses, "x:Task/get")?["list"].as_array().cloned().unwrap_or_default();
        let domains = if list.iter().any(|t| t["domainId"].is_string()) {
            self.domain_names_for_tasks().await
        } else {
            HashMap::new()
        };
        let mut tasks: Vec<(usize, TaskRow)> = list
            .iter()
            .filter_map(|t| Some((ids.iter().position(|id| t["id"].as_str() == Some(id))?, task_row(t, &domains)?)))
            .collect();
        tasks.sort_by_key(|(place, _)| *place);
        let total = answer["total"].as_u64().map_or(tasks.len() as u32, |t| count(&json!(t)));
        Ok(TaskPage { tasks: tasks.into_iter().map(|(_, t)| t).collect(), total })
    }

    /// Domain names for the tasks' subject line; a key that may not read domains gets none.
    async fn domain_names_for_tasks(&self) -> HashMap<String, String> {
        let args = json!({ "accountId": self.account_id(), "ids": null, "properties": ["name"] });
        let Ok(responses) = self.call_keeping_errors(&[], &[("x:Domain/get", args)]).await else {
            return HashMap::new();
        };
        responses
            .iter()
            .filter(|(name, _)| name == "x:Domain/get")
            .filter_map(|(_, a)| a["list"].as_array())
            .flatten()
            .filter_map(|d| Some((d["id"].as_str()?.to_string(), text(d, "name"))))
            .collect()
    }

    /// Has the server run the task again now; only a task that failed or waits for another try (a waiting one would run
    /// before its time: a reminder sent early, keys rotated early).
    pub(crate) async fn task_retry(&self, id: &str) -> Result<()> {
        let get = json!({ "accountId": self.account_id(), "ids": [id], "properties": ["status"] });
        let responses = self.admin_request("see the tasks", &[("x:Task/get", get)]).await?;
        let list = response(&responses, "x:Task/get")?["list"].as_array().cloned().unwrap_or_default();
        match list.first() {
            None => return Err(Error::invalid(GONE)),
            Some(t) if state_of(&t["status"]) == TaskState::Waiting => return Err(Error::invalid(NOT_FAILED)),
            Some(_) => {}
        }
        let patch = json!({ "status": { "@type": "Pending", "due": RETRY_NOW } });
        let args = json!({ "accountId": self.account_id(), "update": { id: patch } });
        let responses = self.admin_request("change tasks", &[("x:Task/set", args)]).await?;
        let set = response(&responses, "x:Task/set")?;
        if let Some(e) = set["notUpdated"].get(id) {
            let detail = text(e, "description").to_lowercase();
            return Err(match e["type"].as_str() {
                Some("notFound") => Error::invalid(GONE),
                Some("forbidden") if detail.contains("being processed") => {
                    Error::invalid("the server is running that task right now: look again in a moment")
                }
                _ => set_refusal("change tasks", e),
            });
        }
        updated(set, id, GONE)
    }

    /// Deletes a one-off task (see [`deletable`]; never the one that finishes an account's deletion). One that is gone already is as good as deleted.
    pub(crate) async fn task_delete(&self, id: &str) -> Result<()> {
        let account = self.account_id();
        let get = json!({ "accountId": account, "ids": [id], "properties": ["@type"] });
        let responses = self.admin_request("see the tasks", &[("x:Task/get", get)]).await?;
        let list = response(&responses, "x:Task/get")?["list"].as_array().cloned().unwrap_or_default();
        match list.first() {
            None => return Ok(()),
            Some(t) if t["@type"] == "DestroyAccount" => return Err(Error::invalid(KEEP_TASK)),
            Some(t) if !deletable(t["@type"].as_str().unwrap_or_default()) => return Err(Error::invalid(SERVER_TASK)),
            Some(_) => {}
        }
        let args = json!({ "accountId": account, "destroy": [id] });
        let responses = self.admin_request("delete tasks", &[("x:Task/set", args)]).await?;
        let set = response(&responses, "x:Task/set")?;
        match set["notDestroyed"].get(id) {
            Some(e) if e["type"] == "forbidden" => Err(set_refusal("delete tasks", e)),
            _ => destroyed(set, id),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_task_type_has_words_and_an_unknown_one_is_spelled_out() {
        assert_eq!(kind_words("DkimManagement"), "Rotate DKIM keys");
        assert_eq!(kind_words("SomeNewKind"), "Some new kind");
    }

    #[test]
    fn a_task_row_has_its_state_attempts_and_reason() {
        let none = HashMap::new();
        let failed = json!({
            "id": "t1", "@type": "DmarcReport", "due": "2027-01-01T00:00:00Z",
            "status": { "@type": "Failed", "failedAttemptNumber": 5, "failureReason": "connection\nrefused" },
        });
        let row = task_row(&failed, &none).unwrap();
        assert_eq!((row.state, row.attempts, row.reason.as_str()), (TaskState::Failed, 5, "connection refused"));
        let retry = json!({
            "id": "t2", "@type": "TlsReport",
            "status": { "@type": "Retry", "attemptNumber": 2, "due": "2027-01-02T00:00:00Z" },
        });
        let row = task_row(&retry, &none).unwrap();
        assert_eq!((row.state, row.attempts), (TaskState::Retrying, 2));
        assert_eq!(row.due, time(&json!("2027-01-02T00:00:00Z")));
    }
}
