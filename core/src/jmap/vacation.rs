//! The away message: the account's one `VacationResponse` (RFC 8621), over plain JMAP requests.
//!
//! What Stalwart 1.1.0 does, found against the real server: the object is `singleton` and can
//! be neither created nor destroyed; it takes `fromDate` and `toDate` as date-times and keeps
//! them as UTC to the second (an offset is turned into UTC, fractions of a second are dropped);
//! `isEnabled` with no text at all is accepted, which is why the core
//! refuses it; `null` clears a property.

use chrono::{DateTime, SecondsFormat};
use serde_json::{json, Value};

use super::{
    send::{response, updated},
    Connection,
};
use crate::{compose::rules::html_to_text, model::AwayMessage, Error, Result};

const VACATION_CAPABILITY: &str = "urn:ietf:params:jmap:vacationresponse";
const SINGLETON: &str = "singleton";

impl Connection {
    fn vacation_account_id(&self) -> String {
        self.account_for(VACATION_CAPABILITY)
    }

    /// The away message as the server has it. A server that has none to give is `Error::Invalid`.
    pub async fn away_message(&self) -> Result<AwayMessage> {
        self.needs_vacation()?;
        let args = json!({ "accountId": self.vacation_account_id() });
        let responses = self.call(&[VACATION_CAPABILITY], &[("VacationResponse/get", args)]).await?;
        let get = response(&responses, "VacationResponse/get")?;
        let one = get["list"].get(0).ok_or_else(|| Error::temporary("the server has no away message to give"))?;
        Ok(away_from(one))
    }

    /// Switches the away message on or off and sets its dates, subject and text. A refused
    /// message is refused before the server is asked; a subject and text the user did not change
    /// are left as they are, so that formatting another client wrote stays.
    pub async fn set_away_message(&self, message: &AwayMessage) -> Result<()> {
        self.needs_vacation()?;
        check(message)?;
        let patch = away_patch(message, &self.away_message().await?)?;
        let args = json!({ "accountId": self.vacation_account_id(), "update": { SINGLETON: patch } });
        let responses = self.call(&[VACATION_CAPABILITY], &[("VacationResponse/set", args)]).await?;
        updated(response(&responses, "VacationResponse/set")?, SINGLETON, "the server has no away message")
    }

    fn needs_vacation(&self) -> Result<()> {
        match self.has_capability(VACATION_CAPABILITY) {
            true => Ok(()),
            false => Err(Error::invalid("the server does not support an away message")),
        }
    }
}

fn parse_date(value: &Value) -> Option<i64> {
    DateTime::parse_from_rfc3339(value.as_str()?).ok().map(|at| at.timestamp())
}

/// The text of a message that has only HTML is the HTML as text.
fn away_from(v: &Value) -> AwayMessage {
    let text = match (v["textBody"].as_str(), v["htmlBody"].as_str()) {
        (Some(text), _) if !text.trim().is_empty() => text.to_string(),
        (_, Some(html)) => html_to_text(html),
        _ => String::new(),
    };
    AwayMessage {
        is_enabled: v["isEnabled"] == true,
        from: parse_date(&v["fromDate"]),
        until: parse_date(&v["toDate"]),
        subject: v["subject"].as_str().unwrap_or_default().to_string(),
        text,
    }
}

/// A UTC date-time as the server writes it.
fn format_date(secs: i64) -> Result<String> {
    let at = DateTime::from_timestamp(secs, 0).ok_or_else(|| Error::invalid("That date is out of range."))?;
    Ok(at.to_rfc3339_opts(SecondsFormat::Secs, true))
}

/// The dates of `message` as the server writes them, or what is wrong with the message.
fn check(message: &AwayMessage) -> Result<(Option<String>, Option<String>)> {
    let dates = (message.from.map(format_date).transpose()?, message.until.map(format_date).transpose()?);
    if message.from.zip(message.until).is_some_and(|(from, until)| until <= from) {
        return Err(Error::invalid("The away message has to end after it starts."));
    }
    if message.is_enabled && message.text.trim().is_empty() {
        return Err(Error::invalid("Write the message that is sent while you are away."));
    }
    Ok(dates)
}

/// The update that makes the away message what `message` says, or what is wrong with it.
/// `current` is what the server has: the subject and the text are written only when they
/// differ from it. The HTML version is cleared with a text that is written, so that the two
/// cannot disagree; what is empty is cleared.
fn away_patch(message: &AwayMessage, current: &AwayMessage) -> Result<Value> {
    let (from, until) = check(message)?;
    let (subject, text) = (message.subject.trim(), message.text.trim());
    let some = |s: &str| if s.is_empty() { Value::Null } else { json!(s) };
    let mut patch = json!({ "isEnabled": message.is_enabled, "fromDate": from, "toDate": until });
    if subject != current.subject.trim() {
        patch["subject"] = some(subject);
    }
    if text != current.text.trim() {
        patch["textBody"] = some(text);
        patch["htmlBody"] = Value::Null;
    }
    Ok(patch)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message(enabled: bool, from: Option<i64>, until: Option<i64>, text: &str) -> AwayMessage {
        AwayMessage { is_enabled: enabled, from, until, subject: " Away ".into(), text: text.into() }
    }

    #[test]
    fn the_answer_of_the_server_is_read_with_dates_in_utc() {
        let on = away_from(&json!({
            "id": "singleton", "isEnabled": true, "fromDate": "2026-12-01T00:00:00Z",
            "toDate": "2026-12-10T12:30:00+02:00", "subject": "Away", "textBody": "I am away.", "htmlBody": null
        }));
        assert_eq!(
            on,
            AwayMessage {
                is_enabled: true,
                from: Some(1_796_083_200),
                until: Some(1_796_898_600),
                subject: "Away".into(),
                text: "I am away.".into()
            }
        );
        // What the server has when it was never set.
        let off = away_from(&json!({
            "id": "singleton", "isEnabled": false, "fromDate": null, "toDate": null,
            "subject": null, "textBody": null, "htmlBody": null
        }));
        assert_eq!(
            off,
            AwayMessage { is_enabled: false, from: None, until: None, subject: String::new(), text: String::new() }
        );
        assert_eq!(away_from(&json!({ "fromDate": "garbage" })).from, None);
    }

    #[test]
    fn a_message_with_only_html_is_read_as_text() {
        let m =
            away_from(&json!({ "isEnabled": true, "textBody": null, "htmlBody": "<p>Back <b>soon</b></p><p>Ann</p>" }));
        assert_eq!(m.text, "Back soon\nAnn");
        let both = away_from(&json!({ "textBody": "plain", "htmlBody": "<p>html</p>" }));
        assert_eq!(both.text, "plain");
        assert_eq!(away_from(&json!({ "textBody": "  ", "htmlBody": "<p>html</p>" })).text, "html");
    }

    /// What the server has when the away message was never set.
    fn blank() -> AwayMessage {
        AwayMessage { is_enabled: false, from: None, until: None, subject: String::new(), text: String::new() }
    }

    #[test]
    fn the_update_is_exact_and_clears_the_html() {
        let patch =
            away_patch(&message(true, Some(1_796_083_200), Some(1_796_898_600), " I am away.\n "), &blank()).unwrap();
        assert_eq!(
            patch,
            json!({
                "isEnabled": true, "fromDate": "2026-12-01T00:00:00Z", "toDate": "2026-12-10T10:30:00Z",
                "subject": "Away", "textBody": "I am away.", "htmlBody": null
            })
        );
        // Off, with nothing else, over a message that had a subject and text: both are cleared.
        let mut off = message(false, None, None, "");
        off.subject = String::new();
        let had = message(true, None, None, "Old text");
        assert_eq!(
            away_patch(&off, &had).unwrap(),
            json!({ "isEnabled": false, "fromDate": null, "toDate": null, "subject": null, "textBody": null, "htmlBody": null })
        );
        // Off keeps the text the user wrote, for the next time.
        assert_eq!(away_patch(&message(false, None, None, "Later"), &blank()).unwrap()["textBody"], "Later");
    }

    #[test]
    fn a_subject_and_text_that_did_not_change_are_not_written() {
        let current = AwayMessage { subject: "Away".into(), ..message(true, Some(100), Some(200), "I am away.") };
        // Only the switch and the dates are sent: the HTML version another client wrote stays.
        let same = message(false, Some(100), Some(300), " I am away.\n");
        assert_eq!(
            away_patch(&same, &current).unwrap(),
            json!({ "isEnabled": false, "fromDate": "1970-01-01T00:01:40Z", "toDate": "1970-01-01T00:05:00Z" })
        );
        let new_text = away_patch(&message(true, Some(100), Some(200), "New"), &current).unwrap();
        assert_eq!((&new_text["textBody"], &new_text["htmlBody"]), (&json!("New"), &Value::Null));
        assert!(new_text.get("subject").is_none());
    }

    #[test]
    fn it_must_end_after_it_starts_and_an_enabled_message_needs_text() {
        let refused = |m: AwayMessage| match check(&m) {
            Err(Error::Invalid { msg }) => msg,
            other => panic!("expected a refusal, got {other:?}"),
        };
        assert_eq!(refused(message(true, Some(100), Some(100), "x")), "The away message has to end after it starts.");
        assert_eq!(refused(message(false, Some(100), Some(50), "x")), "The away message has to end after it starts.");
        assert_eq!(refused(message(true, None, None, " \n")), "Write the message that is sent while you are away.");
        assert_eq!(refused(message(true, Some(i64::MAX), None, "x")), "That date is out of range.");
        assert!(check(&message(true, Some(100), Some(101), "x")).is_ok());
        assert!(check(&message(true, None, Some(50), "x")).is_ok(), "one date alone is fine");
        assert!(check(&message(false, None, None, "")).is_ok(), "off needs no text");
    }
}
