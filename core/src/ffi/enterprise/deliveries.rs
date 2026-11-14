//! A delivery trace of the server in the app's words. A trace is
//! `{id, timestamp, from, to, size, events}`; `events` is a map "0", "1", ... of
//! `{event, timestamp, keyValues}`, and `keyValues` a map of `{key, value: {@type, value}}`
//! (`dev/notes/stalwart-enterprise-observed.md`). The event names are the server's own list.

use serde_json::Value;

use super::{history::parse_time, Delivery, DeliveryDetail, DeliveryStatus, DeliveryStep};

/// A recipient the other server accepted (the server's report of it, and the event of the
/// accepting reply).
const SUCCESS: [&str; 2] = ["delivery.dsn-success", "delivery.delivered"];
/// The server is done with the message: it left the queue. Also so when every recipient failed
/// or ran out of time, so on its own it counts as delivered only if nothing else says otherwise.
const COMPLETED: &str = "delivery.completed";
/// A recipient given up on for good: its report, the failure of one that ran out of time (the
/// server logs `delivery.failed` when a recipient expired), and a report that could not go out.
const PERMANENT: [&str; 3] = ["delivery.dsn-perm-fail", "delivery.double-bounce", "delivery.failed"];
/// A recipient that is still in the queue: the server logs `queue.rescheduled` with the next
/// try whenever an attempt leaves one behind. `delivery.dsn-temp-fail` is only logged once the
/// delay report is due, usually hours later.
const RETRY: [&str; 2] = ["queue.rescheduled", "delivery.dsn-temp-fail"];

/// One event as read: its name, time and key values in the server's order.
struct Event {
    name: String,
    at: i64,
    values: Vec<(String, Value)>,
}

/// The events of a trace in order. The server sends a map keyed by position; a plain list is
/// read too.
fn events_of(trace: &Value) -> Vec<Event> {
    let raw = &trace["events"];
    let mut found: Vec<(u64, &Value)> = match raw {
        Value::Object(map) => map.iter().enumerate().map(|(i, (k, v))| (k.parse().unwrap_or(i as u64), v)).collect(),
        Value::Array(list) => list.iter().enumerate().map(|(i, v)| (i as u64, v)).collect(),
        _ => Vec::new(),
    };
    found.sort_by_key(|(i, _)| *i);
    let fallback = trace["timestamp"].as_str().and_then(parse_time).unwrap_or(0);
    found
        .into_iter()
        .filter_map(|(_, e)| {
            let name = e["event"].as_str()?.to_string();
            let at = e["timestamp"].as_str().and_then(parse_time).unwrap_or(fallback);
            let pairs: Vec<&Value> = match &e["keyValues"] {
                Value::Object(map) => map.values().collect(),
                Value::Array(list) => list.iter().collect(),
                _ => Vec::new(),
            };
            let values = pairs
                .into_iter()
                .filter_map(|kv| Some((kv["key"].as_str()?.to_string(), kv["value"].clone())))
                .collect();
            Some(Event { name, at, values })
        })
        .collect()
}

/// What the events say about this one attempt (a trace is an attempt, not a message):
/// - a permanent failure anywhere: Failed;
/// - else a success with no retry marker after the last of them: Delivered (a retry marker after
///   it means another recipient is still waiting: partly delivered is Retrying);
/// - else any retry marker: Retrying;
/// - else a bare `delivery.completed`: Delivered;
/// - else Unknown.
fn status_of(events: &[Event]) -> DeliveryStatus {
    let named = |names: &[&str], e: &Event| names.contains(&e.name.as_str());
    if events.iter().any(|e| named(&PERMANENT, e)) {
        return DeliveryStatus::Failed;
    }
    let last_success = events.iter().rposition(|e| named(&SUCCESS, e));
    let last_retry = events.iter().rposition(|e| named(&RETRY, e));
    match (last_success, last_retry) {
        (Some(good), Some(retry)) if retry > good => DeliveryStatus::Retrying,
        (Some(_), _) => DeliveryStatus::Delivered,
        (None, Some(_)) => DeliveryStatus::Retrying,
        (None, None) if events.iter().any(|e| e.name == COMPLETED) => DeliveryStatus::Delivered,
        (None, None) => DeliveryStatus::Unknown,
    }
}

/// Addresses from the trace's `to`: a text (one address, or several joined with "; " as the
/// server writes a list, or with commas) or a list.
fn addresses(value: &Value) -> Vec<String> {
    let clean = |s: &str| s.trim().to_string();
    let found: Vec<String> = match value {
        Value::String(s) => s.split([';', ',']).map(clean).collect(),
        Value::Array(list) => list.iter().filter_map(|v| v.as_str().map(clean)).collect(),
        Value::Object(map) => map.keys().map(|k| clean(k)).collect(),
        _ => Vec::new(),
    };
    found.into_iter().filter(|a| !a.is_empty()).collect()
}

/// The recipients: the list the first event (the start of the attempt) carries, as the server
/// sends it, else the trace's own `to` text.
fn recipients(trace: &Value, events: &[Event]) -> Vec<String> {
    let listed: Option<Vec<String>> = events
        .first()
        .and_then(|e| lookup(e, "to"))
        .map(|v| v.get("value").unwrap_or(v))
        .and_then(Value::as_array)
        .map(|list| list.iter().filter_map(text_of).map(|a| a.trim().to_string()).filter(|a| !a.is_empty()).collect());
    match listed {
        Some(list) if !list.is_empty() => list,
        _ => addresses(&trace["to"]),
    }
}

/// The message as the list shows it. `None` for a trace without an id.
pub fn delivery_from(trace: &Value) -> Option<Delivery> {
    let id = trace["id"].as_str()?.to_string();
    let events = events_of(trace);
    let at = trace["timestamp"].as_str().and_then(parse_time).or_else(|| events.first().map(|e| e.at)).unwrap_or(0);
    Some(Delivery {
        id,
        from: trace["from"].as_str().unwrap_or_default().trim().to_string(),
        to: recipients(trace, &events),
        at,
        size: trace["size"].as_u64().unwrap_or(0),
        status: status_of(&events),
    })
}

pub fn detail_from(trace: &Value) -> Option<DeliveryDetail> {
    let delivery = delivery_from(trace)?;
    let steps = events_of(trace).iter().map(step).collect();
    Some(DeliveryDetail { delivery, steps })
}

// ---- words ----

/// The title of an event. An event not in this table shows its name made readable.
fn title(event: &str) -> String {
    let known = match event {
        "delivery.attempt-start" => "Delivery attempt started",
        "delivery.attempt-end" => "Delivery attempt ended",
        "delivery.domain-delivery-start" => "Delivery to the domain started",
        "delivery.completed" => "Delivery finished",
        "delivery.failed" => "Gave up: the message expired",
        "delivery.mx-lookup" => "Found the domain's mail servers",
        "delivery.mx-lookup-failed" => "Could not look up the domain's mail servers",
        "delivery.ip-lookup" => "Found the address of their server",
        "delivery.ip-lookup-failed" => "Could not find the address of their server",
        "delivery.null-mx" => "The domain says it takes no mail",
        "delivery.missing-outbound-hostname" => "The server has no name to introduce itself with",
        "delivery.connect" => "Connected to their server",
        "delivery.connect-error" => "Could not connect to their server",
        "delivery.greeting-failed" => "Their server's greeting was refused",
        "delivery.ehlo" => "Introduced ourselves to their server",
        "delivery.ehlo-rejected" => "Their server refused our introduction",
        "delivery.auth" => "Signed in to their server",
        "delivery.auth-failed" => "Their server refused the sign-in",
        "delivery.mail-from" => "Told their server who the message is from",
        "delivery.mail-from-rejected" => "Their server refused the sender",
        "delivery.delivered" => "Accepted by their server",
        "delivery.rcpt-to" => "Told their server who the message is for",
        "delivery.rcpt-to-rejected" => "Their server refused the recipient",
        "delivery.rcpt-to-failed" => "Their server could not take the recipient just now",
        "delivery.message-rejected" => "Their server refused the message",
        "delivery.start-tls" => "Switched to an encrypted connection",
        "delivery.start-tls-unavailable" => "Their server offers no encryption",
        "delivery.start-tls-error" => "The encrypted connection could not be made",
        "delivery.start-tls-disabled" => "Encryption is switched off for this server",
        "delivery.implicit-tls-error" => "The encrypted connection could not be made",
        "delivery.concurrency-limit-exceeded" => "Too many connections to this server at once",
        "delivery.rate-limit-exceeded" => "Too many messages to this server in a short time",
        "delivery.double-bounce" => "The report of the failure could not be delivered either",
        "delivery.dsn-success" => "Report sent: delivered",
        "delivery.dsn-temp-fail" => "Report sent: delayed, the server will try again",
        "delivery.dsn-perm-fail" => "Report sent: could not be delivered",
        "delivery.raw-input" => "Their server said",
        "delivery.raw-output" => "This server said",
        "message-ingest.ham" => "Stored in a mailbox",
        "message-ingest.spam" => "Stored as junk",
        "queue.message-queued" => "Put in the queue",
        "queue.authenticated-message-queued" => "Put in the queue (sent by an account)",
        "queue.report-queued" => "Report put in the queue",
        "queue.dsn-queued" => "Delivery report put in the queue",
        "queue.rescheduled" => "Scheduled for another try",
        "smtp.connection-start" => "A server connected",
        _ => "",
    };
    if !known.is_empty() {
        return known.to_string();
    }
    let words = event.replace(['.', '-', '_'], " ");
    let mut chars = words.chars();
    chars.next().map(|c| c.to_uppercase().chain(chars).collect()).unwrap_or_default()
}

/// A time span the server gave in milliseconds, in words.
fn elapsed_words(ms: f64) -> String {
    if ms < 1000.0 {
        format!("{} ms", ms.round() as u64)
    } else if ms < 60_000.0 {
        let secs = (ms / 100.0).round() / 10.0;
        if secs.fract() == 0.0 {
            format!("{} seconds", secs as u64)
        } else {
            format!("{secs} seconds")
        }
    } else {
        let total = (ms / 1000.0).round() as u64;
        format!("{} min {} s", total / 60, total % 60)
    }
}

/// A value as text: strings and numbers as they are, a list joined with commas, a nested event
/// as its own name. `None` for what has no text (null).
fn text_of(value: &Value) -> Option<String> {
    let inner = value.get("value").unwrap_or(value);
    match inner {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        Value::Array(list) => Some(list.iter().filter_map(text_of).collect::<Vec<_>>().join(",")),
        _ => value["event"].as_str().map(str::to_string),
    }
}

fn lookup<'a>(event: &'a Event, key: &str) -> Option<&'a Value> {
    event.values.iter().find(|(k, _)| k == key).map(|(_, v)| v)
}

fn text_for(event: &Event, key: &str) -> Option<String> {
    lookup(event, key).and_then(text_of).filter(|t| !t.is_empty())
}

/// The useful part of an event: where it was about (domain, recipients, host), what the other
/// server said and how long it took.
fn detail(event: &Event) -> String {
    let mut parts: Vec<String> = Vec::new();
    if let Some(domain) = text_for(event, "domain") {
        parts.push(domain);
    }
    if let Some(to) = text_for(event, "to") {
        parts.push(format!("for {to}"));
    }
    let host = text_for(event, "hostname").or_else(|| text_for(event, "host")).or_else(|| text_for(event, "remoteIp"));
    if let Some(host) = host {
        parts.push(host);
    }
    let answer: Vec<String> =
        ["code", "details", "reason", "response", "error"].iter().filter_map(|k| text_for(event, k)).collect();
    if !answer.is_empty() {
        parts.push(format!("they said {}", answer.join(" ")));
    }
    if let Some(ms) = lookup(event, "elapsed").and_then(|v| v.get("value").unwrap_or(v).as_f64()) {
        parts.push(format!("took {}", elapsed_words(ms)));
    }
    parts.join("; ")
}

/// `event key=value key=value`; a value with a space is in quotes.
fn raw(event: &Event) -> String {
    let mut line = event.name.clone();
    for (key, value) in &event.values {
        let text = text_of(value).unwrap_or_default();
        if text.contains(char::is_whitespace) || text.is_empty() {
            line.push_str(&format!(" {key}=\"{}\"", text.replace('"', "'")));
        } else {
            line.push_str(&format!(" {key}={text}"));
        }
    }
    line
}

fn step(event: &Event) -> DeliveryStep {
    DeliveryStep { at: event.at, title: title(&event.name), detail: detail(event), raw: raw(event) }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use serde_json::json;

    fn status_of_trace(trace: &Value) -> DeliveryStatus {
        status_of(&events_of(trace))
    }

    fn trace(names: &[&str]) -> Value {
        let events: BTreeMap<String, Value> = names
            .iter()
            .enumerate()
            .map(|(i, n)| (i.to_string(), json!({"event": n, "timestamp": "2026-11-14T10:00:00Z", "keyValues": {}})))
            .collect();
        json!({"id": "t", "events": events})
    }

    #[test]
    fn status_follows_the_events() {
        assert_eq!(
            status_of_trace(&trace(&["delivery.attempt-start", "delivery.completed"])),
            DeliveryStatus::Delivered
        );
        assert_eq!(status_of_trace(&trace(&["delivery.dsn-temp-fail"])), DeliveryStatus::Retrying);
        // What the server sends for an ordinary failed attempt that will be retried: no report yet.
        assert_eq!(
            status_of_trace(&trace(&[
                "delivery.attempt-start",
                "delivery.connect-error",
                "queue.rescheduled",
                "delivery.attempt-end"
            ])),
            DeliveryStatus::Retrying
        );
        // Two recipients, one accepted and one deferred in the same attempt.
        assert_eq!(
            status_of_trace(&trace(&[
                "delivery.attempt-start",
                "delivery.dsn-success",
                "queue.rescheduled",
                "delivery.attempt-end"
            ])),
            DeliveryStatus::Retrying
        );
        // A recipient that ran out of time: gave up, not "will try again".
        assert_eq!(
            status_of_trace(&trace(&["delivery.attempt-start", "delivery.failed", "delivery.completed"])),
            DeliveryStatus::Failed
        );
        assert_eq!(
            status_of_trace(&trace(&["delivery.dsn-temp-fail", "delivery.dsn-success"])),
            DeliveryStatus::Delivered
        );
        assert_eq!(
            status_of_trace(&trace(&["delivery.dsn-success", "delivery.dsn-temp-fail"])),
            DeliveryStatus::Retrying
        );
        assert_eq!(status_of_trace(&trace(&["delivery.dsn-perm-fail", "delivery.completed"])), DeliveryStatus::Failed);
        assert_eq!(status_of_trace(&trace(&["delivery.attempt-start"])), DeliveryStatus::Unknown);
    }

    #[test]
    fn several_recipients_the_server_joined_are_told_apart() {
        let joined = json!({"id": "t", "to": "a@x.test; b@y.test", "events": {}});
        assert_eq!(delivery_from(&joined).unwrap().to, ["a@x.test", "b@y.test"]);
        let listed = json!({"id": "t", "to": "a@x.test; b@y.test", "events": { "0": {
            "event": "delivery.attempt-start", "keyValues": { "0": { "key": "to", "value": {
                "@type": "List", "value": [{"@type": "String", "value": "a@x.test"}, {"@type": "String", "value": "b@y.test"}] } } } } } });
        assert_eq!(delivery_from(&listed).unwrap().to, ["a@x.test", "b@y.test"]);
    }

    #[test]
    fn unknown_events_get_readable_titles() {
        assert_eq!(title("delivery.mx-lookup-slow"), "Delivery mx lookup slow");
        assert_eq!(title("delivery.delivered"), "Accepted by their server");
    }

    #[test]
    fn elapsed_is_in_words() {
        assert_eq!(elapsed_words(340.0), "340 ms");
        assert_eq!(elapsed_words(1200.0), "1.2 seconds");
        assert_eq!(elapsed_words(2000.0), "2 seconds");
        assert_eq!(elapsed_words(125_000.0), "2 min 5 s");
    }
}
