//! The reports the server received from other mail servers: DMARC aggregate reports
//! (`x:DmarcExternalReport`), TLS reports (`x:TlsExternalReport`) and abuse or failure reports
//! (`x:ArfExternalReport`). See `dev/notes/stalwart-admin-more.md`. What matters here:
//!
//! - A query has no sort (`receivedAt` is refused) and no filter the app uses, so the ids are
//!   read to the end, their `receivedAt` is read, and the list is put in order and cut into
//!   pages here, as the blocked addresses are.
//! - The report is one object property (`report`): it cannot be asked for in parts. A list
//!   page therefore reads the whole report of each row on that page (a page is 50 at most).
//! - A report cannot be changed, only deleted (a key needs the destroy permission).
//! - The shapes were read from the server's schema (`GET /api/schema`): the local server had
//!   no report to show. The reading is defensive: a missing part is left out, never an error.
//! - An ARF report carries part of the original message (`message`, `headers`); the app
//!   shows none of it.

use std::collections::BTreeMap;

use serde_json::{json, Value};

use super::{count, set_refusal, text, time, PAGE_MAX};
use crate::{
    ffi::{ReportDetail, ReportFact, ReportGroup, ReportKind, ReportPage, ReportRow},
    jmap::{
        send::{destroyed, response},
        Connection,
    },
    Error, Result,
};

const GONE: &str = "that report is no longer on the server";
/// How many sources or policies one report detail lists at most.
const GROUPS_MAX: usize = 100;

fn object(kind: ReportKind) -> &'static str {
    match kind {
        ReportKind::Dmarc => "x:DmarcExternalReport",
        ReportKind::Tls => "x:TlsExternalReport",
        ReportKind::Arf => "x:ArfExternalReport",
    }
}

/// "startTlsNotSupported" as "start tls not supported".
fn spaced(camel: &str) -> String {
    let mut out = String::new();
    for c in camel.chars() {
        if c.is_ascii_uppercase() {
            out.push(' ');
            out.push(c.to_ascii_lowercase());
        } else {
            out.push(c);
        }
    }
    out.trim().to_string()
}

/// An enum word of the server as words; "unspecified" and nothing are empty.
fn word(v: &Value) -> String {
    match v.as_str() {
        Some("unspecified") | None => String::new(),
        Some(s) => spaced(s),
    }
}

fn strings(v: &Value) -> Vec<String> {
    let mut out: Vec<String> = match v {
        Value::Object(map) => map.keys().cloned().collect(),
        Value::Array(list) => list.iter().filter_map(|s| s.as_str().map(str::to_string)).collect(),
        _ => Vec::new(),
    };
    out.sort();
    out
}

fn list(v: &Value) -> Vec<&Value> {
    match v {
        Value::Array(items) => items.iter().collect(),
        Value::Object(map) => {
            // An object list is keyed "0", "1", ...: in that order.
            let mut keyed: Vec<(&String, &Value)> = map.iter().collect();
            keyed.sort_by_key(|(k, _)| (k.parse::<u64>().unwrap_or(u64::MAX), (*k).clone()));
            keyed.into_iter().map(|(_, v)| v).collect()
        }
        _ => Vec::new(),
    }
}

fn plural(n: u32, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// A fact, left out when it has no value.
fn fact(facts: &mut Vec<ReportFact>, label: &str, value: impl Into<String>) {
    let value = value.into();
    if !value.trim().is_empty() {
        facts.push(ReportFact { label: label.to_string(), value });
    }
}

/// A DMARC record passes when either of its two checks passed and aligned.
fn dmarc_passed(record: &Value) -> bool {
    record["evaluatedDkim"] == "pass" || record["evaluatedSpf"] == "pass"
}

struct Totals {
    passed: u32,
    failed: u32,
}

fn dmarc_totals(report: &Value) -> Totals {
    let mut t = Totals { passed: 0, failed: 0 };
    for record in list(&report["records"]) {
        let n = count(&record["count"]);
        if dmarc_passed(record) {
            t.passed = t.passed.saturating_add(n);
        } else {
            t.failed = t.failed.saturating_add(n);
        }
    }
    t
}

fn tls_totals(report: &Value) -> Totals {
    let mut t = Totals { passed: 0, failed: 0 };
    for policy in list(&report["policies"]) {
        t.passed = t.passed.saturating_add(count(&policy["totalSuccessfulSessions"]));
        t.failed = t.failed.saturating_add(count(&policy["totalFailedSessions"]));
    }
    t
}

fn tls_policy_type(v: &Value) -> &'static str {
    match v.as_str() {
        Some("tlsa") => "DANE",
        Some("sts") => "MTA-STS",
        Some("noPolicyFound") => "no policy",
        _ => "other",
    }
}

fn first(v: &Value) -> String {
    strings(v).into_iter().next().unwrap_or_default()
}

fn row(kind: ReportKind, r: &Value) -> Option<ReportRow> {
    let report = &r["report"];
    let (org, domain, totals, note) = match kind {
        ReportKind::Dmarc => {
            let t = dmarc_totals(report);
            (text(report, "orgName"), text(report, "policyDomain"), Some(t), String::new())
        }
        ReportKind::Tls => {
            let policies = list(&report["policies"]);
            let domain = policies.first().map(|p| text(p, "policyDomain")).unwrap_or_default();
            (text(report, "organizationName"), domain, Some(tls_totals(report)), String::new())
        }
        ReportKind::Arf => {
            let domain = Some(first(&report["reportedDomains"])).filter(|d| !d.is_empty());
            let domain = domain.unwrap_or_else(|| text(report, "dkimDomain"));
            (text(report, "reportingMta"), domain, None, word(&report["feedbackType"]))
        }
    };
    Some(ReportRow {
        id: r["id"].as_str()?.to_string(),
        kind,
        from: text(r, "from"),
        subject: text(r, "subject"),
        organization: org,
        domain,
        received_at: time(&r["receivedAt"]),
        passed: totals.as_ref().map(|t| t.passed),
        failed: match kind {
            ReportKind::Arf => Some(count(&report["incidents"])).filter(|n| *n > 0),
            _ => totals.map(|t| t.failed),
        },
        note,
    })
}

fn dmarc_policy_line(report: &Value) -> String {
    let mut parts = Vec::new();
    let policy = word(&report["policyDisposition"]);
    if !policy.is_empty() {
        parts.push(format!("p={policy}"));
    }
    let sub = word(&report["policySubdomainDisposition"]);
    if !sub.is_empty() {
        parts.push(format!("subdomains {sub}"));
    }
    if report["policyTestingMode"] == true {
        parts.push("testing mode".into());
    }
    parts.join(", ")
}

fn dmarc_groups(report: &Value) -> Vec<ReportGroup> {
    let mut records: Vec<&Value> = list(&report["records"]);
    records.sort_by_key(|r| std::cmp::Reverse(count(&r["count"])));
    records
        .into_iter()
        .take(GROUPS_MAX)
        .map(|r| {
            let n = count(&r["count"]);
            let ip = Some(text(r, "sourceIp")).filter(|s| !s.is_empty()).unwrap_or_else(|| "unknown source".into());
            let mut facts = Vec::new();
            fact(&mut facts, "Result", if dmarc_passed(r) { "passed" } else { "failed" });
            fact(&mut facts, "What was done", word(&r["evaluatedDisposition"]));
            fact(&mut facts, "DKIM", word(&r["evaluatedDkim"]));
            fact(&mut facts, "SPF", word(&r["evaluatedSpf"]));
            fact(&mut facts, "From domain", text(r, "headerFrom"));
            fact(&mut facts, "Envelope from", text(r, "envelopeFrom"));
            let dkim: Vec<String> = list(&r["dkimResults"])
                .iter()
                .map(|d| {
                    let who = match (text(d, "domain"), text(d, "selector")) {
                        (domain, s) if s.is_empty() => domain,
                        (domain, s) => format!("{domain} ({s})"),
                    };
                    format!("{who}: {}", word(&d["result"]))
                })
                .collect();
            fact(&mut facts, "DKIM signatures", dkim.join("\n"));
            let spf: Vec<String> = list(&r["spfResults"])
                .iter()
                .map(|s| format!("{}: {}", text(s, "domain"), word(&s["result"])))
                .collect();
            fact(&mut facts, "SPF checks", spf.join("\n"));
            let reasons: Vec<String> = list(&r["policyOverrideReasons"])
                .iter()
                .map(|o| {
                    let kind = word(&o["overrideType"]);
                    match text(o, "comment") {
                        c if c.is_empty() => kind,
                        c => format!("{kind}: {c}"),
                    }
                })
                .collect();
            fact(&mut facts, "Policy overridden", reasons.join("\n"));
            ReportGroup { title: format!("{ip} - {}", plural(n, "message", "messages")), facts }
        })
        .collect()
}

fn tls_groups(report: &Value) -> Vec<ReportGroup> {
    list(&report["policies"])
        .into_iter()
        .take(GROUPS_MAX)
        .map(|p| {
            let mut facts = Vec::new();
            fact(&mut facts, "Sessions that worked", count(&p["totalSuccessfulSessions"]).to_string());
            fact(&mut facts, "Sessions that failed", count(&p["totalFailedSessions"]).to_string());
            fact(&mut facts, "Mail servers", strings(&p["mxHosts"]).join("\n"));
            let failures: Vec<String> = list(&p["failureDetails"])
                .iter()
                .map(|f| {
                    let host = [text(f, "receivingMxHostname"), text(f, "receivingIp")]
                        .into_iter()
                        .find(|h| !h.is_empty())
                        .unwrap_or_default();
                    let n = plural(count(&f["failedSessionCount"]), "session", "sessions");
                    let what = word(&f["resultType"]);
                    if host.is_empty() {
                        format!("{what}: {n}")
                    } else {
                        format!("{what}: {n} ({host})")
                    }
                })
                .collect();
            fact(&mut facts, "What went wrong", failures.join("\n"));
            ReportGroup { title: format!("{} ({})", text(p, "policyDomain"), tls_policy_type(&p["policyType"])), facts }
        })
        .collect()
}

fn detail(kind: ReportKind, r: &Value) -> Option<ReportDetail> {
    let report = &r["report"];
    let mut summary = Vec::new();
    let mut groups = Vec::new();
    let (mut start, mut end) = (None, None);
    match kind {
        ReportKind::Dmarc => {
            start = time(&report["dateRangeStart"]);
            end = time(&report["dateRangeEnd"]);
            fact(&mut summary, "Sent by", text(report, "orgName"));
            fact(&mut summary, "Contact", text(report, "email"));
            fact(&mut summary, "Report ID", text(report, "reportId"));
            fact(&mut summary, "Domain", text(report, "policyDomain"));
            fact(&mut summary, "Policy", dmarc_policy_line(report));
            let t = dmarc_totals(report);
            fact(&mut summary, "Messages that passed", t.passed.to_string());
            fact(&mut summary, "Messages that failed", t.failed.to_string());
            groups = dmarc_groups(report);
        }
        ReportKind::Tls => {
            start = time(&report["dateRangeStart"]);
            end = time(&report["dateRangeEnd"]);
            fact(&mut summary, "Sent by", text(report, "organizationName"));
            fact(&mut summary, "Contact", text(report, "contactInfo"));
            fact(&mut summary, "Report ID", text(report, "reportId"));
            let t = tls_totals(report);
            fact(&mut summary, "Sessions that worked", t.passed.to_string());
            fact(&mut summary, "Sessions that failed", t.failed.to_string());
            groups = tls_groups(report);
        }
        ReportKind::Arf => {
            fact(&mut summary, "Kind", word(&report["feedbackType"]));
            fact(&mut summary, "Reported by", text(report, "reportingMta"));
            fact(&mut summary, "Program", text(report, "userAgent"));
            fact(&mut summary, "Incidents", count(&report["incidents"]).to_string());
            fact(&mut summary, "Source address", text(report, "sourceIp"));
            fact(&mut summary, "Domains", strings(&report["reportedDomains"]).join("\n"));
            fact(&mut summary, "Addresses", strings(&report["reportedUris"]).join("\n"));
            fact(&mut summary, "What happened to the message", word(&report["deliveryResult"]));
            fact(&mut summary, "What failed", word(&report["authFailure"]));
            fact(&mut summary, "Signed by", text(report, "dkimDomain"));
            fact(&mut summary, "Original sender", text(report, "originalMailFrom"));
            fact(&mut summary, "Original recipient", text(report, "originalRcptTo"));
            fact(&mut summary, "Checks on arrival", strings(&report["authenticationResults"]).join("\n"));
        }
    }
    Some(ReportDetail {
        id: r["id"].as_str()?.to_string(),
        kind,
        from: text(r, "from"),
        to: strings(&r["to"]),
        subject: text(r, "subject"),
        received_at: time(&r["receivedAt"]),
        expires_at: time(&r["expiresAt"]),
        period_start: start,
        period_end: end,
        summary,
        groups,
    })
}

/// Whether a list of `n` ids reached the most the app reads, so that the server may have more.
fn list_was_cut(n: usize) -> bool {
    n >= super::LIST_MAX
}

impl Connection {
    /// A page of the reports of one kind, the newest first.
    pub(crate) async fn report_page(&self, kind: ReportKind, offset: usize, limit: usize) -> Result<ReportPage> {
        let account = self.account_id();
        let what = "see the reports";
        let ids = self.all_ids(object(kind), &json!({ "accountId": account }), what).await?;
        let truncated = list_was_cut(ids.len());
        // The order is made here: when each report came in.
        let mut dated: Vec<(Option<i64>, String)> = Vec::with_capacity(ids.len());
        for chunk in ids.chunks(self.get_batch()) {
            let args = json!({ "accountId": account, "ids": chunk, "properties": ["receivedAt"] });
            let method = format!("{}/get", object(kind));
            let responses = self.admin_request(what, &[(&method, args)]).await?;
            for item in response(&responses, &method)?["list"].as_array().into_iter().flatten() {
                if let Some(id) = item["id"].as_str() {
                    dated.push((time(&item["receivedAt"]), id.to_string()));
                }
            }
        }
        dated.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
        let total = dated.len().min(u32::MAX as usize) as u32;
        let page: Vec<String> = dated.into_iter().skip(offset).take(limit.min(PAGE_MAX)).map(|(_, id)| id).collect();
        if page.is_empty() {
            return Ok(ReportPage { reports: Vec::new(), total, truncated });
        }
        let args = json!({
            "accountId": account, "ids": page, "properties": ["from", "subject", "receivedAt", "report"],
        });
        let method = format!("{}/get", object(kind));
        let responses = self.admin_request(what, &[(&method, args)]).await?;
        let list = response(&responses, &method)?["list"].as_array().cloned().unwrap_or_default();
        let by_id: BTreeMap<&str, &Value> = list.iter().filter_map(|r| Some((r["id"].as_str()?, r))).collect();
        let reports = page.iter().filter_map(|id| by_id.get(id.as_str()).and_then(|r| row(kind, r))).collect();
        Ok(ReportPage { reports, total, truncated })
    }

    pub(crate) async fn report_detail(&self, kind: ReportKind, id: &str) -> Result<ReportDetail> {
        let method = format!("{}/get", object(kind));
        let args = json!({ "accountId": self.account_id(), "ids": [id] });
        let responses = self.admin_request("see the reports", &[(&method, args)]).await?;
        let list = response(&responses, &method)?["list"].as_array().cloned().unwrap_or_default();
        list.first().and_then(|r| detail(kind, r)).ok_or_else(|| Error::invalid(GONE))
    }

    /// Deletes one received report. One that is gone already is as good as deleted.
    pub(crate) async fn report_delete(&self, kind: ReportKind, id: &str) -> Result<()> {
        let method = format!("{}/set", object(kind));
        let args = json!({ "accountId": self.account_id(), "destroy": [id] });
        let responses = self.admin_request("delete reports", &[(&method, args)]).await?;
        let set = response(&responses, &method)?;
        match set["notDestroyed"].get(id) {
            Some(e) if e["type"] == "forbidden" => Err(set_refusal("delete reports", e)),
            _ => destroyed(set, id),
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_list_that_reached_the_most_read_is_said_to_be_cut() {
        assert!(!super::list_was_cut(super::super::LIST_MAX - 1));
        assert!(super::list_was_cut(super::super::LIST_MAX));
    }

    use super::*;

    #[test]
    fn enum_words_become_plain_words() {
        assert_eq!(spaced("startTlsNotSupported"), "start tls not supported");
        assert_eq!(word(&json!("unspecified")), "");
        assert_eq!(word(&json!("softFail")), "soft fail");
        assert_eq!(word(&Value::Null), "");
    }

    #[test]
    fn a_dmarc_record_passes_when_dkim_or_spf_does() {
        assert!(dmarc_passed(&json!({ "evaluatedDkim": "pass", "evaluatedSpf": "fail" })));
        assert!(dmarc_passed(&json!({ "evaluatedDkim": "fail", "evaluatedSpf": "pass" })));
        assert!(!dmarc_passed(&json!({ "evaluatedDkim": "fail", "evaluatedSpf": "fail" })));
        assert!(!dmarc_passed(&json!({})));
    }

    #[test]
    fn a_list_keyed_by_number_is_read_in_that_order() {
        let v = json!({ "10": "c", "2": "b", "1": "a" });
        let got: Vec<&str> = list(&v).into_iter().filter_map(Value::as_str).collect();
        assert_eq!(got, ["a", "b", "c"]);
    }
}
