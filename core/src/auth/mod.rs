//! OAuth 2.0 sign-in to Stalwart, no passwords stored anywhere (RFC 8252 native-app flow).
//!
//! - Discovery: `<server>/.well-known/oauth-authorization-server`.
//! - Client: registered on the fly (RFC 7591) as a public client. Stalwart answers with a
//!   stateless, signed client_id; it allows that for loopback redirects without an admin step.
//! - Sign-in: authorization code + PKCE (S256) in the user's browser, so whatever the server
//!   asks for there (password, 2FA, passkey, SSO) stays between the browser and Stalwart.
//!   The host picks the redirect URI and hands the callback URL back; [`loopback`] is a
//!   one-shot listener on 127.0.0.1 for hosts that use a loopback redirect.
//! - Tokens: only the refresh token is kept, through [`crate::secrets::SecretStore`]. Stalwart
//!   rotates it on every refresh, so the new one is stored each time. Access tokens live in
//!   memory, inside [`Tokens`].
//!
//! The browser itself is the host's business: this module only builds the URL to open.

pub mod loopback;
mod tokens;

#[cfg(test)]
mod tests;

use std::{
    sync::{Arc, LazyLock},
    time::{Duration, Instant},
};

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use url::Url;

use crate::{secrets::SecretStore, Error, Result};

pub use tokens::Tokens;

const CLIENT_NAME: &str = "kweb mail";
/// Mail, calendars and contacts for JMAP; offline_access for the refresh token. Nothing administrative.
const SCOPES: &str = "openid offline_access urn:ietf:params:oauth:scope:mail \
                      urn:ietf:params:oauth:scope:calendars urn:ietf:params:oauth:scope:contacts";
/// File storage. Asked for only where the server offers it: without it the session of a
/// Stalwart server lists no file storage at all, and a server that does not know the scope
/// must not be asked for it.
const FILES_SCOPE: &str = "urn:ietf:params:oauth:scope:files";
/// The account's own settings (masked addresses). Stalwart's own scope, asked for where offered.
pub const SETTINGS_SCOPE: &str = "urn:stalwart:oauth:scope:settings";
/// The account's own security settings (password, two-step codes, app passwords, API keys,
/// passkeys, connected apps). Nothing administrative: an administrator's rights stay out of it.
pub const SECURITY_SCOPE: &str = "urn:stalwart:oauth:scope:security";

/// The scopes to ask this server for: the base set, and each of the optional ones the server's
/// metadata lists.
fn scopes_for(meta: &Metadata) -> String {
    let mut scope = SCOPES.to_string();
    for optional in [FILES_SCOPE, SETTINGS_SCOPE, SECURITY_SCOPE] {
        if meta.scopes_supported.iter().any(|s| s == optional) {
            scope.push(' ');
            scope.push_str(optional);
        }
    }
    scope
}

/// Which of the account scopes (settings, security) a server offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AccountScopes {
    pub settings: bool,
    pub security: bool,
}

/// Asks the server's metadata which of the account scopes it offers. A server that cannot be
/// asked offers none as far as the caller can tell: `Err` is for the caller to treat as that.
pub async fn account_scopes_offered(server_url: &str) -> Result<AccountScopes> {
    let meta = discover(server_url.trim()).await?;
    let has = |scope: &str| meta.scopes_supported.iter().any(|s| s == scope);
    Ok(AccountScopes { settings: has(SETTINGS_SCOPE), security: has(SECURITY_SCOPE) })
}
/// How long a host should wait for the browser to come back before giving up.
pub const SIGN_IN_TIMEOUT: Duration = Duration::from_secs(300);

/// Non-secret OAuth facts about one server. The caller persists it (serde) between runs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OAuthClient {
    pub server_url: String,
    pub client_id: String,
    pub token_endpoint: String,
    /// Empty when the server has none.
    pub revocation_endpoint: String,
    /// The scopes the client was registered with. Empty in what an older version stored, which
    /// then registers again at the next sign-in.
    #[serde(default)]
    pub scope: String,
    /// The scopes the server granted at the sign-in, as its token answer says. Empty in what an
    /// older version stored: that sign-in counts as having been granted none of the optional
    /// ones, which is how the app knows to ask the user to sign in again.
    #[serde(default)]
    pub granted: String,
}

impl OAuthClient {
    /// Whether the sign-in was granted this scope.
    pub fn was_granted(&self, scope: &str) -> bool {
        self.granted.split_whitespace().any(|s| s == scope)
    }
}

/// A sign-in that has been started in the browser and not finished yet.
pub struct PendingSignIn {
    /// Open this in the user's browser.
    pub authorize_url: String,
    client: OAuthClient,
    verifier: String,
    state: String,
    redirect_uri: String,
}

impl PendingSignIn {
    /// The `state` value the browser must bring back.
    pub fn state(&self) -> &str {
        &self.state
    }
}

/// The result of a finished sign-in.
pub struct SignedIn {
    /// Persist this: it is what [`Tokens::new`] needs next time.
    pub client: OAuthClient,
    /// What the server reported as the user's name (empty if it did not say).
    pub username: String,
    pub tokens: Tokens,
}

/// An access token in memory, with when to stop using it.
#[derive(Clone)]
pub struct AccessToken {
    pub token: String,
    pub renew_at: Instant,
}

#[derive(Deserialize)]
struct Metadata {
    authorization_endpoint: String,
    token_endpoint: String,
    registration_endpoint: Option<String>,
    revocation_endpoint: Option<String>,
    #[serde(default)]
    scopes_supported: Vec<String>,
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    expires_in: Option<u64>,
    refresh_token: Option<String>,
    username: Option<String>,
    /// What was granted; left out by a server that granted what was asked for (RFC 6749 5.1).
    scope: Option<String>,
}

#[derive(Deserialize)]
struct TokenError {
    error: String,
    error_description: Option<String>,
}

/// One client for every OAuth request, so connections are reused. No redirects: an OAuth
/// endpoint that redirects is not one to trust with a refresh token.
static HTTP: LazyLock<std::result::Result<reqwest::Client, String>> = LazyLock::new(|| {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|e| e.to_string())
});

fn http() -> Result<reqwest::Client> {
    HTTP.clone().map_err(|e| Error::temporary(format!("http client: {e}")))
}

/// https, or plain http only to this machine (a test server). Everything the app talks to or
/// opens for a sign-in goes through this, including endpoints a server's metadata names.
pub fn secure_url(s: &str) -> std::result::Result<Url, String> {
    let url = Url::parse(s.trim()).map_err(|e| e.to_string())?;
    match url.scheme() {
        "https" if url.host().is_some() => Ok(url),
        "http" if is_loopback(&url) => Ok(url),
        _ => Err("must use https".into()),
    }
}

fn is_loopback(url: &Url) -> bool {
    match url.host() {
        Some(url::Host::Domain(d)) => d.eq_ignore_ascii_case("localhost"),
        Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
        Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
        None => false,
    }
}

/// `scheme://host[:port]/` of the mail server, whatever path the configured URL has.
pub fn server_base(url: &str) -> std::result::Result<Url, String> {
    let url = Url::parse(url.trim()).map_err(|e| format!("Mail server URL: {e}"))?;
    let host = url.host_str().ok_or("Mail server URL has no host")?;
    let mut base = format!("{}://{}", url.scheme(), host);
    if let Some(port) = url.port() {
        base.push_str(&format!(":{port}"));
    }
    Url::parse(&base).map_err(|e| e.to_string())
}

async fn discover(server_url: &str) -> Result<Metadata> {
    let base = server_base(server_url).map_err(Error::invalid)?;
    let base = secure_url(base.as_str()).map_err(|e| Error::invalid(format!("Mail server URL {e}")))?;
    let url = base.join("/.well-known/oauth-authorization-server").map_err(|e| Error::invalid(e.to_string()))?;
    let resp = http()?.get(url).send().await.map_err(|e| Error::temporary(format!("OAuth discovery: {e}")))?;
    let status = resp.status();
    if !status.is_success() {
        let msg = format!("OAuth discovery: HTTP {status} (does the server support OAuth?)");
        // A 404 means this server does not do OAuth at all; anything else may pass.
        return Err(if status == reqwest::StatusCode::NOT_FOUND { Error::invalid(msg) } else { Error::temporary(msg) });
    }
    let bytes = resp.bytes().await.map_err(|e| Error::temporary(e.to_string()))?;
    serde_json::from_slice(&bytes).map_err(|e| Error::temporary(format!("OAuth discovery: {e}")))
}

/// Refuses endpoints the server's metadata names that are not https.
fn check_endpoints(meta: &Metadata) -> Result<()> {
    let check = |what: &str, u: &str| {
        secure_url(u).map(|_| ()).map_err(|e| Error::invalid(format!("the server's {what} endpoint {e}")))
    };
    check("authorization", &meta.authorization_endpoint)?;
    check("token", &meta.token_endpoint)?;
    if let Some(u) = &meta.registration_endpoint {
        check("registration", u)?;
    }
    if let Some(u) = &meta.revocation_endpoint {
        check("revocation", u)?;
    }
    Ok(())
}

/// What to register as the redirect URI. A loopback redirect is registered without its port,
/// because the port is only known once the listener is bound and servers match any port
/// (RFC 8252 7.3). Anything else (a custom scheme) is registered exactly as the host gave it.
fn registered_redirect(redirect: &Url) -> String {
    match (is_loopback(redirect), redirect.host_str()) {
        (true, Some(host)) if redirect.scheme() == "http" => format!("http://{host}{}", redirect.path()),
        _ => redirect.as_str().to_string(),
    }
}

async fn register(meta: &Metadata, redirect: &str, scope: &str) -> Result<String> {
    let endpoint = meta
        .registration_endpoint
        .as_deref()
        .ok_or_else(|| Error::invalid("the server does not offer client registration"))?;
    let body = serde_json::json!({
        "client_name": CLIENT_NAME,
        "redirect_uris": [redirect],
        "grant_types": ["authorization_code", "refresh_token"],
        "response_types": ["code"],
        "token_endpoint_auth_method": "none",
        "scope": scope,
    });
    let resp = http()?
        .post(endpoint)
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .body(body.to_string())
        .send()
        .await
        .map_err(|e| Error::temporary(format!("client registration: {e}")))?;
    let status = resp.status();
    let bytes = resp.bytes().await.map_err(|e| Error::temporary(e.to_string()))?;
    let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap_or_default();
    if !status.is_success() {
        return Err(Error::temporary(format!(
            "client registration: HTTP {status}: {}",
            value["error_description"].as_str().or(value["error"].as_str()).unwrap_or("no details")
        )));
    }
    value["client_id"].as_str().map(str::to_string).ok_or_else(|| Error::temporary("client registration: no client_id"))
}

pub(crate) fn random_url_safe(bytes: usize) -> Result<String> {
    let mut buf = vec![0u8; bytes];
    getrandom::fill(&mut buf).map_err(|e| Error::temporary(format!("random numbers: {e}")))?;
    Ok(URL_SAFE_NO_PAD.encode(buf))
}

fn form(pairs: &[(&str, &str)]) -> String {
    url::form_urlencoded::Serializer::new(String::new()).extend_pairs(pairs).finish()
}

async fn token_request(endpoint: &str, pairs: &[(&str, &str)]) -> Result<TokenResponse> {
    let endpoint = secure_url(endpoint).map_err(|e| Error::invalid(format!("token endpoint {e}")))?;
    let resp = http()?
        .post(endpoint)
        .header(reqwest::header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(form(pairs))
        .send()
        .await
        .map_err(|e| Error::temporary(format!("token request: {e}")))?;
    let status = resp.status();
    let bytes = resp.bytes().await.map_err(|e| Error::temporary(e.to_string()))?;
    if status.is_success() {
        return serde_json::from_slice(&bytes).map_err(|e| Error::temporary(format!("token response: {e}")));
    }
    match serde_json::from_slice::<TokenError>(&bytes) {
        // RFC 6749 5.2: these mean the grant itself is no good any more.
        Ok(e) if matches!(e.error.as_str(), "invalid_grant" | "invalid_client" | "unauthorized_client") => {
            Err(Error::sign_in_needed(format!("{}: {}", e.error, e.error_description.unwrap_or_default())))
        }
        Ok(e) => Err(Error::temporary(format!("{}: {}", e.error, e.error_description.unwrap_or_default()))),
        Err(_) => Err(Error::temporary(format!("token request: HTTP {status}"))),
    }
}

fn keep(token: &TokenResponse) -> AccessToken {
    // Renew a minute early, and never trust a lifetime under two minutes for longer than half of it.
    // A lifetime under 30 s (or 0) is taken as 30 s: otherwise renewal would loop. If the token
    // really dies sooner, the server's 401 drops it (see `Tokens::invalidate`).
    // Capped at a day: a huge value must not overflow the Instant below.
    let life = token.expires_in.unwrap_or(3600).clamp(30, 86_400);
    let margin = if life > 120 { 60 } else { life / 2 };
    AccessToken { token: token.access_token.clone(), renew_at: Instant::now() + Duration::from_secs(life - margin) }
}

/// Discovery, (re)registration, and the URL to open in the browser.
///
/// `redirect_uri` is chosen by the host: a loopback URL such as `http://127.0.0.1:<port>/callback`
/// or a custom scheme. `previous` is what [`SignedIn::client`] gave last time: its client_id is
/// reused when the server and token endpoint are unchanged (stateless ids are cheap, so any
/// change simply registers again).
pub async fn begin_sign_in(
    server_url: &str,
    redirect_uri: &str,
    previous: Option<&OAuthClient>,
) -> Result<PendingSignIn> {
    let server_url = server_url.trim();
    let redirect = Url::parse(redirect_uri.trim()).map_err(|e| Error::invalid(format!("redirect URI: {e}")))?;
    let meta = discover(server_url).await?;
    check_endpoints(&meta)?;

    let scope = scopes_for(&meta);
    let reusable = previous.filter(|p| {
        p.server_url == server_url
            && p.token_endpoint == meta.token_endpoint
            && !p.client_id.is_empty()
            && p.scope == scope
    });
    let client_id = match reusable {
        Some(p) => p.client_id.clone(),
        None => register(&meta, &registered_redirect(&redirect), &scope).await?,
    };
    let client = OAuthClient {
        server_url: server_url.to_string(),
        client_id,
        token_endpoint: meta.token_endpoint.clone(),
        revocation_endpoint: meta.revocation_endpoint.clone().unwrap_or_default(),
        scope: scope.clone(),
        granted: String::new(),
    };

    let verifier = random_url_safe(32)?;
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    let state = random_url_safe(16)?;
    let mut authorize =
        secure_url(&meta.authorization_endpoint).map_err(|e| Error::invalid(format!("authorization endpoint: {e}")))?;
    authorize
        .query_pairs_mut()
        .append_pair("response_type", "code")
        .append_pair("client_id", &client.client_id)
        .append_pair("redirect_uri", redirect.as_str())
        .append_pair("scope", &scope)
        .append_pair("state", &state)
        .append_pair("code_challenge", &challenge)
        .append_pair("code_challenge_method", "S256");
    Ok(PendingSignIn {
        authorize_url: authorize.to_string(),
        client,
        verifier,
        state,
        redirect_uri: redirect.to_string(),
    })
}

/// Finishes a sign-in. `callback_url` is the full URL the browser was sent back to (with `code`
/// and `state`, or `error`). Checks `state`, exchanges the code, stores the refresh token.
pub async fn complete_sign_in(
    pending: PendingSignIn,
    callback_url: &str,
    secrets: Arc<dyn SecretStore>,
) -> Result<SignedIn> {
    let callback = Url::parse(callback_url.trim()).map_err(|e| Error::invalid(format!("callback URL: {e}")))?;
    let param = |k: &str| callback.query_pairs().find(|(n, _)| n == k).map(|(_, v)| v.into_owned());
    // Checked before anything else, so a forged reply can't even cause a request.
    if param("state").as_deref() != Some(pending.state.as_str()) {
        return Err(Error::invalid("the sign-in reply does not belong to this sign-in"));
    }
    if let Some(err) = param("error") {
        return Err(Error::sign_in_needed(format!("sign-in refused: {err}")));
    }
    let code = param("code").ok_or_else(|| Error::invalid("the sign-in reply had no code"))?;

    let token = token_request(
        &pending.client.token_endpoint,
        &[
            ("grant_type", "authorization_code"),
            ("code", &code),
            ("redirect_uri", &pending.redirect_uri),
            ("client_id", &pending.client.client_id),
            ("code_verifier", &pending.verifier),
        ],
    )
    .await?;
    let refresh = token.refresh_token.clone().ok_or_else(|| Error::temporary("the server issued no refresh token"))?;
    let username = token.username.clone().unwrap_or_default();
    let mut client = pending.client;
    client.granted = token.scope.clone().unwrap_or_else(|| client.scope.clone());
    let tokens = Tokens::started(client.clone(), secrets, &token, &refresh);
    Ok(SignedIn { client, username, tokens })
}
