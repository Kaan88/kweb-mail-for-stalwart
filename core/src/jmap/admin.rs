//! The server's management calls (`x:<Object>/get|query|set`, JMAP with a capability of
//! Stalwart's own) as raw requests, for the admin functions of `ffi/admin.rs`. The
//! connection is the one made with the account's admin API key (see `ffi/admin_link.rs`).
//! What Stalwart 1.1.0 really does is in `dev/notes/stalwart-admin-access.md` and
//! `stalwart-admin-api-local.md`; the points that shape this code:
//!
//! - `GET /api/account` gives the permission names and the edition. A call a key may not make
//!   is a method error `forbidden` (HTTP 200); an Enterprise object on a server without a
//!   licence is the same type with a text that says so, so the text is what tells them apart.
//! - A queue query sorts only by `due`. A log query filters only by `text` and sorts only by
//!   id, which is not time order across the log files; a blocked address has no sort at all.
//!   Those two are read in a window and sorted here.
//! - An account query matches whole words only, so a search is made here over a list.
//! - Retrying a message is moving its `nextRetry` far into the past; cancelling it is destroying it.
//! - Nothing here is cached.

mod accounts;

use std::collections::HashSet;

use chrono::DateTime;
use serde_json::{json, Value};

use super::{
    errors::unauthorized,
    send::{destroyed, response, set_error, updated},
    Connection,
};
use crate::{
    ffi::{AdminAccess, AdminSummary, BlockedAddress, LogLine, Maintenance, QueuePage, QueuedMessage, QueuedRecipient},
    Error, Result,
};

pub(super) const NEEDS_LICENCE: &str = "this needs an Enterprise licence on the server";
const ENTERPRISE_TEXT: &str = "only available in the Enterprise edition";
/// Not found as the message is gone, in words for the user.
const MESSAGE_GONE: &str = "that message is no longer in the queue";
/// A retry is a due time in the past; the web UI of the server uses this one.
const RETRY_NOW: &str = "2000-01-01T00:00:00Z";
/// What one page of a list holds at most.
pub(super) const PAGE_MAX: usize = 100;
const LOG_PAGE_MAX: usize = 200;
/// How many of the log's lines are read to find the newest: the server's order is by file.
const LOG_WINDOW: usize = 1000;
/// The most ids a list is read to the end for (the server never gives more).
pub(super) const LIST_MAX: usize = 5000;
/// How much of what the other server said is kept.
const DETAIL_MAX: usize = 200;

/// What `GET /api/account` says about the key.
pub(crate) struct AdminInfo {
    pub edition: String,
    permissions: HashSet<String>,
}

impl AdminInfo {
    fn has(&self, permission: &str) -> bool {
        self.permissions.contains(permission)
    }

    fn has_all(&self, permissions: &[&str]) -> bool {
        permissions.iter().all(|p| self.has(p))
    }

    /// What the key opens. The Enterprise areas need the licence and the read permission.
    pub(crate) fn access(&self) -> AdminAccess {
        let enterprise = self.edition == "enterprise";
        // An area is offered exactly when its first screen can work: the Enterprise functions
        // (`ffi/enterprise.rs`) ask for these same permissions.
        let reads = |permissions: &[&str]| enterprise && self.has_all(permissions);
        let actions =
            ["actionReloadSettings", "actionReloadTlsCertificates", "actionPauseMtaQueue", "actionResumeMtaQueue"];
        AdminAccess {
            connected: true,
            key_refused: false,
            edition: self.edition.clone(),
            may_queue: self.has_all(&["sysQueuedMessageQuery", "sysQueuedMessageGet"]),
            may_accounts: self.has_all(&["sysAccountQuery", "sysAccountGet"]),
            may_blocked: self.has_all(&["sysBlockedIpQuery", "sysBlockedIpGet"]),
            may_logs: self.has_all(&["sysLogQuery", "sysLogGet"]),
            may_maintenance: self.has("sysActionCreate") && actions.iter().any(|a| self.has(a)),
            may_dashboard: reads(&["sysMetricQuery", "sysMetricGet"]),
            may_history: reads(&["sysTraceQuery", "sysTraceGet"]),
            may_recovery: reads(&["sysArchivedItemQuery", "sysArchivedItemGet"]),
            may_alerts: reads(&["sysAlertGet"]),
            may_tenants: reads(&["sysTenantGet"]),
        }
    }

    /// Whether the key opens any admin area at all.
    pub(crate) fn opens_something(&self) -> bool {
        let a = self.access();
        a.may_queue
            || a.may_accounts
            || a.may_blocked
            || a.may_logs
            || a.may_maintenance
            || a.may_dashboard
            || a.may_history
            || a.may_recovery
            || a.may_alerts
            || a.may_tenants
    }
}

/// A method error as a plain-worded `Error::Invalid` (a licence missing, a permission missing)
/// or a temporary one. `what` completes "this key may not ...".
fn refusal(what: &str, args: &Value) -> Error {
    let kind = args["type"].as_str().unwrap_or("unknown");
    let detail = args["description"].as_str().filter(|d| !d.is_empty()).unwrap_or(kind);
    match kind {
        "forbidden" if detail.contains(ENTERPRISE_TEXT) => Error::invalid(NEEDS_LICENCE),
        "forbidden" => Error::invalid(format!("this key may not {what}")),
        "invalidArguments" | "unsupportedFilter" | "unsupportedSort" | "requestTooLarge" => {
            Error::invalid(format!("the server did not take the request: {detail}"))
        }
        _ => Error::temporary(format!("the server could not do it now: {detail}")),
    }
}

/// A set error (an object not made, changed or deleted) in the same words as [`refusal`].
pub(super) fn set_refusal(what: &str, e: &Value) -> Error {
    match e["type"].as_str() {
        Some("forbidden") => {
            let detail = e["description"].as_str().unwrap_or_default();
            let lower = detail.to_lowercase();
            if detail.contains(ENTERPRISE_TEXT) {
                Error::invalid(NEEDS_LICENCE)
            } else if lower.contains("external directory") {
                Error::invalid("this account is kept in an outside directory: its password cannot be changed here")
            } else if lower.contains("account limit reached") {
                Error::invalid("the licence's limit of accounts is reached")
            } else if lower.contains("grant permissions") {
                Error::invalid("this key may not give permissions that it does not have itself")
            } else if detail.is_empty() || PERMISSION_TEXTS.iter().any(|t| lower.starts_with(t)) {
                Error::invalid(format!("this key may not {what}"))
            } else {
                set_error(e)
            }
        }
        _ => set_error(e),
    }
}

/// The two sentences the server uses when the key lacks the right (compared in lower case).
const PERMISSION_TEXTS: [&str; 2] =
    ["you are not authorized to perform this action", "you are not authorized to create, update or destroy objects"];

pub(super) fn text(v: &Value, key: &str) -> String {
    v[key].as_str().unwrap_or_default().to_string()
}

pub(super) fn time(v: &Value) -> Option<i64> {
    DateTime::parse_from_rfc3339(v.as_str()?).ok().map(|t| t.timestamp())
}

fn count(v: &Value) -> u32 {
    v.as_u64().map_or(0, |n| n.min(u64::from(u32::MAX)) as u32)
}

fn shorten(s: &str) -> String {
    let s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if s.chars().count() <= DETAIL_MAX {
        return s;
    }
    let mut cut: String = s.chars().take(DETAIL_MAX).collect();
    cut.push('…');
    cut
}

impl Connection {
    /// A management request. A method that failed is an error with words for the user;
    /// `what` completes "this key may not ...".
    pub(super) async fn admin_request(&self, what: &str, calls: &[(&str, Value)]) -> Result<Vec<(String, Value)>> {
        let responses = self.call_keeping_errors(&[], calls).await?;
        match responses.iter().find(|(name, _)| name == "error") {
            Some((_, args)) => Err(refusal(what, args)),
            None => Ok(responses),
        }
    }

    /// The key's permissions and the edition of the server.
    pub(crate) async fn admin_info(&self) -> Result<AdminInfo> {
        let failed = |e: &dyn std::fmt::Display| Error::temporary(format!("the permissions could not be read: {e}"));
        let url =
            url::Url::parse(self.client.session_url()).and_then(|u| u.join("/api/account")).map_err(|e| failed(&e))?;
        let answer = self.http.get(url).headers(self.client.headers().clone()).send().await.map_err(|e| failed(&e))?;
        let status = answer.status().as_u16();
        if status == 401 {
            return Err(unauthorized("api/account: HTTP 401"));
        }
        if status == 403 {
            return Err(super::errors::forbidden("api/account: HTTP 403"));
        }
        if !(200..300).contains(&status) {
            return Err(failed(&format!("HTTP {status}")));
        }
        let bytes = answer.bytes().await.map_err(|e| failed(&e))?;
        let body: Value = serde_json::from_slice(&bytes).map_err(|e| failed(&e))?;
        let permissions = body["permissions"].as_array().into_iter().flatten().filter_map(Value::as_str);
        Ok(AdminInfo {
            edition: body["edition"].as_str().unwrap_or_default().to_lowercase(),
            permissions: permissions.map(str::to_string).collect(),
        })
    }

    /// The figures of the home screen. A figure the key may not read is `None`.
    pub(crate) async fn admin_summary(&self) -> Result<AdminSummary> {
        let account = self.account_id();
        let mut figures = [None, None, None];
        for (i, object) in ["QueuedMessage", "Account", "BlockedIp"].into_iter().enumerate() {
            let args = json!({ "accountId": account, "limit": 0, "calculateTotal": true });
            // One request each: a method error does not say which call it was.
            let method = format!("x:{object}/query");
            let responses = self.call_keeping_errors(&[], &[(&method, args)]).await?;
            match responses.first() {
                Some((name, answer)) if name == "error" => {
                    if answer["type"] != "forbidden"
                        || answer["description"].as_str().is_some_and(|d| d.contains(ENTERPRISE_TEXT))
                    {
                        return Err(refusal("see the figures", answer));
                    }
                }
                Some((_, answer)) => figures[i] = answer["total"].as_u64().map(|n| count(&json!(n))),
                None => {}
            }
        }
        let [queue_waiting, accounts, blocked] = figures;
        Ok(AdminSummary { queue_waiting, accounts, blocked })
    }

    // ---- the mail queue ----

    pub(crate) async fn queue_page(&self, offset: usize, limit: usize) -> Result<QueuePage> {
        let account = self.account_id();
        let query = json!({
            "accountId": account, "sort": [{ "property": "due", "isAscending": true }],
            "position": offset, "limit": limit.min(PAGE_MAX), "calculateTotal": true,
        });
        let get = json!({
            "accountId": account, "properties": QUEUE_PROPERTIES,
            "#ids": { "resultOf": "c0", "name": "x:QueuedMessage/query", "path": "/ids" },
        });
        let calls = [("x:QueuedMessage/query", query), ("x:QueuedMessage/get", get)];
        let responses = self.admin_request("see the mail queue", &calls).await?;
        let answer = response(&responses, "x:QueuedMessage/query")?;
        let ids: Vec<&str> = answer["ids"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
        let list = response(&responses, "x:QueuedMessage/get")?["list"].as_array().cloned().unwrap_or_default();
        // The server's order is the order of the ids: the next to be tried first.
        let mut messages: Vec<(usize, QueuedMessage)> = list
            .iter()
            .filter_map(|m| Some((ids.iter().position(|id| m["id"].as_str() == Some(id))?, queued_message(m)?)))
            .collect();
        messages.sort_by_key(|(place, _)| *place);
        let total = answer["total"].as_u64().map_or(messages.len() as u32, |t| count(&json!(t)));
        Ok(QueuePage { messages: messages.into_iter().map(|(_, m)| m).collect(), total })
    }

    pub(crate) async fn queue_message(&self, id: &str) -> Result<QueuedMessage> {
        let args = json!({ "accountId": self.account_id(), "ids": [id], "properties": QUEUE_PROPERTIES });
        let responses = self.admin_request("see the mail queue", &[("x:QueuedMessage/get", args)]).await?;
        let list = response(&responses, "x:QueuedMessage/get")?["list"].as_array().cloned().unwrap_or_default();
        list.first().and_then(queued_message).ok_or_else(|| Error::invalid(MESSAGE_GONE))
    }

    pub(crate) async fn queue_retry(&self, id: &str) -> Result<()> {
        let args = json!({ "accountId": self.account_id(), "update": { id: { "nextRetry": RETRY_NOW } } });
        let responses = self.admin_request("change the mail queue", &[("x:QueuedMessage/set", args)]).await?;
        let set = response(&responses, "x:QueuedMessage/set")?;
        if let Some(e) = set["notUpdated"].get(id) {
            return Err(if e["type"] == "notFound" {
                Error::invalid(MESSAGE_GONE)
            } else {
                set_refusal("change the mail queue", e)
            });
        }
        updated(set, id, MESSAGE_GONE)
    }

    /// Takes the message out of the queue. One that is gone already is as good as taken out.
    pub(crate) async fn queue_cancel(&self, id: &str) -> Result<()> {
        let args = json!({ "accountId": self.account_id(), "destroy": [id] });
        let responses = self.admin_request("change the mail queue", &[("x:QueuedMessage/set", args)]).await?;
        let set = response(&responses, "x:QueuedMessage/set")?;
        match set["notDestroyed"].get(id) {
            Some(e) if e["type"] == "forbidden" => Err(set_refusal("change the mail queue", e)),
            _ => destroyed(set, id),
        }
    }

    // ---- blocked addresses ----

    /// The blocked addresses, newest first. The server has no order to ask for, so the list is
    /// read to the end (it is short) and sorted here.
    pub(crate) async fn blocked_list(&self, offset: usize, limit: usize) -> Result<Vec<BlockedAddress>> {
        let account = self.account_id();
        let ids = self.all_ids("x:BlockedIp", &json!({ "accountId": account }), "see the blocked addresses").await?;
        let mut items = Vec::with_capacity(ids.len());
        for chunk in ids.chunks(self.get_batch()) {
            let args = json!({ "accountId": account, "ids": chunk });
            let responses = self.admin_request("see the blocked addresses", &[("x:BlockedIp/get", args)]).await?;
            items.extend(response(&responses, "x:BlockedIp/get")?["list"].as_array().cloned().unwrap_or_default());
        }
        let mut blocked: Vec<BlockedAddress> = items.iter().filter_map(blocked_address).collect();
        blocked.sort_by(|a, b| b.blocked_at.cmp(&a.blocked_at).then_with(|| a.address.cmp(&b.address)));
        Ok(blocked.into_iter().skip(offset).take(limit.min(PAGE_MAX)).collect())
    }

    pub(crate) async fn unblock(&self, id: &str) -> Result<()> {
        let args = json!({ "accountId": self.account_id(), "destroy": [id] });
        let responses = self.admin_request("unblock addresses", &[("x:BlockedIp/set", args)]).await?;
        let set = response(&responses, "x:BlockedIp/set")?;
        match set["notDestroyed"].get(id) {
            Some(e) if e["type"] == "forbidden" => Err(set_refusal("unblock addresses", e)),
            _ => destroyed(set, id),
        }
    }

    // ---- logs ----

    /// The newest lines that have `text`, newest first. The server's order is by log file, so a
    /// window of its lines is read and put in time order here.
    pub(crate) async fn log_lines(&self, text: &str, limit: usize) -> Result<Vec<LogLine>> {
        let account = self.account_id();
        let mut query = json!({ "accountId": account, "limit": LOG_WINDOW });
        if !text.trim().is_empty() {
            query["filter"] = json!({ "text": text.trim() });
        }
        let responses = self.admin_request("read the log", &[("x:Log/query", query)]).await?;
        let ids: Vec<String> = response(&responses, "x:Log/query")?["ids"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|id| id.as_str().map(str::to_string))
            .collect();
        let mut lines = Vec::new();
        for chunk in ids.chunks(self.get_batch()) {
            let args = json!({ "accountId": account, "ids": chunk });
            let responses = self.admin_request("read the log", &[("x:Log/get", args)]).await?;
            lines.extend(response(&responses, "x:Log/get")?["list"].as_array().into_iter().flatten().map(log_line));
        }
        // Stable: lines of one second stay in the server's order. A line with no time is last.
        lines.sort_by_key(|l: &LogLine| std::cmp::Reverse(l.at));
        lines.truncate(limit.min(LOG_PAGE_MAX));
        Ok(lines)
    }

    // ---- maintenance ----

    pub(crate) async fn run_maintenance(&self, what: Maintenance) -> Result<()> {
        let kind = match what {
            Maintenance::ReloadSettings => "ReloadSettings",
            Maintenance::ReloadCertificates => "ReloadTlsCertificates",
            Maintenance::PauseQueue => "PauseMtaQueue",
            Maintenance::ResumeQueue => "ResumeMtaQueue",
        };
        let args = json!({ "accountId": self.account_id(), "create": { super::send::CREATE_ID: { "@type": kind } } });
        let responses = self.admin_request("run that action", &[("x:Action/set", args)]).await?;
        let set = response(&responses, "x:Action/set")?;
        match set["notCreated"].get(super::send::CREATE_ID) {
            Some(e) => Err(set_refusal("run that action", e)),
            None => Ok(()),
        }
    }

    // ---- shared by the lists ----

    /// Every id the query gives (at most [`LIST_MAX`]), in the server's order.
    pub(super) async fn all_ids(&self, object: &str, args: &Value, what: &str) -> Result<Vec<String>> {
        let mut query = args.clone();
        query["limit"] = json!(LIST_MAX);
        let responses = self.admin_request(what, &[(&format!("{object}/query"), query)]).await?;
        let ids = response(&responses, &format!("{object}/query"))?["ids"].as_array().cloned().unwrap_or_default();
        Ok(ids.iter().filter_map(|id| id.as_str().map(str::to_string)).collect())
    }
}

const QUEUE_PROPERTIES: [&str; 5] = ["returnPath", "recipients", "size", "createdAt", "nextRetry"];

/// The words for a recipient's state.
fn recipient_status(kind: &str) -> &'static str {
    match kind {
        "Completed" => "delivered",
        "TemporaryFailure" => "failed for now",
        "PermanentFailure" => "failed",
        _ => "waiting",
    }
}

/// What went wrong with a delivery, in plain words, and what the other server said.
fn delivery_error(status: &Value) -> String {
    let words = match status["errorType"].as_str() {
        Some("dnsError") => "the address could not be looked up",
        Some("unexpectedResponse") => "the other server refused it",
        Some("connectionError") => "the other server could not be reached",
        Some("tlsError") => "the secure connection failed",
        Some("daneError") | Some("mtaStsError") => "the other server's security policy was not met",
        Some("rateLimited") | Some("concurrencyLimited") => "delayed to keep within the limits",
        Some("io") => "the connection broke",
        Some("encryptionError") => "it could not be encrypted",
        _ => "",
    };
    let said = [status["responseMessage"].as_str(), status["errorMessage"].as_str()]
        .into_iter()
        .flatten()
        .find(|s| !s.trim().is_empty())
        .map(|s| {
            let code = status["responseCode"].as_u64().map(|c| format!("{c} ")).unwrap_or_default();
            shorten(&format!("{code}{s}"))
        })
        .unwrap_or_default();
    match (words.is_empty(), said.is_empty()) {
        (true, _) => said,
        (false, true) => words.to_string(),
        (false, false) => format!("{words}: {said}"),
    }
}

fn queued_message(m: &Value) -> Option<QueuedMessage> {
    let id = m["id"].as_str()?.to_string();
    let mut recipients: Vec<(QueuedRecipient, u32, Option<i64>)> = Vec::new();
    for (address, r) in m["recipients"].as_object().into_iter().flatten() {
        let status = &r["status"];
        let kind = status["@type"].as_str().unwrap_or("Scheduled");
        let detail = match kind {
            "Completed" => status["responseMessage"].as_str().map(shorten).unwrap_or_default(),
            "TemporaryFailure" | "PermanentFailure" => delivery_error(status),
            _ => String::new(),
        };
        let due = if kind == "Completed" || kind == "PermanentFailure" { None } else { time(&r["retryDue"]) };
        recipients.push((
            QueuedRecipient { address: address.clone(), status: recipient_status(kind).to_string(), detail },
            count(&r["retryCount"]),
            due,
        ));
    }
    recipients.sort_by(|a, b| a.0.address.cmp(&b.0.address));
    let tries = recipients.iter().map(|(_, tries, _)| *tries).max().unwrap_or(0);
    let next_try_at = time(&m["nextRetry"]).or_else(|| recipients.iter().filter_map(|(_, _, due)| *due).min());
    // The last error: the first failing recipient's, a permanent one before a temporary one.
    let failing = |status: &str| recipients.iter().find(|(r, _, _)| r.status == status && !r.detail.is_empty());
    let error = failing("failed").or_else(|| failing("failed for now")).map(|(r, _, _)| r.detail.clone());
    Some(QueuedMessage {
        id,
        from: text(m, "returnPath"),
        recipients: recipients.into_iter().map(|(r, _, _)| r).collect(),
        size: m["size"].as_u64().unwrap_or(0),
        queued_at: time(&m["createdAt"]),
        next_try_at,
        tries,
        error: error.unwrap_or_default(),
    })
}

fn block_reason(kind: &str) -> String {
    match kind {
        "rcptToFailure" => "Excessive failed RCPT TO commands",
        "authFailure" => "Excessive failed authentication attempts",
        "loitering" => "Excessive loitering connections",
        "portScanning" => "Excessive port scanning attempts",
        "manual" => "Manually blocked IP address",
        "" => "",
        _ => "Other reason",
    }
    .to_string()
}

fn blocked_address(b: &Value) -> Option<BlockedAddress> {
    Some(BlockedAddress {
        id: b["id"].as_str()?.to_string(),
        address: text(b, "address"),
        reason: block_reason(b["reason"].as_str().unwrap_or_default()),
        blocked_at: time(&b["createdAt"]),
        expires_at: time(&b["expiresAt"]),
    })
}

/// The level in the words the contract names; empty when the line has none.
fn level_name(level: &str) -> String {
    match level.to_ascii_lowercase().as_str() {
        "error" | "err" => "error",
        "warn" | "warning" => "warning",
        "info" => "info",
        "debug" => "debug",
        "trace" => "trace",
        _ => "",
    }
    .to_string()
}

/// A log entry as a line. The server gives time, level, event and details apart; a line that
/// comes as one text has the time and level read from its first two words where they are there.
fn log_line(entry: &Value) -> LogLine {
    let event = entry["event"].as_str().unwrap_or_default();
    let details = entry["details"].as_str().unwrap_or_default();
    let body = [event, details].iter().filter(|s| !s.is_empty()).copied().collect::<Vec<_>>().join(" ");
    let (mut at, mut level) = (time(&entry["timestamp"]), level_name(entry["level"].as_str().unwrap_or_default()));
    let mut text = if body.is_empty() { entry["text"].as_str().unwrap_or_default().to_string() } else { body };
    if at.is_none() {
        if let Some((first, rest)) = text.split_once(' ') {
            if let Some(t) = time(&json!(first)) {
                at = Some(t);
                text = rest.trim_start().to_string();
            }
        }
    }
    if level.is_empty() {
        let first = text.split(' ').next().unwrap_or_default();
        level = level_name(first.trim_matches(|c: char| !c.is_ascii_alphabetic()));
    }
    LogLine { at, level, text }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_log_line_with_its_time_and_level_in_the_text_is_read() {
        let line = log_line(&json!({ "text": "2026-11-14T10:00:00Z WARN something odd happened" }));
        assert_eq!(line.at, Some(1_794_650_400));
        assert_eq!(line.level, "warning");
        assert_eq!(line.text, "WARN something odd happened");
    }

    #[test]
    fn a_log_line_the_server_split_up_is_put_together() {
        let line = log_line(&json!({
            "timestamp": "2026-11-14T10:00:00Z", "level": "warn", "event": "auth.failed", "details": "remoteIp = 1.2.3.4",
        }));
        assert_eq!((line.level.as_str(), line.text.as_str()), ("warning", "auth.failed remoteIp = 1.2.3.4"));
    }

    #[test]
    fn a_line_with_neither_time_nor_level_keeps_its_text() {
        let line = log_line(&json!({ "text": "just words here" }));
        assert_eq!((line.at, line.level.as_str(), line.text.as_str()), (None, "", "just words here"));
    }

    #[test]
    fn an_enterprise_refusal_and_a_missing_permission_are_told_apart_by_the_text() {
        let licence = json!({ "type": "forbidden", "description": "This feature is only available in the Enterprise edition. Obtain your trial license at https://license.stalw.art/trial." });
        assert_eq!(refusal("see the figures", &licence), Error::invalid(NEEDS_LICENCE));
        assert_eq!(set_refusal("see the figures", &licence), Error::invalid(NEEDS_LICENCE));
        let permission = json!({ "type": "forbidden", "description": "You are not authorized to perform this action" });
        assert_eq!(refusal("see the figures", &permission), Error::invalid("this key may not see the figures"));
        assert_eq!(set_refusal("see the figures", &permission), Error::invalid("this key may not see the figures"));
        assert!(matches!(refusal("x", &json!({ "type": "serverFail" })), Error::Temporary { .. }));
    }
}
