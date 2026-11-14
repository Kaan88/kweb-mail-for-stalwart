//! Alerts and tenants. An alert is `{enable, condition: {match: [{if, then}], else},
//! emailAlert: Disabled | Enabled {fromName, fromAddress, to, subject, body},
//! eventAlert: Disabled | Enabled {eventMessage}}`; the condition is an expression such as
//! `metric('queue.count') > 5000`, which the server judges every five minutes
//! (`dev/notes/stalwart-webui.md`, section 8). The local server had no alert; the shape is
//! from the schema.

use serde_json::Value;

use super::{ServerAlert, Tenant};

/// The items of a list the server sends as a map keyed by position, or as a plain list.
fn items(value: &Value) -> Vec<&Value> {
    match value {
        Value::Object(map) => map.values().collect(),
        Value::Array(list) => list.iter().collect(),
        _ => Vec::new(),
    }
}

/// Addresses from a set (a map of address to true) or a list.
fn addresses(value: &Value) -> Vec<String> {
    match value {
        Value::Object(map) => map.keys().cloned().collect(),
        Value::Array(list) => list.iter().filter_map(|v| v.as_str().map(str::to_string)).collect(),
        _ => Vec::new(),
    }
}

fn metric_words(name: &str) -> String {
    match name {
        "queue.count" => "the number of queued messages".into(),
        "server.memory" => "the memory in use".into(),
        "user.count" => "the number of accounts".into(),
        "domain.count" => "the number of domains".into(),
        "delivery.active-connections" => "the number of active delivery connections".into(),
        _ => format!("the metric {}", name.replace(['.', '-'], " ")),
    }
}

fn number_words(n: f64) -> String {
    if n.fract() == 0.0 && n.abs() < 1e15 {
        format!("{}", n as i64)
    } else {
        format!("{n}")
    }
}

/// `metric('name') > 5000` in words; `None` for any other expression.
fn plain_comparison(expression: &str) -> Option<String> {
    let rest = expression.trim().strip_prefix("metric(")?.trim_start();
    let quote = rest.chars().next().filter(|c| *c == '\'' || *c == '"')?;
    let rest = &rest[1..];
    let end = rest.find(quote)?;
    let (name, rest) = (&rest[..end], rest[end + 1..].trim_start().strip_prefix(')')?.trim());
    let (operator, rest) =
        ["==", "!=", ">=", "<=", ">", "<"].iter().find_map(|op| rest.strip_prefix(op).map(|r| (*op, r)))?;
    let value: f64 = rest.trim().parse().ok()?;
    let words = match operator {
        ">" => "above",
        ">=" => "at least",
        "<" => "below",
        "<=" => "at most",
        "==" => "exactly",
        _ => "not",
    };
    Some(format!("{} is {words} {}", metric_words(name), number_words(value)))
}

/// When the alert fires, in words. What this cannot read it quotes.
fn condition_words(condition: &Value) -> String {
    let mut parts: Vec<String> = Vec::new();
    for m in items(&condition["match"]) {
        let expression = m["if"].as_str().unwrap_or_default().trim();
        if m["then"].as_str().map(str::trim).is_some_and(|t| t == "false") || expression.is_empty() {
            continue;
        }
        parts.push(plain_comparison(expression).unwrap_or_else(|| format!("{expression} holds")));
    }
    if parts.is_empty() {
        let other = condition["else"].as_str().unwrap_or_default().trim();
        return match other {
            "" | "false" => "Never".to_string(),
            _ => format!("When {} holds", plain_comparison(other).unwrap_or_else(|| other.to_string())),
        };
    }
    format!("When {}", parts.join(", or when "))
}

fn notify_words(alert: &Value) -> String {
    let mut ways: Vec<String> = Vec::new();
    if alert["emailAlert"]["@type"].as_str() == Some("Enabled") {
        let to = addresses(&alert["emailAlert"]["to"]);
        ways.push(if to.is_empty() { "sends an email".to_string() } else { format!("emails {}", to.join(", ")) });
    }
    if alert["eventAlert"]["@type"].as_str() == Some("Enabled") {
        ways.push("records an event on the server".to_string());
    }
    if ways.is_empty() {
        "it tells nobody".to_string()
    } else {
        format!("it {}", ways.join(" and "))
    }
}

/// An alert as the app shows it. `None` for one without an id.
pub fn alert_from(alert: &Value) -> Option<ServerAlert> {
    let id = alert["id"].as_str()?.to_string();
    let subject = alert["emailAlert"]["subject"].as_str().map(str::trim).filter(|s| !s.is_empty());
    Some(ServerAlert {
        name: subject.map_or_else(|| format!("Alert {id}"), str::to_string),
        is_enabled: alert["enable"].as_bool().unwrap_or(false),
        summary: format!("{}, {}.", condition_words(&alert["condition"]), notify_words(alert)),
        id,
    })
}

/// A tenant with its disk limit (0 or none: no limit) and use. `None` for one without an id.
pub fn tenant_from(tenant: &Value) -> Option<Tenant> {
    let id = tenant["id"].as_str()?.to_string();
    let quota = tenant["quotas"]["maxDiskQuota"].as_u64().filter(|q| *q > 0);
    Some(Tenant {
        name: tenant["name"].as_str().map(str::trim).filter(|n| !n.is_empty()).unwrap_or(&id).to_string(),
        quota,
        used: tenant["usedDiskQuota"].as_u64(),
        id,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_metric_comparison_is_in_words() {
        let alert = json!({
            "id": "a", "enable": true,
            "condition": {"match": {"0": {"if": "metric('queue.count') > 5000", "then": "true"}}, "else": "false"},
            "emailAlert": {"@type": "Enabled", "to": {"ops@example.test": true}, "subject": "Queue is long"},
            "eventAlert": {"@type": "Disabled"},
        });
        let a = alert_from(&alert).unwrap();
        assert_eq!(a.name, "Queue is long");
        assert_eq!(a.summary, "When the number of queued messages is above 5000, it emails ops@example.test.");
    }

    #[test]
    fn what_cannot_be_read_is_quoted() {
        let alert = json!({"id": "b", "enable": false, "condition": {"match": [{"if": "foo(1)", "then": "true"}], "else": "false"},
            "emailAlert": {"@type": "Disabled"}, "eventAlert": {"@type": "Enabled"}});
        let a = alert_from(&alert).unwrap();
        assert_eq!(a.name, "Alert b");
        assert!(!a.is_enabled);
        assert_eq!(a.summary, "When foo(1) holds, it records an event on the server.");
    }

    #[test]
    fn a_tenant_with_no_limit_has_none() {
        let t = tenant_from(&json!({"id": "t", "name": "Acme", "quotas": {"maxDiskQuota": 0}, "usedDiskQuota": 5}))
            .unwrap();
        assert_eq!((t.quota, t.used), (None, Some(5)));
    }
}
