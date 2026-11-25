//! The server's `x:Action` objects (the web admin's "Actions" page; see `dev/notes/stalwart-webui.md`
//! and `stalwart-admin-features.md`). What matters here:
//!
//! - An action is a create-only object: `x:Action/set` with `create: { id: { "@type": <variant>, ... } }`.
//!   The server does the work while it answers, and what it found comes back only in the `created` part
//!   of the answer (read on the local test server, 2026-11-25: `warnings` of a reload is a set, so a map
//!   of text to `true`; the checks answer objects like `{ "@type": "Fail", "details": ... }`; `rcptTo` is
//!   a set too; `elapsed` is a number of milliseconds).
//! - The key needs `sysActionCreate` and one `action<Variant>` permission for each variant.
//! - Nothing of a message that was pasted for a check, and no answer of the server, goes into an error:
//!   a refusal says what was refused in the core's own words.

use std::net::IpAddr;

use serde_json::{json, Map, Value};

use super::{set_refusal, shorten, text};
use crate::{
    ffi::{ActionOutcome, AuthResult, DmarcCheck, DmarcCheckResult, Maintenance, SpamCheck, SpamCheckResult, SpamTag},
    jmap::{
        send::{response, CREATE_ID},
        Connection,
    },
    Error, Result,
};

/// The largest message that is pasted for a check (the server's own request limit is far above this).
const MESSAGE_MAX: usize = 2_000_000;
/// Most recipients of one check.
const RCPT_MAX: usize = 50;

/// The variant name of a simple action: also the suffix of its permission (`action<Variant>`).
pub(crate) fn variant(what: Maintenance) -> &'static str {
    match what {
        Maintenance::ReloadSettings => "ReloadSettings",
        Maintenance::ReloadCertificates => "ReloadTlsCertificates",
        Maintenance::ReloadLookupStores => "ReloadLookupStores",
        Maintenance::ReloadBlockedIps => "ReloadBlockedIps",
        Maintenance::UpdateApps => "UpdateApps",
        Maintenance::InvalidateCaches => "InvalidateCaches",
        Maintenance::InvalidateNegativeCaches => "InvalidateNegativeCaches",
        Maintenance::PauseQueue => "PauseMtaQueue",
        Maintenance::ResumeQueue => "ResumeMtaQueue",
    }
}

/// Every simple action, in the order the screen lists them.
pub(crate) const SIMPLE: [Maintenance; 9] = [
    Maintenance::ReloadSettings,
    Maintenance::ReloadCertificates,
    Maintenance::ReloadLookupStores,
    Maintenance::ReloadBlockedIps,
    Maintenance::UpdateApps,
    Maintenance::InvalidateCaches,
    Maintenance::InvalidateNegativeCaches,
    Maintenance::PauseQueue,
    Maintenance::ResumeQueue,
];

/// The text of a set the server sends as a map of text to `true` (or as a list).
fn set_of_text(v: &Value) -> Vec<String> {
    let mut out: Vec<String> = match v {
        Value::Object(map) => map.keys().cloned().collect(),
        Value::Array(list) => list.iter().filter_map(Value::as_str).map(str::to_string).collect(),
        _ => Vec::new(),
    };
    out.sort();
    out
}

/// The result of an auth check in plain words. `{ "@type": "SoftFail", "details": "..." }`.
fn auth_result(v: &Value) -> AuthResult {
    let kind = v["@type"].as_str().unwrap_or("None");
    let outcome = match kind {
        "Pass" => "pass",
        "Fail" => "fail",
        "SoftFail" => "soft fail",
        "TempError" => "temporary error",
        "PermError" => "permanent error",
        "Neutral" => "neutral",
        "None" => "none",
        _ => "unknown",
    };
    AuthResult { outcome: outcome.to_string(), good: kind == "Pass", details: shorten(&text(v, "details")) }
}

/// A line break in a pasted message as the SMTP form: lone line feeds become CRLF.
fn crlf(message: &str) -> String {
    let mut out = String::with_capacity(message.len() + message.len() / 40);
    let mut last = '\0';
    for c in message.chars() {
        if c == '\n' && last != '\r' {
            out.push('\r');
        }
        out.push(c);
        last = c;
    }
    out
}

fn required(value: &str, what: &str) -> Result<String> {
    let v = value.trim();
    if v.is_empty() {
        Err(Error::invalid(format!("fill in {what}")))
    } else {
        Ok(v.to_string())
    }
}

fn ip(value: &str) -> Result<String> {
    let v = required(value, "the remote IP address")?;
    v.parse::<IpAddr>().map(|a| a.to_string()).map_err(|_| Error::invalid("the remote IP address is not valid"))
}

fn address(value: &str, what: &str) -> Result<String> {
    let v = required(value, what)?;
    if v.contains('@') && !v.contains(char::is_whitespace) {
        Ok(v)
    } else {
        Err(Error::invalid(format!("{what} is not an address")))
    }
}

/// The recipients as the server's set (a map of address to `true`); none is allowed when `must` is false.
fn recipients(value: &str, must: bool) -> Result<Value> {
    let mut set = Map::new();
    for part in value.split(|c: char| c == ',' || c == ';' || c.is_whitespace()).filter(|p| !p.is_empty()) {
        set.insert(address(part, "a recipient")?, Value::Bool(true));
    }
    if must && set.is_empty() {
        return Err(Error::invalid("fill in at least one recipient"));
    }
    if set.len() > RCPT_MAX {
        return Err(Error::invalid("too many recipients"));
    }
    Ok(Value::Object(set))
}

fn message_text(value: &str) -> Result<String> {
    if value.len() > MESSAGE_MAX {
        return Err(Error::invalid("the message is too large to check here"));
    }
    Ok(crlf(value))
}

impl Connection {
    /// Starts one action and returns what `created` says about it. The caller has validated the fields.
    async fn run_action_body(&self, body: Value) -> Result<Value> {
        let args = json!({ "accountId": self.account_id(), "create": { CREATE_ID: body } });
        let responses = self.admin_request("run that action", &[("x:Action/set", args)]).await?;
        let set = response(&responses, "x:Action/set")?;
        if let Some(e) = set["notCreated"].get(CREATE_ID) {
            return Err(set_refusal("run that action", e));
        }
        match set["created"].get(CREATE_ID) {
            Some(created) => Ok(created.clone()),
            None => Err(Error::temporary("the server gave no answer to the action")),
        }
    }

    /// A simple action. The warnings of a settings reload (licence problems and the like) are returned.
    pub(crate) async fn run_action(&self, what: Maintenance) -> Result<ActionOutcome> {
        let created = self.run_action_body(json!({ "@type": variant(what) })).await?;
        Ok(ActionOutcome { warnings: set_of_text(&created["warnings"]).iter().map(|w| shorten(w)).collect() })
    }

    pub(crate) async fn dmarc_check(&self, c: &DmarcCheck) -> Result<DmarcCheckResult> {
        let mut body = json!({
            "@type": "TroubleshootDmarc",
            "remoteIp": ip(&c.remote_ip)?,
            "ehloDomain": required(&c.ehlo_domain, "the EHLO domain")?,
            "mailFrom": address(&c.mail_from, "MAIL FROM")?,
            "spfEhloDomain": required(&c.spf_ehlo_domain, "the SPF EHLO domain")?,
            "spfMailFromDomain": required(&c.spf_mail_from_domain, "the SPF MAIL FROM domain")?,
        });
        let rcpt = recipients(&c.rcpt_to, false)?;
        if rcpt.as_object().is_some_and(|m| !m.is_empty()) {
            body["rcptTo"] = rcpt;
        }
        if !c.message.trim().is_empty() {
            body["message"] = json!(message_text(&c.message)?);
        }
        let r = self.run_action_body(body).await?;
        Ok(DmarcCheckResult {
            spf_ehlo: auth_result(&r["spfEhloResult"]),
            spf_mail_from: auth_result(&r["spfMailFromResult"]),
            dkim_pass: r["dkimPass"].as_bool().unwrap_or(false),
            dmarc: auth_result(&r["dmarcResult"]),
            dmarc_pass: r["dmarcPass"].as_bool().unwrap_or(false),
            dmarc_policy: text(&r, "dmarcPolicy"),
            elapsed_ms: r["elapsed"].as_f64().map_or(0, |n| n.max(0.0) as u64),
        })
    }

    pub(crate) async fn spam_check(&self, c: &SpamCheck) -> Result<SpamCheckResult> {
        if c.message.trim().is_empty() {
            return Err(Error::invalid("paste the message first"));
        }
        let mut body = json!({
            "@type": "ClassifySpam",
            "message": message_text(&c.message)?,
            "remoteIp": ip(&c.remote_ip)?,
            "ehloDomain": required(&c.ehlo_domain, "the EHLO domain")?,
            "mailFrom": address(&c.mail_from, "MAIL FROM")?,
            "rcptTo": recipients(&c.rcpt_to, true)?,
            "isTls": c.is_tls,
        });
        if !c.authenticated_as.trim().is_empty() {
            body["authenticatedAs"] = json!(c.authenticated_as.trim());
        }
        let r = self.run_action_body(body).await?;
        let mut tags: Vec<SpamTag> = r["tags"]
            .as_object()
            .into_iter()
            .flatten()
            .map(|(name, t)| SpamTag {
                name: name.clone(),
                score: t["score"].as_f64().unwrap_or(0.0),
                disposition: t["disposition"].as_str().unwrap_or("score").to_string(),
            })
            .collect();
        // What counts most first: a tag that rejects or discards, then the biggest scores.
        tags.sort_by(|a, b| {
            (b.disposition != "score")
                .cmp(&(a.disposition != "score"))
                .then(b.score.abs().total_cmp(&a.score.abs()))
                .then_with(|| a.name.cmp(&b.name))
        });
        Ok(SpamCheckResult {
            score: r["score"].as_f64().unwrap_or(0.0),
            result: text(&r, "result"),
            tags,
            ai_gate: text(&r, "aiGate"),
            ai_verdict: text(&r, "aiVerdict"),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_set_comes_as_a_map_or_a_list() {
        assert_eq!(set_of_text(&json!({ "b": true, "a": true })), ["a", "b"]);
        assert_eq!(set_of_text(&json!(["x"])), ["x"]);
        assert!(set_of_text(&json!({})).is_empty());
        assert!(set_of_text(&Value::Null).is_empty());
    }

    #[test]
    fn auth_results_are_put_in_words() {
        let r = auth_result(&json!({ "@type": "SoftFail", "details": "not aligned" }));
        assert_eq!((r.outcome.as_str(), r.good, r.details.as_str()), ("soft fail", false, "not aligned"));
        let r = auth_result(&json!({ "@type": "Pass" }));
        assert!(r.good && r.details.is_empty());
        assert_eq!(auth_result(&Value::Null).outcome, "none");
    }

    #[test]
    fn line_breaks_become_crlf_once() {
        assert_eq!(crlf("a\nb\r\nc"), "a\r\nb\r\nc");
    }

    #[test]
    fn fields_are_checked_before_anything_is_sent() {
        assert!(ip("192.0.2.1").is_ok() && ip("2001:db8::1").is_ok());
        assert!(ip("not an ip").is_err() && ip(" ").is_err());
        assert!(address("a@example.org", "x").is_ok() && address("example.org", "x").is_err());
        assert_eq!(recipients("a@x.org, b@x.org", true).unwrap().as_object().unwrap().len(), 2);
        assert!(recipients("", true).is_err());
        assert!(recipients("", false).unwrap().as_object().unwrap().is_empty());
        assert!(recipients("nobody", false).is_err());
    }
}
