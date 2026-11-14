//! The server's `PushSubscription` objects (RFC 8620, section 7.2) and the key it signs its
//! pushes with. Written against what Stalwart 1.1 does (`dev/notes/stalwart-webpush.md`,
//! `stalwart-fcm-experiment.md`).
//!
//! What goes into an error here is fixed words: never the push address, a key, a code or a
//! token. The server's own description is passed on only where it cannot repeat one of them.

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde_json::{json, Value};

use super::{send::response, Connection};
use crate::{Error, Result};

const VAPID_CAPABILITY: &str = "urn:ietf:params:jmap:webpush-vapid";
const EMAIL_PUSH_CAPABILITY: &str = "urn:ietf:params:jmap:emailpush";

/// One subscription as the server lists it (the push address and the keys are not readable).
#[derive(Debug, Clone, PartialEq)]
pub struct SubscriptionInfo {
    pub id: String,
    pub device_client_id: Option<String>,
    pub expires: Option<i64>,
}

/// What a new subscription is made of.
pub struct NewSubscription<'a> {
    pub device_client_id: &'a str,
    pub url: &'a str,
    pub p256dh: &'a str,
    pub auth: &'a str,
    pub types: &'a [&'a str],
    pub expires: i64,
    /// The JMAP account for which new mail is pushed (`emailPush`), with the properties and
    /// the urgency.
    pub account_id: &'a str,
    pub properties: &'a [&'a str],
    pub urgency: &'a str,
}

/// The server's answer to a verification code.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Verification {
    Accepted,
    /// The code is not the one the server sent (or was sent for another subscription).
    Refused,
    /// The server has no such subscription.
    Gone,
}

pub fn format_utc(secs: i64) -> String {
    chrono::DateTime::from_timestamp(secs, 0).unwrap_or_default().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

fn parse_utc(text: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(text).ok().map(|t| t.timestamp())
}

/// `key` as the unpadded base64url of a valid 65-byte uncompressed P-256 point; `None` for
/// anything else.
pub fn normalize_key(key: &str) -> Option<String> {
    let bytes = URL_SAFE_NO_PAD.decode(key.trim().trim_end_matches('=')).ok()?;
    if bytes.len() != 65 || bytes[0] != 4 {
        return None;
    }
    p256::PublicKey::from_sec1_bytes(&bytes).ok()?;
    Some(URL_SAFE_NO_PAD.encode(bytes))
}

impl Connection {
    fn push_capabilities(&self) -> Vec<&'static str> {
        if self.has_capability(EMAIL_PUSH_CAPABILITY) {
            vec![EMAIL_PUSH_CAPABILITY]
        } else {
            Vec::new()
        }
    }

    /// The server's `applicationServerKey`, from a session read anew (never the one this
    /// connection was made with): unpadded base64url of the 65-byte point. `None` when the
    /// session names none, or one that is not a valid point.
    pub async fn web_push_key(&self) -> Result<Option<String>> {
        let session = self.session_json().await?;
        let key = session["capabilities"][VAPID_CAPABILITY]["applicationServerKey"].as_str();
        Ok(key.and_then(normalize_key))
    }

    /// Every subscription of the user (or just `ids`), without the ones that expired.
    pub async fn push_subscriptions(&self, ids: Option<&[String]>) -> Result<Vec<SubscriptionInfo>> {
        let args = json!({ "ids": ids, "properties": ["id", "deviceClientId", "expires"] });
        let responses = self.call(&self.push_capabilities(), &[("PushSubscription/get", args)]).await?;
        let list = response(&responses, "PushSubscription/get")?["list"].as_array().cloned().unwrap_or_default();
        Ok(list
            .iter()
            .filter_map(|s| {
                Some(SubscriptionInfo {
                    id: s["id"].as_str()?.to_string(),
                    device_client_id: s["deviceClientId"].as_str().map(str::to_string),
                    expires: s["expires"].as_str().and_then(parse_utc),
                })
            })
            .collect())
    }

    /// Makes a subscription. Returns its id and the time the server settled on (it cuts a
    /// wish beyond seven days).
    pub async fn push_subscription_create(&self, new: &NewSubscription<'_>) -> Result<(String, Option<i64>)> {
        let mut email_push = serde_json::Map::new();
        email_push.insert(new.account_id.to_string(), json!({ "properties": new.properties, "urgency": new.urgency }));
        let create = json!({
            "deviceClientId": new.device_client_id,
            "url": new.url,
            "keys": { "p256dh": new.p256dh, "auth": new.auth },
            "types": new.types,
            "expires": format_utc(new.expires),
            "emailPush": email_push,
        });
        let args = json!({ "create": { "a": create } });
        let responses = self.call_keeping_errors(&self.push_capabilities(), &[("PushSubscription/set", args)]).await?;
        let set = response(&responses, "PushSubscription/set").map_err(|_| refused_create(&responses))?;
        if let Some(e) = set["notCreated"].get("a") {
            return Err(create_error(e));
        }
        let created = &set["created"]["a"];
        let Some(id) = created["id"].as_str() else {
            return Err(Error::temporary("the server's answer does not say whether the push subscription was made"));
        };
        Ok((id.to_string(), created["expires"].as_str().and_then(parse_utc)))
    }

    /// Extends the subscription to `expires` (the server cuts it to seven days). `false`: the
    /// server does not have it.
    pub async fn push_subscription_extend(&self, id: &str, expires: i64) -> Result<bool> {
        let args = json!({ "update": { id: { "expires": format_utc(expires) } } });
        let responses = self.call_keeping_errors(&self.push_capabilities(), &[("PushSubscription/set", args)]).await?;
        let set = response(&responses, "PushSubscription/set").map_err(|_| refused_update(&responses))?;
        match set["notUpdated"].get(id) {
            Some(e) if e["type"] == "notFound" => Ok(false),
            Some(_) => Err(Error::invalid("the server did not extend the push subscription")),
            None if set["updated"].get(id).is_some() => Ok(true),
            None => Err(Error::temporary("the server's answer does not say whether the subscription was extended")),
        }
    }

    /// Gives the server the code it sent to the push address.
    pub async fn push_subscription_verify(&self, id: &str, code: &str) -> Result<Verification> {
        let args = json!({ "update": { id: { "verificationCode": code } } });
        let responses = self.call_keeping_errors(&self.push_capabilities(), &[("PushSubscription/set", args)]).await?;
        let set = response(&responses, "PushSubscription/set").map_err(|_| refused_update(&responses))?;
        match set["notUpdated"].get(id) {
            Some(e) if e["type"] == "notFound" => Ok(Verification::Gone),
            Some(_) => Ok(Verification::Refused),
            None if set["updated"].get(id).is_some() => Ok(Verification::Accepted),
            None => Err(Error::temporary("the server's answer does not say whether the code was accepted")),
        }
    }

    /// Destroys these subscriptions. Returns the ids that are gone now (a subscription the
    /// server did not have counts); the others failed and are left for another try.
    pub async fn push_subscriptions_destroy(&self, ids: &[String]) -> Result<Vec<String>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let args = json!({ "destroy": ids });
        let responses = self.call_keeping_errors(&self.push_capabilities(), &[("PushSubscription/set", args)]).await?;
        let set = response(&responses, "PushSubscription/set").map_err(|_| refused_update(&responses))?;
        Ok(ids
            .iter()
            .filter(|id| match set["notDestroyed"].get(id.as_str()) {
                Some(e) => e["type"] == "notFound",
                None => true,
            })
            .cloned()
            .collect())
    }
}

/// A whole `PushSubscription/set` that failed: what no retry cures is `Invalid`.
fn method_failed(responses: &[(String, Value)], what: &str) -> Error {
    let kind = responses.iter().find(|(n, _)| n == "error").and_then(|(_, a)| a["type"].as_str()).unwrap_or("unknown");
    match kind {
        "forbidden" | "invalidArguments" | "unknownMethod" | "unknownCapability" => {
            Error::invalid(format!("the server does not take push subscriptions ({what})"))
        }
        _ => Error::temporary(format!("the server could not {what}")),
    }
}

fn refused_create(responses: &[(String, Value)]) -> Error {
    method_failed(responses, "make the push subscription")
}

fn refused_update(responses: &[(String, Value)]) -> Error {
    method_failed(responses, "change the push subscriptions")
}

/// A refused create. A refused address or keys are named in fixed words: the server's
/// description may repeat them.
fn create_error(e: &Value) -> Error {
    let properties: Vec<&str> = e["properties"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
    match e["type"].as_str().unwrap_or_default() {
        "overQuota" | "limit" => Error::invalid("this account has too many push subscriptions on other devices"),
        "invalidProperties" if properties.contains(&"url") => Error::invalid("the server refused the push address"),
        "invalidProperties" if properties.contains(&"keys") => Error::invalid("the server refused the push keys"),
        "invalidProperties" | "forbidden" => {
            let description = e["description"].as_str().filter(|d| !d.is_empty() && d.len() < 200);
            Error::invalid(description.unwrap_or("the server refused the push subscription").to_string())
        }
        _ => Error::temporary("the server could not make the push subscription"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_are_normalised_or_refused() {
        let key = "BFK3F4tWa25zdwbTkgyC9L_yRwldD0fr2vMBoD98bmgryXdluemPzdn7FJcwaSoKYDZq36hioQ_e7iJdeu0pat0";
        assert_eq!(normalize_key(key).as_deref(), Some(key));
        assert_eq!(normalize_key(&format!("{key}=")).as_deref(), Some(key));
        assert_eq!(normalize_key("").as_deref(), None);
        assert_eq!(normalize_key("AAAA"), None);
        // 65 bytes, but not a point on the curve.
        assert_eq!(normalize_key(&URL_SAFE_NO_PAD.encode([4u8; 65])), None);
    }

    #[test]
    fn utc_times_round_trip() {
        assert_eq!(format_utc(0), "1970-01-01T00:00:00Z");
        assert_eq!(parse_utc(&format_utc(1_800_000_000)), Some(1_800_000_000));
    }

    #[test]
    fn a_refused_create_names_no_address_or_key() {
        let e = json!({ "type": "invalidProperties", "description": "https://fcm.googleapis.com/fcm/send/SECRET is bad", "properties": ["url"] });
        assert!(!create_error(&e).to_string().contains("SECRET"));
        let e = json!({ "type": "invalidProperties", "description": "key SECRET", "properties": ["keys"] });
        assert!(!create_error(&e).to_string().contains("SECRET"));
    }
}
