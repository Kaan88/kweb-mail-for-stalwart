//! What a licensed (Enterprise) Stalwart server adds, over plain JMAP requests: stored metrics,
//! delivery traces, archived (deleted) items, alerts and tenants. jmap-client knows none of
//! these objects, so the calls are raw JSON like those of `files.rs`. What the server really
//! does is in `dev/notes/stalwart-admin-access.md` and `stalwart-enterprise-observed.md`; the
//! points that shape this code:
//!
//! - Management is JMAP on the usual API URL, with the methods `x:<Object>/get|query|set` and
//!   the capability `urn:stalwart:jmap`.
//! - Whether the server is licensed is only known from `GET /api/account` (`edition`). On an
//!   unlicensed server the Enterprise objects answer with a method error `forbidden` whose text
//!   says "only available in the Enterprise edition"; that is the same type as "not allowed", so
//!   the text decides.
//! - A query gives ids; a get gives the objects (at most 500 ids at a time).
//! - Nothing from a licensed server was seen for archived items and the recovery setting
//!   (the local test server has no licence): those shapes are from the documentation, and what
//!   is read from them is read loosely.
//!
//! This layer returns the server's JSON; turning it into the app's types is the job of the
//! functions in `ffi/enterprise`.

use std::collections::HashSet;

use chrono::{DateTime, SecondsFormat};
use serde_json::{json, Value};

use super::{errors::unauthorized, Connection};
use crate::{Error, Result};

pub const MANAGE_CAPABILITY: &str = "urn:stalwart:jmap";
pub const ENTERPRISE_NEEDED: &str = "this needs an Enterprise licence on the server";
const ENTERPRISE_MARK: &str = "only available in the Enterprise edition";
/// The most ids one `get` is given (the server refuses more).
const GET_BATCH: usize = 500;
/// How many ids one `query` is asked for.
const QUERY_PAGE: usize = 500;
/// A server that never ends a list does not make a request that never ends.
const MAX_ROWS: usize = 200_000;
/// The event every outbound delivery trace starts with (the web UI's "outbound delivery" list
/// filters on it).
/// How far back a delivery list reaches when no start is given: a year, more than any
/// retention the server is likely to have and well after its ids begin.
const HISTORY_REACH: i64 = 365 * 86_400;
const DELIVERY_EVENT: &str = "delivery.attempt-start";

fn now_secs() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

/// Unix seconds as the UTC time the server's filters read.
pub fn iso(secs: i64) -> String {
    DateTime::from_timestamp(secs, 0).unwrap_or_default().to_rfc3339_opts(SecondsFormat::Secs, true)
}

/// A method error in words for the user. The server's own text is only used for what is not a
/// refusal: it never holds a key.
fn refusal(what: &str, error: &Value) -> Error {
    let kind = error["type"].as_str().unwrap_or("unknown");
    let detail = error["description"].as_str().unwrap_or_default();
    if detail.contains(ENTERPRISE_MARK) {
        return Error::invalid(ENTERPRISE_NEEDED);
    }
    match kind {
        "forbidden" => Error::invalid(format!("this key may not {what}")),
        "invalidArguments" | "requestTooLarge" | "unknownMethod" => {
            Error::invalid(format!("the server cannot {what}: {kind}"))
        }
        _ => {
            Error::temporary(format!("the server could not {what}: {}", if detail.is_empty() { kind } else { detail }))
        }
    }
}

fn error_of(answers: &[(String, Value)]) -> Option<&Value> {
    answers.iter().find(|(name, _)| name == "error").map(|(_, args)| args)
}

/// The ids of a query answer.
fn ids_of(answer: &Value) -> Vec<String> {
    answer["ids"].as_array().into_iter().flatten().filter_map(|i| i.as_str().map(str::to_string)).collect()
}

impl Connection {
    /// The server's own address (scheme, host, port): where `/api/...` lives. It is the address
    /// the user signed in at, never one the server names in an answer.
    pub fn server_origin(&self) -> Result<String> {
        let url = url::Url::parse(self.client.session_url())
            .map_err(|_| Error::invalid("the server address is not usable"))?;
        Ok(url.origin().ascii_serialization())
    }

    /// The `Authorization` header of this connection, and nothing else of it.
    pub fn key_header(&self) -> Result<reqwest::header::HeaderValue> {
        self.client
            .headers()
            .get(reqwest::header::AUTHORIZATION)
            .cloned()
            .ok_or_else(|| Error::invalid("this connection has no key to send"))
    }

    /// Makes sure the server is licensed and that the key holds every permission in `needs`
    /// (`what` finishes "this key may not ..."). Returns the key's permissions, for a caller
    /// that wants to look at more. Licence first: an unlicensed server is refused without
    /// asking anything else.
    pub async fn require_enterprise(&self, needs: &[&str], what: &str) -> Result<HashSet<String>> {
        let url = format!("{}/api/account", self.server_origin()?);
        let mut request = self.http.get(url);
        request = request.header(reqwest::header::AUTHORIZATION, self.key_header()?);
        let response = request.send().await.map_err(|_| Error::temporary("could not reach the server"))?;
        let status = response.status().as_u16();
        if status == 401 {
            return Err(unauthorized("the server did not accept the key"));
        }
        if status == 403 {
            return Err(Error::invalid(format!("this key may not {what}")));
        }
        if !(200..300).contains(&status) {
            return Err(Error::temporary(format!("the server answered {status} for the account")));
        }
        let bytes =
            response.bytes().await.map_err(|_| Error::temporary("the server's answer for the account broke off"))?;
        let account: Value = serde_json::from_slice(&bytes)
            .map_err(|_| Error::temporary("the server's answer for the account is not JSON"))?;
        if account["edition"].as_str() != Some("enterprise") {
            return Err(Error::invalid(ENTERPRISE_NEEDED));
        }
        let held: HashSet<String> = account["permissions"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|p| p.as_str().map(str::to_string))
            .collect();
        // A server that sends no list is not asked to prove anything: the method calls will say.
        if !held.is_empty() && needs.iter().any(|n| !held.contains(*n)) {
            return Err(Error::invalid(format!("this key may not {what}")));
        }
        Ok(held)
    }

    /// One management request. A method that failed is an error with words for the user.
    async fn manage(&self, what: &str, calls: &[(&str, Value)]) -> Result<Vec<(String, Value)>> {
        let answers = self.call_keeping_errors(&[MANAGE_CAPABILITY], calls).await?;
        match error_of(&answers) {
            Some(error) => Err(refusal(what, error)),
            None => Ok(answers),
        }
    }

    /// `manage` for one call, giving its answer.
    async fn manage_one(&self, what: &str, method: &str, args: Value) -> Result<Value> {
        let answers = self.manage(what, &[(method, args)]).await?;
        answers
            .into_iter()
            .find(|(name, _)| name == method)
            .map(|(_, args)| args)
            .ok_or_else(|| Error::temporary(format!("the server's answer has no {method}")))
    }

    /// The objects with these ids, in chunks the server takes, in the order asked. An id the
    /// server no longer has is left out.
    async fn manage_get(&self, what: &str, object: &str, ids: &[String]) -> Result<Vec<Value>> {
        let method = format!("{object}/get");
        let mut found = Vec::new();
        for chunk in ids.chunks(GET_BATCH) {
            let args = json!({ "accountId": self.account_id, "ids": chunk });
            let answer = self.manage_one(what, &method, args).await?;
            found.extend(answer["list"].as_array().cloned().unwrap_or_default());
        }
        let position = |v: &Value| ids.iter().position(|i| Some(i.as_str()) == v["id"].as_str()).unwrap_or(usize::MAX);
        found.sort_by_key(position);
        Ok(found)
    }

    /// A query with a sort, run again without it when the server does not know that sort
    /// (what the server sorts by is not the same for every object).
    async fn manage_query(&self, what: &str, object: &str, mut args: Value, sort: Value) -> Result<Value> {
        let method = format!("{object}/query");
        args["sort"] = sort;
        let answers = self.call_keeping_errors(&[MANAGE_CAPABILITY], &[(&method, args.clone())]).await?;
        match error_of(&answers) {
            Some(error) if error["type"].as_str() == Some("unsupportedSort") => {
                args.as_object_mut().map(|o| o.remove("sort"));
                self.manage_one(what, &method, args).await
            }
            Some(error) => Err(refusal(what, error)),
            None => answers
                .into_iter()
                .find(|(name, _)| *name == method)
                .map(|(_, args)| args)
                .ok_or_else(|| Error::temporary(format!("the server's answer has no {method}"))),
        }
    }

    // ---- stored metrics ----

    /// Every stored metric row named in `names` between `from` and `to` (Unix seconds), all
    /// pages of it.
    pub async fn metric_rows(&self, from: i64, to: i64, names: &[&str]) -> Result<Vec<Value>> {
        let what = "read the server's history";
        let filter = json!({
            "timestampIsGreaterThanOrEqual": iso(from),
            "timestampIsLessThanOrEqual": iso(to),
            "metric": names,
        });
        let mut ids: Vec<String> = Vec::new();
        let mut seen: HashSet<String> = HashSet::new();
        loop {
            let args = json!({
                "accountId": self.account_id, "filter": filter, "position": ids.len(), "limit": QUERY_PAGE,
            });
            let answer = self.manage_one(what, "x:Metric/query", args).await?;
            let fresh: Vec<String> = ids_of(&answer).into_iter().filter(|i| seen.insert(i.clone())).collect();
            // An empty page, or one with nothing new (a server that ignores the position),
            // is the end.
            if fresh.is_empty() {
                break;
            }
            ids.extend(fresh);
            if ids.len() >= MAX_ROWS {
                break;
            }
        }
        self.manage_get(what, "x:Metric", &ids).await
    }

    // ---- delivery traces ----

    /// One page of the traces of delivery attempts, newest first, with the server's count of
    /// all that match. `text` is the server's text filter (blank: none).
    pub async fn delivery_traces(
        &self,
        text: &str,
        since: Option<i64>,
        offset: usize,
        limit: usize,
    ) -> Result<(Vec<Value>, u64)> {
        let what = "read the delivery history";
        let mut filter = json!({ "event": DELIVERY_EVENT });
        let text = text.trim();
        // The server indexes the whole values of the addresses, domains and hosts as keywords
        // and the queue id as a number of its own, so a queue id is asked for as one.
        let by_queue_id = !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit()) && text.parse::<u64>().is_ok();
        if by_queue_id {
            filter["queueId"] = json!(text);
        } else if !text.is_empty() {
            filter["text"] = json!(text);
        }
        // Without a text, a queue id or a time the server answers only for the last 24 hours, which
        // is not what "as far back as the server keeps" means: a lower bound is always sent.
        let lowest = since.unwrap_or_else(|| now_secs() - HISTORY_REACH);
        filter["timestampIsGreaterThanOrEqual"] = json!(iso(lowest));
        let args = json!({
            "accountId": self.account_id, "filter": filter, "position": offset, "limit": limit,
            "calculateTotal": true,
        });
        let sort = json!([{ "property": "timestamp", "isAscending": false }]);
        let answer = self.manage_query(what, "x:Trace", args, sort).await?;
        let ids = ids_of(&answer);
        let total = answer["total"].as_u64().unwrap_or((offset + ids.len()) as u64);
        Ok((self.manage_get(what, "x:Trace", &ids).await?, total))
    }

    /// One trace with all its events; `None` when the server no longer has it.
    pub async fn delivery_trace(&self, id: &str) -> Result<Option<Value>> {
        let ids = [id.to_string()];
        Ok(self.manage_get("read the delivery history", "x:Trace", &ids).await?.into_iter().next())
    }

    // ---- deleted items ----

    /// The `archiveDeletedItemsFor` of the data retention settings as the server sent it
    /// (milliseconds, or null when off). `Value::Null` when the answer has nothing of the kind.
    pub async fn retention_setting(&self) -> Result<Value> {
        let args = json!({
            "accountId": self.account_id, "ids": ["singleton"], "properties": ["archiveDeletedItemsFor"],
        });
        let answer = self.manage_one("read the deleted-item settings", "x:DataRetention/get", args).await?;
        Ok(answer["list"][0]["archiveDeletedItemsFor"].clone())
    }

    /// One page of the deleted items kept for `account`, newest first, and the server's count.
    pub async fn archived_items(&self, account: &str, offset: usize, limit: usize) -> Result<(Vec<Value>, u64)> {
        let what = "look at deleted items";
        let args = json!({
            "accountId": self.account_id, "filter": { "accountId": account }, "position": offset, "limit": limit,
            "calculateTotal": true,
        });
        let sort = json!([{ "property": "archivedAt", "isAscending": false }]);
        let answer = self.manage_query(what, "x:ArchivedItem", args, sort).await?;
        let ids = ids_of(&answer);
        let total = answer["total"].as_u64().unwrap_or((offset + ids.len()) as u64);
        Ok((self.manage_get(what, "x:ArchivedItem", &ids).await?, total))
    }

    /// Asks the server to restore these items. One it refuses is an error that says so; the
    /// others were asked for already.
    pub async fn archived_restore(&self, ids: &[String]) -> Result<()> {
        let update: serde_json::Map<String, Value> =
            ids.iter().map(|id| (id.clone(), json!({ "status": "requestRestore" }))).collect();
        let args = json!({ "accountId": self.account_id, "update": update });
        let answer = self.manage_one("restore deleted items", "x:ArchivedItem/set", args).await?;
        let refused = answer["notUpdated"].as_object().filter(|m| !m.is_empty());
        let Some(refused) = refused else { return Ok(()) };
        let all_gone = refused.values().all(|e| e["type"].as_str() == Some("notFound"));
        let any_forbidden = refused.values().any(|e| e["type"].as_str() == Some("forbidden"));
        Err(if any_forbidden {
            Error::invalid("this key may not restore deleted items")
        } else if all_gone {
            Error::invalid("the server no longer keeps that item")
        } else {
            Error::temporary("the server did not restore every item")
        })
    }

    // ---- alerts ----

    pub async fn alert_objects(&self) -> Result<Vec<Value>> {
        let args = json!({ "accountId": self.account_id, "ids": null });
        let answer = self.manage_one("read the alerts", "x:Alert/get", args).await?;
        Ok(answer["list"].as_array().cloned().unwrap_or_default())
    }

    /// Switches an alert on or off. An alert the server does not have is `Error::Invalid`.
    pub async fn alert_enable(&self, id: &str, enabled: bool) -> Result<()> {
        let args = json!({ "accountId": self.account_id, "update": { id: { "enable": enabled } } });
        let answer = self.manage_one("change alerts", "x:Alert/set", args).await?;
        match answer["notUpdated"].get(id) {
            None | Some(Value::Null) => Ok(()),
            Some(e) if e["type"].as_str() == Some("notFound") => Err(Error::invalid("that alert is no longer there")),
            Some(e) if e["type"].as_str() == Some("forbidden") => Err(Error::invalid("this key may not change alerts")),
            Some(_) => Err(Error::temporary("the server did not change the alert")),
        }
    }

    // ---- tenants ----

    pub async fn tenant_objects(&self) -> Result<Vec<Value>> {
        let args = json!({ "accountId": self.account_id, "ids": null });
        let answer = self.manage_one("read the tenants", "x:Tenant/get", args).await?;
        Ok(answer["list"].as_array().cloned().unwrap_or_default())
    }
}
