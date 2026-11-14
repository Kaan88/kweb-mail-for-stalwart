//! The mock's `PushSubscription` methods and the Web Push messages the server would send, as
//! Stalwart 1.1 does them (`dev/notes/stalwart-webpush.md`, `stalwart-fcm-experiment.md`):
//!
//! - The url must be https; it cannot be changed. The keys are checked (a valid P-256 point and
//!   16 bytes of auth). Types are checked against the list the server knows.
//! - `expires` defaults to seven days and is cut to seven days, on create and on update. An
//!   expired subscription is no longer listed.
//! - At most `max` (15) subscriptions. Ids are short and reused after a destroy.
//! - `get` does not read `url` or `keys` (`forbidden`), and the verification code only
//!   through the message. A create sends one `PushVerification` (encrypted for the new
//!   subscription's keys), but the server sends at most one verification per minute per
//!   account: inside that time the new subscription gets none and stays unverified for good.
//! - A wrong verification code is `invalidProperties` on `verificationCode`.
//!
//! "Sending" means the message is put in a list (`Delivery`), encrypted the way the server
//! does it (RFC 8291, one record, padded to a multiple of 128 bytes); the tests hand the list
//! to the core the way Play services would.

use std::{
    collections::BTreeMap,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes128Gcm, Nonce,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use hkdf::Hkdf;
use p256::{
    ecdh::diffie_hellman,
    elliptic_curve::sec1::{FromEncodedPoint, ToEncodedPoint},
    EncodedPoint, PublicKey, SecretKey,
};
use serde_json::{json, Map, Value};
use sha2::Sha256;

use super::state::{MethodError, ACCOUNT_ID};

pub const CAPABILITY: &str = "urn:ietf:params:jmap:webpush-vapid";
const TYPES: [&str; 8] =
    ["Email", "Mailbox", "CalendarAlert", "Thread", "EmailDelivery", "Identity", "Core", "EmailSubmission"];
const EMAIL_PROPERTIES: [&str; 8] =
    ["id", "threadId", "mailboxIds", "keywords", "receivedAt", "subject", "from", "preview"];
const WEEK: i64 = 7 * 24 * 3600;

/// A message the server sent to Google for a subscription: the encrypted body.
#[derive(Debug, Clone)]
pub struct Delivery {
    pub subscription_id: String,
    pub url: String,
    pub body: Vec<u8>,
}

/// A subscription as the tests see it.
#[derive(Debug, Clone, PartialEq)]
pub struct SubView {
    pub id: String,
    pub device_client_id: Option<String>,
    pub url: String,
    pub expires: i64,
    pub types: Vec<String>,
    pub verified: bool,
    pub email_push: Value,
    pub p256dh: Vec<u8>,
    pub auth: Vec<u8>,
}

struct Sub {
    id: String,
    device_client_id: Option<String>,
    url: String,
    p256dh: Vec<u8>,
    auth: Vec<u8>,
    types: Vec<String>,
    expires: i64,
    code: String,
    verified: bool,
    email_push: Value,
}

pub struct PushSubs {
    /// What the session names as `applicationServerKey`; `None`: no such capability.
    pub key: Option<String>,
    server_secret: SecretKey,
    subs: BTreeMap<String, Sub>,
    pub max: usize,
    verify_gap: Duration,
    last_verification: Option<Instant>,
    pub deliveries: Vec<Delivery>,
    /// The arguments of every `PushSubscription/set`, as received.
    pub sets: Vec<Value>,
    /// With a `(type, property)`, every create is refused with it.
    pub refuse_create: Option<(&'static str, &'static str)>,
    /// How many more requests that touch PushSubscription are answered with a 500.
    pub fail_next: usize,
    /// A code that was written back and accepted, per subscription id.
    pub accepted_codes: Vec<(String, String)>,
    next_code: u64,
    /// Seconds added to the server's clock.
    pub clock_offset: i64,
}

fn random_secret() -> SecretKey {
    loop {
        let mut bytes = [0u8; 32];
        getrandom::fill(&mut bytes).unwrap();
        if let Ok(key) = SecretKey::from_slice(&bytes) {
            return key;
        }
    }
}

fn point_text(secret: &SecretKey) -> String {
    URL_SAFE_NO_PAD.encode(secret.public_key().to_encoded_point(false).as_bytes())
}

impl Default for PushSubs {
    fn default() -> Self {
        let server_secret = random_secret();
        PushSubs {
            key: Some(point_text(&server_secret)),
            server_secret,
            subs: BTreeMap::new(),
            max: 15,
            verify_gap: Duration::from_secs(60),
            last_verification: None,
            deliveries: Vec::new(),
            sets: Vec::new(),
            refuse_create: None,
            fail_next: 0,
            accepted_codes: Vec::new(),
            next_code: 0,
            clock_offset: 0,
        }
    }
}

fn error(kind: &'static str, description: &str) -> MethodError {
    MethodError { kind, description: description.to_string() }
}

fn invalid(description: &str, property: &str) -> Value {
    json!({ "type": "invalidProperties", "description": description, "properties": [property] })
}

fn decode(text: &str) -> Option<Vec<u8>> {
    URL_SAFE_NO_PAD.decode(text.trim_end_matches('=')).ok()
}

fn utc(secs: i64) -> String {
    chrono::DateTime::from_timestamp(secs, 0).unwrap().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

fn parse_utc(text: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(text).ok().map(|t| t.timestamp())
}

impl PushSubs {
    pub fn now(&self) -> i64 {
        SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs() as i64 + self.clock_offset
    }

    pub fn set_verify_gap(&mut self, gap: Duration) {
        self.verify_gap = gap;
    }

    /// The key the server really signs with, as the session would name it.
    pub fn server_key(&self) -> String {
        point_text(&self.server_secret)
    }

    /// The server gets another key pair (and the session names it).
    pub fn rotate_key(&mut self) -> String {
        self.server_secret = random_secret();
        let key = point_text(&self.server_secret);
        self.key = Some(key.clone());
        key
    }

    fn live(&self) -> impl Iterator<Item = &Sub> {
        let now = self.now();
        self.subs.values().filter(move |s| s.expires > now)
    }

    pub fn views(&self) -> Vec<SubView> {
        self.live()
            .map(|s| SubView {
                id: s.id.clone(),
                device_client_id: s.device_client_id.clone(),
                url: s.url.clone(),
                expires: s.expires,
                types: s.types.clone(),
                verified: s.verified,
                email_push: s.email_push.clone(),
                p256dh: s.p256dh.clone(),
                auth: s.auth.clone(),
            })
            .collect()
    }

    pub fn remove(&mut self, id: &str) -> bool {
        self.subs.remove(id).is_some()
    }

    pub fn set_expires(&mut self, id: &str, at: i64) {
        if let Some(sub) = self.subs.get_mut(id) {
            sub.expires = at;
        }
    }

    /// A subscription of another device (for the quota).
    pub fn add_foreign(&mut self, device: &str) -> String {
        self.purge_expired();
        let id = self.free_id();
        let keys = random_secret();
        self.subs.insert(
            id.clone(),
            Sub {
                id: id.clone(),
                device_client_id: Some(device.to_string()),
                url: "https://fcm.googleapis.com/fcm/send/other".into(),
                p256dh: keys.public_key().to_encoded_point(false).as_bytes().to_vec(),
                auth: vec![1; 16],
                types: vec!["Email".into()],
                expires: self.now() + WEEK,
                code: "other".into(),
                verified: true,
                email_push: Value::Null,
            },
        );
        id
    }

    /// A new id is the highest stored one plus one, as on the real server; with nothing stored
    /// the first id comes again (ids are reused after a destroy).
    fn free_id(&self) -> String {
        let next =
            self.subs.keys().filter_map(|id| id.strip_prefix('s')?.parse::<u64>().ok()).max().map_or(0, |n| n + 1);
        format!("s{next}")
    }

    /// The real server purges the expired subscriptions on any get or set; their ids are free.
    fn purge_expired(&mut self) {
        let now = self.now();
        self.subs.retain(|_, s| s.expires > now);
    }

    /// Encrypts `plain` for a subscription, the way the server does.
    pub fn encrypt_for(&self, id: &str, plain: &[u8]) -> Option<Vec<u8>> {
        let sub = self.subs.get(id)?;
        Some(encrypt(plain, &sub.p256dh, &sub.auth, &self.server_secret))
    }

    /// Sends a message to every verified subscription of this account (what a delivery of mail
    /// or a state change does).
    pub fn send_to_verified(&mut self, plain: &Value) -> Vec<Delivery> {
        let text = plain.to_string();
        let out: Vec<Delivery> = self
            .live()
            .filter(|s| s.verified)
            .map(|s| Delivery {
                subscription_id: s.id.clone(),
                url: s.url.clone(),
                body: encrypt(text.as_bytes(), &s.p256dh, &s.auth, &self.server_secret),
            })
            .collect();
        out
    }

    pub fn call(&mut self, name: &str, args: &Value) -> Result<Value, MethodError> {
        self.purge_expired();
        match name {
            "PushSubscription/get" => self.get(args),
            "PushSubscription/set" => self.set(args),
            _ => Err(error("unknownMethod", name)),
        }
    }

    fn get(&self, args: &Value) -> Result<Value, MethodError> {
        let properties: Vec<&str> = match args["properties"].as_array() {
            Some(list) => list.iter().filter_map(Value::as_str).collect(),
            None => vec!["id", "deviceClientId", "verificationCode", "expires", "types"],
        };
        if properties.iter().any(|p| matches!(*p, "url" | "keys")) {
            return Err(error("forbidden", "The 'url' and 'keys' properties are not readable"));
        }
        let wanted: Option<Vec<&str>> = args["ids"].as_array().map(|l| l.iter().filter_map(Value::as_str).collect());
        let mut list = Vec::new();
        let mut not_found = Vec::new();
        let ids: Vec<String> = match &wanted {
            Some(ids) => ids.iter().map(|s| s.to_string()).collect(),
            None => self.live().map(|s| s.id.clone()).collect(),
        };
        for id in ids {
            match self.live().find(|s| s.id == id) {
                Some(sub) => {
                    let mut item = Map::new();
                    for p in &properties {
                        let value = match *p {
                            "id" => json!(sub.id),
                            "deviceClientId" => json!(sub.device_client_id),
                            "verificationCode" => Value::Null,
                            "expires" => json!(utc(sub.expires)),
                            "types" => json!(sub.types),
                            _ => continue,
                        };
                        item.insert(p.to_string(), value);
                    }
                    list.push(Value::Object(item));
                }
                None => not_found.push(id),
            }
        }
        Ok(json!({ "list": list, "notFound": not_found, "state": "ps" }))
    }

    fn set(&mut self, args: &Value) -> Result<Value, MethodError> {
        self.sets.push(args.clone());
        let mut created = Map::new();
        let mut not_created = Map::new();
        for (key, spec) in args["create"].as_object().into_iter().flatten() {
            match self.create(spec) {
                Ok(done) => {
                    created.insert(key.clone(), done);
                }
                Err(e) => {
                    not_created.insert(key.clone(), e);
                }
            }
        }
        let mut updated = Map::new();
        let mut not_updated = Map::new();
        for (id, patch) in args["update"].as_object().into_iter().flatten() {
            match self.update(id, patch) {
                Ok(()) => {
                    updated.insert(id.clone(), Value::Null);
                }
                Err(e) => {
                    not_updated.insert(id.clone(), e);
                }
            }
        }
        let mut destroyed = Vec::new();
        let mut not_destroyed = Map::new();
        for id in args["destroy"].as_array().into_iter().flatten().filter_map(Value::as_str) {
            if self.live().any(|s| s.id == id) {
                self.subs.remove(id);
                destroyed.push(id.to_string());
            } else {
                self.subs.remove(id);
                not_destroyed.insert(id.to_string(), json!({ "type": "notFound" }));
            }
        }
        let mut out = Map::new();
        out.insert("oldState".into(), json!("ps"));
        out.insert("newState".into(), json!("ps"));
        for (name, value) in [
            ("created", created),
            ("notCreated", not_created),
            ("updated", updated),
            ("notUpdated", not_updated),
            ("notDestroyed", not_destroyed),
        ] {
            if !value.is_empty() {
                out.insert(name.into(), Value::Object(value));
            }
        }
        if !destroyed.is_empty() {
            out.insert("destroyed".into(), json!(destroyed));
        }
        Ok(Value::Object(out))
    }

    fn create(&mut self, spec: &Value) -> Result<Value, Value> {
        if let Some((kind, property)) = self.refuse_create {
            return Err(json!({ "type": kind, "description": "refused by the test", "properties": [property] }));
        }
        let url = spec["url"].as_str().unwrap_or_default();
        if !url.starts_with("https://") {
            return Err(invalid("URL scheme is not permitted", "url"));
        }
        let keys = &spec["keys"];
        let (p256dh, auth) = match (keys["p256dh"].as_str().and_then(decode), keys["auth"].as_str().and_then(decode)) {
            (Some(p), Some(a)) => (p, a),
            _ => return Err(invalid("Invalid keys.", "keys")),
        };
        if EncodedPoint::from_bytes(&p256dh)
            .ok()
            .and_then(|p| PublicKey::from_encoded_point(&p).into_option())
            .is_none()
        {
            return Err(invalid("Invalid P-256 ECDH public key.", "keys"));
        }
        if auth.len() != 16 {
            return Err(invalid("Invalid auth secret, expected 16 octets.", "keys"));
        }
        let types: Vec<String> = match spec["types"].as_array() {
            Some(list) => list.iter().filter_map(Value::as_str).map(str::to_string).collect(),
            None => TYPES.iter().map(|t| t.to_string()).collect(),
        };
        if types.iter().any(|t| !TYPES.contains(&t.as_str())) {
            return Err(invalid("Invalid data type.", "types"));
        }
        let email_push = spec.get("emailPush").cloned().unwrap_or(Value::Null);
        for (account, config) in email_push.as_object().into_iter().flatten() {
            if account != ACCOUNT_ID {
                return Err(
                    json!({ "type": "forbidden", "description": "No access to one of the accounts in the emailPush map." }),
                );
            }
            if !matches!(config["urgency"].as_str(), None | Some("very-low" | "low" | "normal" | "high")) {
                return Err(invalid("Invalid urgency value.", "emailPush"));
            }
            for p in config["properties"].as_array().into_iter().flatten() {
                if !p.as_str().is_some_and(|p| EMAIL_PROPERTIES.contains(&p) || p.starts_with("header:") || p == "size")
                {
                    return Err(invalid("Unknown email property.", "emailPush"));
                }
            }
        }
        let now = self.now();
        let expires = spec["expires"].as_str().and_then(parse_utc).unwrap_or(now + WEEK).min(now + WEEK);
        if self.live().count() >= self.max {
            return Err(
                json!({ "type": "overQuota", "description": "There are too many subscriptions, please delete some before adding a new one." }),
            );
        }
        let id = self.free_id();
        self.next_code += 1;
        let code = format!("code-{id}-{}", self.next_code);
        self.subs.insert(
            id.clone(),
            Sub {
                id: id.clone(),
                device_client_id: spec["deviceClientId"].as_str().map(str::to_string),
                url: url.to_string(),
                p256dh,
                auth,
                types,
                expires,
                code: code.clone(),
                verified: false,
                email_push,
            },
        );
        // One verification per account per minute.
        let due = self.last_verification.is_none_or(|at| at.elapsed() >= self.verify_gap);
        if due {
            self.last_verification = Some(Instant::now());
            let plain = json!({ "@type": "PushVerification", "pushSubscriptionId": id, "verificationCode": code });
            let sub = &self.subs[&id];
            let body = encrypt(plain.to_string().as_bytes(), &sub.p256dh, &sub.auth, &self.server_secret);
            self.deliveries.push(Delivery { subscription_id: id.clone(), url: sub.url.clone(), body });
        }
        Ok(json!({ "id": id, "keys": null, "expires": utc(expires) }))
    }

    fn update(&mut self, id: &str, patch: &Value) -> Result<(), Value> {
        let now = self.now();
        if !self.live().any(|s| s.id == id) {
            return Err(json!({ "type": "notFound" }));
        }
        let sub = self.subs.get_mut(id).unwrap();
        for (property, value) in patch.as_object().into_iter().flatten() {
            match property.as_str() {
                "verificationCode" => {
                    if value.as_str() != Some(sub.code.as_str()) {
                        return Err(invalid("Verification code does not match.", "verificationCode"));
                    }
                    sub.verified = true;
                    self.accepted_codes.push((id.to_string(), sub.code.clone()));
                }
                "expires" => {
                    sub.expires = value.as_str().and_then(parse_utc).unwrap_or(now + WEEK).min(now + WEEK);
                }
                "types" => {
                    sub.types =
                        value.as_array().into_iter().flatten().filter_map(Value::as_str).map(str::to_string).collect();
                }
                _ => return Err(invalid("Field could not be set.", property)),
            }
        }
        Ok(())
    }
}

/// RFC 8291, `aes128gcm`, one record: the header, then the data padded (delimiter 2, zeros) to
/// a multiple of 128 bytes, sealed.
pub fn encrypt(plain: &[u8], ua_point: &[u8], auth: &[u8], as_secret: &SecretKey) -> Vec<u8> {
    let mut salt = [0u8; 16];
    getrandom::fill(&mut salt).unwrap();
    encrypt_with_salt(plain, ua_point, auth, as_secret, &salt, 128)
}

pub fn encrypt_with_salt(
    plain: &[u8],
    ua_point: &[u8],
    auth: &[u8],
    as_secret: &SecretKey,
    salt: &[u8],
    multiple: usize,
) -> Vec<u8> {
    let as_point = as_secret.public_key().to_encoded_point(false).as_bytes().to_vec();
    let ua_public = PublicKey::from_sec1_bytes(ua_point).unwrap();
    let shared = diffie_hellman(as_secret.to_nonzero_scalar(), ua_public.as_affine());
    let mut info = b"WebPush: info\0".to_vec();
    info.extend_from_slice(ua_point);
    info.extend_from_slice(&as_point);
    let mut ikm = [0u8; 32];
    Hkdf::<Sha256>::new(Some(auth), shared.raw_secret_bytes()).expand(&info, &mut ikm).unwrap();
    let hk = Hkdf::<Sha256>::new(Some(salt), &ikm);
    let (mut cek, mut nonce) = ([0u8; 16], [0u8; 12]);
    hk.expand(b"Content-Encoding: aes128gcm\0", &mut cek).unwrap();
    hk.expand(b"Content-Encoding: nonce\0", &mut nonce).unwrap();
    let mut data = plain.to_vec();
    data.push(2);
    data.resize(data.len().div_ceil(multiple) * multiple, 0);
    let sealed = Aes128Gcm::new_from_slice(&cek).unwrap().encrypt(&Nonce::from(nonce), data.as_slice()).unwrap();
    let mut out = salt.to_vec();
    out.extend_from_slice(&4096u32.to_be_bytes());
    out.push(65);
    out.extend_from_slice(&as_point);
    out.extend_from_slice(&sealed);
    out
}
