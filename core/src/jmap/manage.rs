//! What a signed-in user manages in their own account at the server: masked addresses and the
//! account's security settings (password, two-step codes, app passwords, API keys, passkeys,
//! connected apps). All of it is Stalwart's management API, which is JMAP with methods named
//! `x:<Object>/get|query|set` on the ordinary endpoint, plus `GET /api/account` for what the
//! account may do. What the server answers was found against a real server and is written in
//! `dev/notes/stalwart-admin-access.md` and `dev/notes/stalwart-enterprise-observed.md`:
//!
//! - Which of it a sign-in reaches depends on the scopes it was granted: `settings` for masked
//!   addresses, `security` for the rest. Without them every call is a method error `forbidden`.
//! - A server without the Enterprise licence answers `x:MaskedEmail` with a `forbidden` whose
//!   text says so; that is told from a plain refusal by the text.
//! - The password and the two-step codes are the one object `x:AccountPassword`, and every
//!   change to it needs `currentSecret` (and, with two-step codes on, `otpCode`).
//! - A key or app password made with the account's own rights (`Inherit`) is refused for a
//!   sign-in that has fewer rights than the account (the scopes narrow them); it is then made
//!   with the rights this sign-in has, which `GET /api/account` lists.
//!
//! Passwords, secrets and codes are never put into an error: what the server wrote is passed
//! on only where it cannot hold one, and the words here are the app's own.

use chrono::{DateTime, SecondsFormat};
use serde_json::{json, Map, Value};

use super::{
    errors::unauthorized,
    send::{destroyed, updated},
    totp, Connection,
};
use crate::{
    model::{
        AccountFeatures, AccountSecurity, ConnectedApp, Credential, MaskedAddress, MaskedAddresses, NewSecret, Passkey,
        TwoStepSetup,
    },
    Error, Result,
};

/// The longest prefix the app lets a masked address start with.
const MAX_PREFIX: usize = 32;
/// The longest name Stalwart takes for a passkey.
const MAX_PASSKEY_NAME: usize = 64;
const ENTERPRISE_TEXT: &str = "only available in the Enterprise edition";
const NOT_GRANTED_TEXT: &str = "not authorized to grant permissions";
const SINGLETON: &str = "singleton";

/// What `GET /api/account` says about the signed-in account.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AccountInfo {
    pub edition: String,
    pub permissions: Vec<String>,
}

impl AccountInfo {
    fn holds(&self, permission: &str) -> bool {
        self.permissions.iter().any(|p| p == permission)
    }
}

/// A method of a request the server answered with an error of its own.
#[derive(Debug, Clone, PartialEq)]
struct MethodRefusal {
    kind: String,
    description: String,
}

impl MethodRefusal {
    fn is_enterprise(&self) -> bool {
        self.description.contains(ENTERPRISE_TEXT)
    }
}

type Answer = std::result::Result<Value, MethodRefusal>;

/// Which kind of key a credential is.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CredentialKind {
    AppPassword,
    ApiKey,
}

impl CredentialKind {
    fn object(self) -> &'static str {
        match self {
            CredentialKind::AppPassword => "x:AppPassword",
            CredentialKind::ApiKey => "x:ApiKey",
        }
    }

    fn plural(self) -> &'static str {
        match self {
            CredentialKind::AppPassword => "app passwords",
            CredentialKind::ApiKey => "API keys",
        }
    }
}

impl Connection {
    // ---- what the server offers ----

    /// The account's permissions and the server's edition. A server that has no such address
    /// (an older one) or does not answer in JSON gives an empty answer, not an error; a 401 is
    /// the usual expired-token failure.
    pub async fn account_info(&self) -> Result<AccountInfo> {
        let mut headers = self.client.headers().clone();
        headers.remove(reqwest::header::CONTENT_TYPE);
        let response = self
            .http
            .get(format!("{}/api/account", self.origin))
            .headers(headers)
            .send()
            .await
            .map_err(|e| Error::temporary(format!("account: {e}")))?;
        let status = response.status().as_u16();
        let bytes = response.bytes().await.map_err(|e| Error::temporary(format!("account: {e}")))?;
        match status {
            200..=299 => {}
            401 => return Err(unauthorized("account: HTTP 401")),
            // Too many requests or a request that timed out says nothing about the server's features.
            408 | 429 => return Err(Error::temporary(format!("account: HTTP {status}"))),
            400..=499 => return Ok(AccountInfo::default()),
            _ => return Err(Error::temporary(format!("account: HTTP {status}"))),
        }
        let Ok(value) = serde_json::from_slice::<Value>(&bytes) else { return Ok(AccountInfo::default()) };
        Ok(AccountInfo {
            edition: value["edition"].as_str().unwrap_or_default().trim().to_lowercase(),
            permissions: strings(&value["permissions"]),
        })
    }

    /// What the server offers this account. `signed_in_short` is true when the server offers
    /// the settings or security scope and this sign-in was made without it: then nothing is
    /// asked beyond the edition and the answer is only that signing in again is needed.
    pub async fn features(&self, signed_in_short: bool) -> Result<AccountFeatures> {
        let info = self.account_info().await?;
        let mut features = AccountFeatures {
            edition: info.edition.clone(),
            masked_email: false,
            security: false,
            needs_sign_in: false,
        };
        if signed_in_short {
            features.needs_sign_in = true;
            return Ok(features);
        }
        if info.holds("sysMaskedEmailGet") && info.holds("sysMaskedEmailQuery") {
            let probe = json!({ "accountId": self.account_id(), "limit": 1 });
            features.masked_email = probe_offered(&self.x_calls(&[("x:MaskedEmail/query", probe)]).await?[0])?;
        }
        let probe = json!({ "accountId": self.account_id(), "ids": [SINGLETON] });
        features.security = probe_offered(&self.x_calls(&[("x:AccountPassword/get", probe)]).await?[0])?;
        Ok(features)
    }

    /// One request with these `x:` method calls; the answer of each, in order. A method the
    /// server refused is an `Err` inside the list, for the caller to read the kind of; a request
    /// that did not get through at all (401, the network) is the outer error.
    async fn x_calls(&self, calls: &[(&str, Value)]) -> Result<Vec<Answer>> {
        let responses = self.call_keeping_errors(&[], calls).await?;
        if responses.len() != calls.len() {
            return Err(Error::temporary("the server's answer does not match the request"));
        }
        Ok(responses
            .into_iter()
            .map(|(name, args)| match name.as_str() {
                "error" => Err(MethodRefusal {
                    kind: args["type"].as_str().unwrap_or("unknown").to_string(),
                    description: args["description"].as_str().unwrap_or_default().to_string(),
                }),
                _ => Ok(args),
            })
            .collect())
    }

    // ---- masked addresses ----

    /// The account's masked addresses, newest first, and the server's limit (`None` when this
    /// account may not read it, which is the usual case for anyone but an administrator).
    pub async fn masked_addresses(&self) -> Result<MaskedAddresses> {
        let account = self.account_id();
        let calls = [
            ("x:MaskedEmail/get", json!({ "accountId": account, "ids": null })),
            ("x:Email/get", json!({ "accountId": account, "ids": [SINGLETON], "properties": ["maxMaskedAddresses"] })),
        ];
        let mut answers = self.x_calls(&calls).await?.into_iter();
        let list = answers.next().unwrap_or(Err(missing_answer())).map_err(|r| masked_refusal(XFailure::Refused(r)))?;
        let limit =
            answers.next().and_then(std::result::Result::ok).and_then(|v| v["list"][0]["maxMaskedAddresses"].as_u64());
        let mut addresses: Vec<MaskedAddress> =
            list["list"].as_array().into_iter().flatten().filter_map(masked_from).collect();
        // Stable: addresses made in the same second stay in the server's order.
        addresses.sort_by_key(|a| std::cmp::Reverse(a.created_at));
        Ok(MaskedAddresses { addresses, limit: limit.and_then(|l| u32::try_from(l).ok()) })
    }

    /// Has the server make a masked address. The prefix is checked here first.
    pub async fn create_masked_address(
        &self,
        description: &str,
        for_domain: &str,
        prefix: &str,
    ) -> Result<MaskedAddress> {
        let prefix = check_prefix(prefix)?;
        let mut new = Map::new();
        for (key, text) in
            [("description", description.trim()), ("forDomain", for_domain.trim()), ("emailPrefix", prefix)]
        {
            if !text.is_empty() {
                new.insert(key.to_string(), json!(text));
            }
        }
        let args = json!({ "accountId": self.account_id(), "create": { "new": new } });
        let set = self.x_one("x:MaskedEmail/set", args).await.map_err(masked_refusal)?;
        let created = &set["created"]["new"];
        let (Some(id), Some(email)) = (created["id"].as_str(), created["email"].as_str()) else {
            return Err(match set["notCreated"].get("new") {
                Some(e) => masked_create_error(e),
                None => Error::temporary("the server's answer does not say whether it was created"),
            });
        };
        // What the server made, read back for the time it was made; what was asked for stands in
        // if it cannot be read just now: the address exists either way.
        let get = json!({ "accountId": self.account_id(), "ids": [id] });
        let read = self.x_one("x:MaskedEmail/get", get).await.ok();
        Ok(read.and_then(|g| g["list"].get(0).and_then(masked_from)).unwrap_or_else(|| MaskedAddress {
            id: id.to_string(),
            email: email.to_string(),
            description: description.trim().to_string(),
            for_domain: for_domain.trim().to_string(),
            enabled: true,
            created_at: None,
            expires_at: None,
        }))
    }

    /// Switches a masked address on or off and, with `description`, changes its note.
    pub async fn update_masked_address(&self, id: &str, enabled: bool, description: Option<&str>) -> Result<()> {
        let mut patch = json!({ "enable": enabled });
        if let Some(note) = description {
            patch["description"] = json!(note.trim());
        }
        let args = json!({ "accountId": self.account_id(), "update": { id: patch } });
        let set = self.x_one("x:MaskedEmail/set", args).await.map_err(masked_refusal)?;
        updated(&set, id, "That masked address no longer exists.")
    }

    /// Deletes a masked address. One that is already gone is as good as deleted.
    pub async fn delete_masked_address(&self, id: &str) -> Result<()> {
        let args = json!({ "accountId": self.account_id(), "destroy": [id] });
        let set = self.x_one("x:MaskedEmail/set", args).await.map_err(masked_refusal)?;
        destroyed(&set, id)
    }

    /// One `x:` call: its answer, or the refusal of the server.
    async fn x_one(&self, method: &str, args: Value) -> std::result::Result<Value, XFailure> {
        let mut answers = self.x_calls(&[(method, args)]).await.map_err(XFailure::Other)?;
        answers.pop().unwrap_or(Err(missing_answer())).map_err(XFailure::Refused)
    }

    // ---- the account's security settings ----

    /// The password setting (two-step codes), app passwords, API keys, passkeys and connected
    /// apps in one request. `client_id` is the id this sign-in was made with: it marks the
    /// app's own grant among the connected apps.
    pub async fn account_security(&self, client_id: &str) -> Result<AccountSecurity> {
        let account = self.account_id();
        let all = json!({ "accountId": account, "ids": null });
        let calls = [
            ("x:AccountPassword/get", json!({ "accountId": account, "ids": [SINGLETON] })),
            ("x:AppPassword/get", all.clone()),
            ("x:ApiKey/get", all.clone()),
            ("x:Passkey/get", all.clone()),
            ("x:OAuthGrant/get", all),
        ];
        let mut lists = Vec::new();
        for answer in self.x_calls(&calls).await? {
            lists.push(answer.map_err(|r| security_refusal(&r))?);
        }
        let [password, app, key, passkey, grant] = <[Value; 5]>::try_from(lists)
            .map_err(|_| Error::temporary("the server's answer does not match the request"))?;
        Ok(AccountSecurity {
            two_step: password["list"][0]["otpAuth"].as_object().is_some_and(|o| !o.is_empty()),
            app_passwords: credentials_from(&app),
            api_keys: credentials_from(&key),
            passkeys: passkeys_from(&passkey),
            connected_apps: apps_from(&grant, client_id),
        })
    }

    /// Changes the password. With two-step codes on, the server also wants a current code:
    /// `code` is that (blank when there are none).
    pub async fn change_password(&self, current: &str, new: &str, code: Option<&str>) -> Result<()> {
        if current.is_empty() {
            return Err(Error::invalid("Type your current password."));
        }
        if new.is_empty() {
            return Err(Error::invalid("Type the new password."));
        }
        let mut patch = json!({ "currentSecret": current, "secret": new });
        add_code(&mut patch, code);
        self.set_password_object(patch, &[current, new], has_code(code)).await
    }

    /// Switches two-step codes on with the secret of `otpauth_url`, once `code` has been
    /// checked against that secret (here, before anything is sent). `now` is Unix seconds.
    pub async fn enable_two_step(
        &self,
        current_password: &str,
        otpauth_url: &str,
        code: &str,
        now: u64,
        old_code: Option<&str>,
    ) -> Result<()> {
        if current_password.is_empty() {
            return Err(Error::invalid("Type your current password."));
        }
        let key = totp::key_of(otpauth_url)?;
        if !totp::code_matches(&key, code, now) {
            return Err(Error::invalid(
                "That code is not right. Check that the authenticator app has the new entry, then type the code it shows now.",
            ));
        }
        let mut patch = json!({ "currentSecret": current_password, "otpAuth": { "otpUrl": otpauth_url.trim() } });
        add_code(&mut patch, old_code);
        self.set_password_object(patch, &[current_password, otpauth_url, code], has_code(old_code)).await
    }

    /// Switches two-step codes off. The server wants a current code for it: `code`.
    pub async fn disable_two_step(&self, current_password: &str, code: Option<&str>) -> Result<()> {
        if current_password.is_empty() {
            return Err(Error::invalid("Type your current password."));
        }
        let mut patch = json!({ "currentSecret": current_password, "otpAuth": { "otpUrl": null } });
        add_code(&mut patch, code);
        self.set_password_object(patch, &[current_password], has_code(code)).await
    }

    /// `x:AccountPassword/set` with `patch`. None of the `secrets` may reach an error.
    /// `with_code` is true when a current two-step code was sent along.
    async fn set_password_object(&self, patch: Value, secrets: &[&str], with_code: bool) -> Result<()> {
        let args = json!({ "accountId": self.account_id(), "update": { SINGLETON: patch } });
        let set = self.x_one("x:AccountPassword/set", args).await.map_err(|f| match f {
            XFailure::Refused(r) => security_refusal(&r),
            XFailure::Other(e) => e,
        })?;
        match set["notUpdated"].get(SINGLETON) {
            Some(e) => Err(password_error(e, secrets, with_code)),
            None if set["updated"].get(SINGLETON).is_some() => Ok(()),
            None => Err(Error::temporary("the server's answer does not say whether it changed")),
        }
    }

    /// Makes an app password or an API key. `expires_at` is Unix seconds, which `now` must be
    /// before. The secret is in the answer and the server shows it nowhere afterwards.
    pub async fn create_credential(
        &self,
        kind: CredentialKind,
        description: &str,
        expires_at: Option<i64>,
        now: i64,
    ) -> Result<NewSecret> {
        let description = description.trim();
        if description.is_empty() {
            return Err(Error::invalid("Give it a name, so you know later what it is for."));
        }
        let expires = match expires_at {
            Some(at) if at <= now => return Err(Error::invalid("The expiry has to be a time that is still to come.")),
            Some(at) => Some(
                DateTime::from_timestamp(at, 0)
                    .ok_or_else(|| Error::invalid("That expiry is out of range."))?
                    .to_rfc3339_opts(SecondsFormat::Secs, true),
            ),
            None => None,
        };
        let make = |permissions: Value| {
            let mut new = json!({ "description": description, "permissions": permissions });
            if let Some(at) = &expires {
                new["expiresAt"] = json!(at);
            }
            json!({ "accountId": self.account_id(), "create": { "new": new } })
        };
        let method = format!("{}/set", kind.object());
        let mut set = self.x_one(&method, make(json!({ "@type": "Inherit" }))).await.map_err(security_failure)?;
        if set["notCreated"].get("new").is_some_and(is_not_granted) {
            // The account's own rights are more than this sign-in has. It may make what it has.
            let held = self.account_info().await?.permissions;
            if !held.is_empty() {
                let replace: Map<String, Value> = held.into_iter().map(|p| (p, json!(true))).collect();
                let rights = json!({ "@type": "Replace", "permissions": replace });
                set = self.x_one(&method, make(rights)).await.map_err(security_failure)?;
            }
        }
        match (set["created"]["new"]["id"].as_str(), set["created"]["new"]["secret"].as_str()) {
            (Some(id), Some(secret)) => Ok(NewSecret { id: id.to_string(), secret: secret.to_string() }),
            _ => Err(match set["notCreated"].get("new") {
                Some(e) => credential_create_error(kind, e),
                None => Error::temporary("the server's answer does not say whether it was created"),
            }),
        }
    }

    /// Deletes an app password or an API key. One that is already gone is as good as deleted.
    pub async fn delete_credential(&self, kind: CredentialKind, id: &str) -> Result<()> {
        let args = json!({ "accountId": self.account_id(), "destroy": [id] });
        let set = self.x_one(&format!("{}/set", kind.object()), args).await.map_err(security_failure)?;
        destroyed(&set, id)
    }

    /// Gives a passkey another name.
    pub async fn rename_passkey(&self, id: &str, name: &str) -> Result<()> {
        let name = name.trim();
        if name.is_empty() {
            return Err(Error::invalid("Give the passkey a name."));
        }
        if name.chars().count() > MAX_PASSKEY_NAME {
            return Err(Error::invalid(format!("A passkey's name has at most {MAX_PASSKEY_NAME} characters.")));
        }
        let args = json!({ "accountId": self.account_id(), "update": { id: { "description": name } } });
        let set = self.x_one("x:Passkey/set", args).await.map_err(security_failure)?;
        updated(&set, id, "That passkey no longer exists.")
    }

    /// Deletes a passkey. One that is already gone is as good as deleted.
    pub async fn delete_passkey(&self, id: &str) -> Result<()> {
        let args = json!({ "accountId": self.account_id(), "destroy": [id] });
        let set = self.x_one("x:Passkey/set", args).await.map_err(security_failure)?;
        destroyed(&set, id)
    }

    /// Takes an app's access away. The grant this app signed in with is refused: `client_id`
    /// is the id of that sign-in.
    pub async fn revoke_connected_app(&self, id: &str, client_id: &str) -> Result<()> {
        let get = json!({ "accountId": self.account_id(), "ids": [id] });
        let read = self.x_one("x:OAuthGrant/get", get).await.map_err(security_failure)?;
        match read["list"].get(0) {
            // Gone already.
            None => return Ok(()),
            Some(grant) if !client_id.is_empty() && grant["clientId"].as_str() == Some(client_id) => {
                return Err(Error::invalid("This is the sign-in this app is using. Sign out of the app instead."));
            }
            Some(_) => {}
        }
        let args = json!({ "accountId": self.account_id(), "destroy": [id] });
        let set = self.x_one("x:OAuthGrant/set", args).await.map_err(security_failure)?;
        destroyed(&set, id)
    }
}

/// A new two-step secret and its address. Nothing is asked of the server.
pub fn new_two_step(issuer_host: &str, account_address: &str) -> Result<TwoStepSetup> {
    let (otpauth_url, secret) = totp::new_setup(issuer_host, account_address)?;
    Ok(TwoStepSetup { otpauth_url, secret })
}

// ---- failures ----

/// A call that did not give an answer: the server refused the method, or the request failed.
#[derive(Debug)]
enum XFailure {
    Refused(MethodRefusal),
    Other(Error),
}

fn missing_answer() -> MethodRefusal {
    MethodRefusal { kind: "missing".into(), description: "the server gave no answer".into() }
}

fn masked_refusal(f: XFailure) -> Error {
    match f {
        XFailure::Other(e) => e,
        XFailure::Refused(r) if r.is_enterprise() => Error::invalid("This server does not offer masked addresses."),
        XFailure::Refused(r) if r.kind == "unknownMethod" => {
            Error::invalid("This server does not offer masked addresses.")
        }
        XFailure::Refused(r) if r.kind == "forbidden" => {
            Error::invalid("This sign-in is not allowed to use masked addresses. Sign in again to allow it.")
        }
        XFailure::Refused(r) => Error::temporary(format!("masked addresses: {}", words(&r))),
    }
}

fn security_failure(f: XFailure) -> Error {
    match f {
        XFailure::Other(e) => e,
        XFailure::Refused(r) => security_refusal(&r),
    }
}

/// A method of the security settings that the server refused as a whole.
fn security_refusal(r: &MethodRefusal) -> Error {
    match r.kind.as_str() {
        "forbidden" | "unknownMethod" => {
            Error::invalid("The security settings are not available with this sign-in. Sign in again to allow them.")
        }
        _ => Error::temporary(format!("security settings: {}", words(r))),
    }
}

fn words(r: &MethodRefusal) -> &str {
    if r.description.is_empty() {
        &r.kind
    } else {
        &r.description
    }
}

fn masked_create_error(e: &Value) -> Error {
    let kind = e["type"].as_str().unwrap_or("unknown");
    let text = e["description"].as_str().unwrap_or_default();
    let names = |property: &str| e["properties"].as_array().is_some_and(|p| p.iter().any(|p| p == property));
    match kind {
        "overQuota" => match first_number(text) {
            Some(n) => Error::invalid(format!("You have reached the limit of {n} masked addresses.")),
            None => Error::invalid("You have reached the limit of masked addresses the server allows."),
        },
        _ if names("emailPrefix") => Error::invalid(
            "The server does not accept that start for the address. Use letters, digits and underscores, and keep it short.",
        ),
        "forbidden" if text.contains(ENTERPRISE_TEXT) => Error::invalid("This server does not offer masked addresses."),
        _ => super::send::set_error(e),
    }
}

fn credential_create_error(kind: CredentialKind, e: &Value) -> Error {
    let text = e["description"].as_str().unwrap_or_default();
    match e["type"].as_str() {
        Some("overQuota") => match first_number(text) {
            Some(n) => Error::invalid(format!("You have reached the limit of {n} {}.", kind.plural())),
            None => Error::invalid(format!("You have reached the limit of {} the server allows.", kind.plural())),
        },
        Some("forbidden") if is_not_granted(e) => Error::invalid(
            "This sign-in has fewer rights than the account, so it cannot make one. Sign in again to allow it.",
        ),
        _ => super::send::set_error(e),
    }
}

fn is_not_granted(e: &Value) -> bool {
    e["type"] == "forbidden" && e["description"].as_str().is_some_and(|d| d.contains(NOT_GRANTED_TEXT))
}

/// The server's answer to a failed change of the password object, in the app's own words. The
/// server's text is passed on only for a weak password, and only when it holds none of `secrets`.
fn password_error(e: &Value, secrets: &[&str], with_code: bool) -> Error {
    let kind = e["type"].as_str().unwrap_or("unknown");
    let text = e["description"].as_str().unwrap_or_default();
    match kind {
        "forbidden" if text.contains("OTP") => Error::invalid(
            "Two-step codes are on for this account, and the server wants a current code for this change.",
        ),
        // The server words a wrong password and a wrong code alike.
        "forbidden" if text.contains("incorrect") && with_code => {
            Error::invalid("The current password or the two-step code is not right.")
        }
        "forbidden" if text.contains("incorrect") => Error::invalid("The current password is not right."),
        "forbidden" => Error::invalid("The server does not allow this sign-in to change the password."),
        "invalidProperties" | "invalidPatch"
            if !text.is_empty() && !secrets.iter().any(|s| !s.is_empty() && text.contains(s)) =>
        {
            Error::invalid(text.to_string())
        }
        "invalidProperties" | "invalidPatch" => Error::invalid("The server does not accept that password."),
        _ => Error::temporary("the server could not make the change now"),
    }
}

fn has_code(code: Option<&str>) -> bool {
    code.map(str::trim).is_some_and(|c| !c.is_empty())
}

/// Whether a probe's answer says the feature is offered. Only a plain refusal or an unknown
/// method means it is not; any other method error says nothing and is passed on.
fn probe_offered(answer: &Answer) -> Result<bool> {
    match answer {
        Ok(_) => Ok(true),
        Err(r) if matches!(r.kind.as_str(), "forbidden" | "unknownMethod") => Ok(false),
        Err(r) => Err(Error::temporary(format!("server features: {}", words(r)))),
    }
}

fn add_code(patch: &mut Value, code: Option<&str>) {
    if let Some(code) = code.map(str::trim).filter(|c| !c.is_empty()) {
        patch["otpAuth"]["otpCode"] = json!(code);
    }
}

/// The prefix as it is sent, or what is wrong with it.
pub fn check_prefix(prefix: &str) -> Result<&str> {
    let prefix = prefix.trim();
    if prefix.chars().count() > MAX_PREFIX {
        return Err(Error::invalid(format!("The start of the address has at most {MAX_PREFIX} characters.")));
    }
    if !prefix.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_')) {
        return Err(Error::invalid(
            "The start of the address may hold only letters, digits, dots, hyphens and underscores.",
        ));
    }
    Ok(prefix)
}

fn first_number(text: &str) -> Option<u64> {
    let digits: String = text.chars().skip_while(|c| !c.is_ascii_digit()).take_while(char::is_ascii_digit).collect();
    digits.parse().ok()
}

// ---- reading answers ----

fn strings(v: &Value) -> Vec<String> {
    v.as_array().into_iter().flatten().filter_map(|s| s.as_str().map(str::to_string)).collect()
}

fn date(v: &Value) -> Option<i64> {
    DateTime::parse_from_rfc3339(v.as_str()?).ok().map(|at| at.timestamp())
}

fn text(v: &Value) -> String {
    v.as_str().unwrap_or_default().to_string()
}

fn masked_from(v: &Value) -> Option<MaskedAddress> {
    let (id, email) = (v["id"].as_str()?, v["email"].as_str()?);
    Some(MaskedAddress {
        id: id.to_string(),
        email: email.to_string(),
        description: text(&v["description"]),
        for_domain: text(&v["forDomain"]),
        // A server that does not say is taken as on: that is how it delivers.
        enabled: v["enable"] != false,
        created_at: date(&v["createdAt"]),
        expires_at: date(&v["expiresAt"]),
    })
}

fn credentials_from(get: &Value) -> Vec<Credential> {
    get["list"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|c| {
            Some(Credential {
                id: c["id"].as_str()?.to_string(),
                description: text(&c["description"]),
                created_at: date(&c["createdAt"]),
                expires_at: date(&c["expiresAt"]),
            })
        })
        .collect()
}

fn passkeys_from(get: &Value) -> Vec<Passkey> {
    get["list"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|p| {
            Some(Passkey {
                id: p["id"].as_str()?.to_string(),
                name: text(&p["description"]),
                created_at: date(&p["createdAt"]),
            })
        })
        .collect()
}

/// The grants the account holds. `pending` is not looked at: on Stalwart it only says that the
/// refresh token was rotated and stays set from the first refresh on, so every grant in use has
/// it. (A consent still waiting is not an `x:OAuthGrant` at all.) Its scopes are the names the server gives them, in a fixed order.
fn apps_from(get: &Value, client_id: &str) -> Vec<ConnectedApp> {
    get["list"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|g| {
            let id = g["id"].as_str()?.to_string();
            let mut scopes: Vec<String> = g["scopes"]
                .as_object()
                .into_iter()
                .flatten()
                .filter(|(_, on)| **on == true)
                .map(|(name, _)| name.clone())
                .collect();
            scopes.sort();
            let name =
                g["clientName"].as_str().filter(|n| !n.is_empty()).or(g["clientId"].as_str()).unwrap_or_default();
            Some(ConnectedApp {
                id,
                name: name.to_string(),
                scopes,
                created_at: date(&g["createdAt"]),
                is_this_app: !client_id.is_empty() && g["clientId"].as_str() == Some(client_id),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_prefix_is_checked_before_the_server_is_asked() {
        assert_eq!(check_prefix("  shop.name-1_x ").unwrap(), "shop.name-1_x");
        assert_eq!(check_prefix("").unwrap(), "");
        assert!(check_prefix(&"a".repeat(32)).is_ok());
        for bad in [&"a".repeat(33)[..], "has space", "slash/", "at@sign", "ümlaut", "plus+"] {
            assert!(matches!(check_prefix(bad), Err(Error::Invalid { .. })), "{bad}");
        }
    }

    #[test]
    fn the_first_number_of_a_text_is_found() {
        assert_eq!(first_number("You have exceeded your quota of 5 API keys."), Some(5));
        assert_eq!(first_number("quota of 500 addresses"), Some(500));
        assert_eq!(first_number("no number"), None);
    }

    #[test]
    fn masked_addresses_are_read_with_the_switch_called_enable() {
        let m = masked_from(&json!({"id":"a","email":"x@y.z","enable":false,"description":"d","forDomain":"https://s",
            "createdAt":"2026-11-14T10:00:00Z","expiresAt":null}))
        .unwrap();
        assert!(!m.enabled && m.description == "d" && m.for_domain == "https://s");
        assert_eq!((m.created_at, m.expires_at), (Some(1_794_650_400), None));
        assert!(masked_from(&json!({"id":"a"})).is_none());
        assert!(masked_from(&json!({"id":"a","email":"x@y.z"})).unwrap().enabled);
    }

    #[test]
    fn the_connected_apps_know_their_own_grant() {
        let grants = json!({"list":[
            {"id":"g1","clientId":"c-app","clientName":"kweb mail","scopes":{"mail":true,"security":true,"files":false},"createdAt":"2026-11-14T10:00:00Z","pending":true,"generation":87},
            {"id":"g2","clientId":"c-other","clientName":null,"scopes":{"mail":true},"pending":false},
            {"id":"g3","clientId":"c-old","clientName":"old","scopes":{},"pending":true}]});
        // `pending` is only the rotation flag of the refresh token: a grant in use has it.
        let apps = apps_from(&grants, "c-app");
        assert_eq!(apps.len(), 3);
        assert!(apps[0].is_this_app && apps[0].name == "kweb mail" && apps[0].scopes == ["mail", "security"]);
        assert!(!apps[1].is_this_app && apps[1].name == "c-other");
        // No client id known: none is marked.
        assert!(apps_from(&grants, "").iter().all(|a| !a.is_this_app));
    }

    #[test]
    fn a_password_never_comes_back_in_an_error() {
        let e =
            json!({"type":"invalidProperties","description":"Password hunter22 is on a list","properties":["secret"]});
        let msg = password_error(&e, &["hunter22", "old"], false).to_string();
        assert!(!msg.contains("hunter22"));
        let weak = json!({"type":"invalidProperties","description":"Password must be at least 8 characters long."});
        assert_eq!(password_error(&weak, &["x"], false).to_string(), "Password must be at least 8 characters long.");
        let wrong = json!({"type":"forbidden","description":"Current secret is incorrect."});
        assert_eq!(password_error(&wrong, &[], false).to_string(), "The current password is not right.");
        assert_eq!(
            password_error(&wrong, &[], true).to_string(),
            "The current password or the two-step code is not right."
        );
    }
}
