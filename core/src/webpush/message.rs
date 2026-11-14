//! What a decrypted push message is. Only the shape matters here; a body that is not JSON, or
//! not about this account, is `Other`.

use serde_json::Value;

/// A decrypted message, as far as the core acts on it.
#[derive(Debug, Clone, PartialEq)]
pub enum Incoming {
    /// The server wants the code back to prove the endpoint is the user's.
    Verification {
        subscription_id: String,
        code: String,
    },
    /// New mail for this account (an EmailPush). Nothing else of it is used.
    Mail,
    /// A StateChange that names this account.
    Changed,
    CalendarAlert {
        event_id: String,
        alert_id: String,
        recurrence_id: Option<String>,
    },
    Other,
}

fn text(value: &Value) -> Option<String> {
    value.as_str().filter(|s| !s.is_empty()).map(str::to_string)
}

pub fn parse(plain: &[u8], account_id: &str) -> Incoming {
    let Ok(value) = serde_json::from_slice::<Value>(plain) else { return Incoming::Other };
    let ours = value["accountId"].as_str() == Some(account_id);
    match value["@type"].as_str() {
        Some("PushVerification") => match (text(&value["pushSubscriptionId"]), text(&value["verificationCode"])) {
            (Some(subscription_id), Some(code)) => Incoming::Verification { subscription_id, code },
            _ => Incoming::Other,
        },
        Some("EmailPush") if ours => Incoming::Mail,
        Some("StateChange") if value["changed"].get(account_id).is_some() => Incoming::Changed,
        Some("CalendarAlert") if ours || value.get("accountId").is_none() => {
            match (text(&value["calendarEventId"]), text(&value["alertId"])) {
                (Some(event_id), Some(alert_id)) => {
                    Incoming::CalendarAlert { event_id, alert_id, recurrence_id: text(&value["recurrenceId"]) }
                }
                _ => Incoming::Other,
            }
        }
        _ => Incoming::Other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_kind_is_recognised() {
        let p = |s: &str| parse(s.as_bytes(), "acc");
        assert_eq!(
            p(r#"{"@type":"PushVerification","pushSubscriptionId":"b","verificationCode":"c"}"#),
            Incoming::Verification { subscription_id: "b".into(), code: "c".into() }
        );
        assert_eq!(p(r#"{"@type":"EmailPush","accountId":"acc","emails":[],"state":"s"}"#), Incoming::Mail);
        assert_eq!(p(r#"{"@type":"EmailPush","accountId":"other"}"#), Incoming::Other);
        assert_eq!(p(r#"{"@type":"StateChange","changed":{"acc":{"Email":"1"}}}"#), Incoming::Changed);
        assert_eq!(p(r#"{"@type":"StateChange","changed":{"other":{"Email":"1"}}}"#), Incoming::Other);
        assert_eq!(
            p(r#"{"@type":"CalendarAlert","accountId":"acc","calendarEventId":"e","alertId":"a","recurrenceId":null}"#),
            Incoming::CalendarAlert { event_id: "e".into(), alert_id: "a".into(), recurrence_id: None }
        );
        assert_eq!(p(r#"{"@type":"PushVerification","pushSubscriptionId":"b"}"#), Incoming::Other);
        assert_eq!(p("not json"), Incoming::Other);
        assert_eq!(p(r#"{"@type":"Something"}"#), Incoming::Other);
    }
}
