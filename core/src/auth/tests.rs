use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};

use axum::{
    body::Bytes,
    extract::State,
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use serde_json::{json, Value};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

use super::*;
use crate::secrets::{MemorySecrets, SecretError};

// ---- unit tests ported from Postern ----

#[test]
fn server_base_strips_path() {
    assert_eq!(server_base("https://mail.example.com/jmap/session").unwrap().as_str(), "https://mail.example.com/");
    assert_eq!(server_base("https://mail.example.com:8443").unwrap().as_str(), "https://mail.example.com:8443/");
}

#[test]
fn only_https_or_loopback_http() {
    assert!(secure_url("https://mail.example.com/x").is_ok());
    assert!(secure_url("http://localhost:8080").is_ok());
    assert!(secure_url("http://127.0.0.1/").is_ok());
    assert!(secure_url("http://[::1]/").is_ok());
    assert!(secure_url("http://mail.example.com").is_err());
    assert!(secure_url("http://localhost.evil.com").is_err());
    assert!(secure_url("javascript:alert(1)").is_err());
    assert!(secure_url("file:///C:/x").is_err());
}

#[test]
fn pkce_challenge_matches_rfc7636_example() {
    // RFC 7636 Appendix B
    let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
    assert_eq!(
        URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes())),
        "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
    );
}

#[test]
fn tiny_or_zero_lifetimes_do_not_renew_at_once() {
    let token = |life| TokenResponse {
        access_token: "t".into(),
        expires_in: Some(life),
        refresh_token: None,
        username: None,
        scope: None,
    };
    for life in [0, 1, 10] {
        assert!(keep(&token(life)).renew_at >= Instant::now() + Duration::from_secs(14));
    }
    assert!(keep(&token(3600)).renew_at >= Instant::now() + Duration::from_secs(3539));
}

#[test]
fn loopback_redirect_is_registered_without_port() {
    let reg = |s: &str| registered_redirect(&Url::parse(s).unwrap());
    assert_eq!(reg("http://127.0.0.1:5555/callback"), "http://127.0.0.1/callback");
    assert_eq!(reg("http://[::1]:5555/callback"), "http://[::1]/callback");
    assert_eq!(reg("ovh.kweb.mail:/oauth"), "ovh.kweb.mail:/oauth");
}

// ---- loopback listener ----

async fn get_path(port: u16, path: &str) -> String {
    let mut s = tokio::net::TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    s.write_all(format!("GET {path} HTTP/1.1\r\nHost: x\r\n\r\n").as_bytes()).await.unwrap();
    let mut out = String::new();
    s.read_to_string(&mut out).await.unwrap();
    out
}

fn port_of(redirect: &str) -> u16 {
    Url::parse(redirect).unwrap().port().unwrap()
}

#[tokio::test(flavor = "current_thread")]
async fn loopback_returns_callback_url_and_checks_state() {
    let (listener, redirect) = loopback::bind().await.unwrap();
    assert!(redirect.starts_with("http://127.0.0.1:") && redirect.ends_with("/callback"));
    let port = port_of(&redirect);
    let waiter = tokio::spawn(async move { loopback::wait_for_callback(listener, "s1", "Test").await });
    assert!(get_path(port, "/favicon.ico").await.starts_with("HTTP/1.1 404"));
    assert!(get_path(port, "/callback?code=abc&state=s1").await.contains("Signed in"));
    let url = Url::parse(&waiter.await.unwrap().unwrap()).unwrap();
    assert_eq!(url.path(), "/callback");
    assert!(url.query_pairs().any(|(k, v)| k == "code" && v == "abc"));
}

#[tokio::test(flavor = "current_thread")]
async fn loopback_hands_back_a_refusal_for_the_caller_to_judge() {
    let (listener, redirect) = loopback::bind().await.unwrap();
    let port = port_of(&redirect);
    let waiter = tokio::spawn(async move { loopback::wait_for_callback(listener, "s1", "Test").await });
    assert!(get_path(port, "/callback?error=access_denied&state=s1").await.contains("Not signed in"));
    assert!(waiter.await.unwrap().unwrap().contains("error=access_denied"));
}

#[tokio::test(flavor = "current_thread")]
async fn loopback_ignores_wrong_state_and_keeps_waiting() {
    let (listener, redirect) = loopback::bind().await.unwrap();
    let port = port_of(&redirect);
    let waiter = tokio::spawn(async move { loopback::wait_for_callback(listener, "s1", "Test").await });
    assert!(get_path(port, "/callback?code=evil&state=forged").await.starts_with("HTTP/1.1 400"));
    assert!(get_path(port, "/callback?error=access_denied").await.starts_with("HTTP/1.1 400"));
    assert!(get_path(port, "/callback?code=abc&state=s1").await.contains("Signed in"));
    assert!(waiter.await.unwrap().unwrap().contains("code=abc"));
}

#[tokio::test(flavor = "current_thread")]
async fn loopback_survives_an_idle_preconnect() {
    let (listener, redirect) = loopback::bind().await.unwrap();
    let port = port_of(&redirect);
    let waiter = tokio::spawn(async move { loopback::wait_for_callback(listener, "s1", "Test").await });
    // A browser's spare socket: connects first, never sends anything.
    let _idle = tokio::net::TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    let reply = tokio::time::timeout(Duration::from_secs(3), get_path(port, "/callback?code=abc&state=s1")).await;
    assert!(reply.expect("the real request was not served behind the idle socket").contains("Signed in"));
    assert!(waiter.await.unwrap().unwrap().contains("code=abc"));
}

// ---- a tiny OAuth server ----

struct Srv {
    base: String,
    /// The one refresh token the server accepts right now.
    valid_refresh: String,
    rotations: u32,
    refresh_calls: u32,
    code_calls: u32,
    revocations: u32,
    registrations: Vec<Value>,
    delay: Duration,
    reject: bool,
    expires_in: u64,
    token_endpoint_override: Option<String>,
    /// What the metadata lists as `scopes_supported`; `None` leaves the field out.
    scopes_supported: Option<Vec<String>>,
    /// What the token answer of a sign-in says was granted; `None` leaves the field out.
    granted: Option<String>,
    last_code_form: Vec<(String, String)>,
}

type Shared = Arc<Mutex<Srv>>;

async fn metadata(State(s): State<Shared>) -> Json<Value> {
    let s = s.lock().unwrap();
    let mut meta = json!({
        "authorization_endpoint": format!("{}/authorize", s.base),
        "token_endpoint": s.token_endpoint_override.clone().unwrap_or(format!("{}/token", s.base)),
        "registration_endpoint": format!("{}/register", s.base),
        "revocation_endpoint": format!("{}/revoke", s.base),
    });
    if let Some(scopes) = &s.scopes_supported {
        meta["scopes_supported"] = json!(scopes);
    }
    Json(meta)
}

async fn registration(State(s): State<Shared>, Json(body): Json<Value>) -> (StatusCode, Json<Value>) {
    let mut s = s.lock().unwrap();
    s.registrations.push(body);
    (StatusCode::CREATED, Json(json!({ "client_id": format!("client-{}", s.registrations.len()) })))
}

async fn token(State(s): State<Shared>, body: Bytes) -> (StatusCode, Json<Value>) {
    let form: Vec<(String, String)> = url::form_urlencoded::parse(&body).into_owned().collect();
    let field = |k: &str| form.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone()).unwrap_or_default();
    let delay;
    let reply = {
        let mut s = s.lock().unwrap();
        delay = s.delay;
        match field("grant_type").as_str() {
            "authorization_code" if field("code") == "good-code" => {
                s.code_calls += 1;
                s.last_code_form = form.clone();
                s.rotations += 1;
                s.valid_refresh = format!("R{}", s.rotations);
                let mut answer = json!({
                    "access_token": "A0", "expires_in": s.expires_in,
                    "refresh_token": s.valid_refresh, "username": "alice@example.com",
                });
                if let Some(granted) = &s.granted {
                    answer["scope"] = json!(granted);
                }
                (StatusCode::OK, answer)
            }
            "refresh_token" => {
                s.refresh_calls += 1;
                if s.reject || field("refresh_token") != s.valid_refresh {
                    (StatusCode::BAD_REQUEST, json!({ "error": "invalid_grant", "error_description": "gone" }))
                } else {
                    s.rotations += 1;
                    s.valid_refresh = format!("R{}", s.rotations);
                    (
                        StatusCode::OK,
                        json!({
                            "access_token": format!("A{}", s.rotations), "expires_in": s.expires_in,
                            "refresh_token": s.valid_refresh,
                        }),
                    )
                }
            }
            _ => (StatusCode::BAD_REQUEST, json!({ "error": "invalid_grant" })),
        }
    };
    // Answered late on purpose in some tests; the state above is already updated.
    tokio::time::sleep(delay).await;
    (reply.0, Json(reply.1))
}

async fn revoke(State(s): State<Shared>) -> StatusCode {
    s.lock().unwrap().revocations += 1;
    StatusCode::OK
}

async fn start_server() -> (String, Shared) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let shared: Shared = Arc::new(Mutex::new(Srv {
        base: base.clone(),
        valid_refresh: String::new(),
        rotations: 0,
        refresh_calls: 0,
        code_calls: 0,
        revocations: 0,
        registrations: vec![],
        delay: Duration::ZERO,
        reject: false,
        expires_in: 3600,
        token_endpoint_override: None,
        scopes_supported: None,
        granted: None,
        last_code_form: vec![],
    }));
    let app = Router::new()
        .route("/.well-known/oauth-authorization-server", get(metadata))
        .route("/register", post(registration))
        .route("/token", post(token))
        .route("/revoke", post(revoke))
        .with_state(shared.clone());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (base, shared)
}

/// A store whose `set` can be made to fail, like a locked keystore.
#[derive(Default)]
struct FlakySecrets {
    inner: MemorySecrets,
    fail_set: AtomicBool,
}

impl SecretStore for FlakySecrets {
    fn get(&self, key: String) -> std::result::Result<Option<String>, SecretError> {
        self.inner.get(key)
    }
    fn set(&self, key: String, value: String) -> std::result::Result<(), SecretError> {
        if self.fail_set.load(Ordering::SeqCst) {
            return Err(SecretError::Failed { msg: "locked".into() });
        }
        self.inner.set(key, value)
    }
    fn delete(&self, key: String) -> std::result::Result<(), SecretError> {
        self.inner.delete(key)
    }
}

const REDIRECT: &str = "http://127.0.0.1:5555/callback";

fn stored(secrets: &dyn SecretStore) -> Option<String> {
    secrets.get(REFRESH_TOKEN.into()).unwrap()
}

use crate::secrets::REFRESH_TOKEN;

/// Runs a whole sign-in against the test server and returns the result.
async fn sign_in(base: &str, secrets: Arc<dyn SecretStore>) -> SignedIn {
    let pending = begin_sign_in(base, REDIRECT, None).await.unwrap();
    let callback = format!("{REDIRECT}?code=good-code&state={}", pending.state());
    complete_sign_in(pending, &callback, secrets).await.unwrap()
}

fn client_for(base: &str) -> OAuthClient {
    OAuthClient {
        server_url: base.into(),
        client_id: "client-1".into(),
        token_endpoint: format!("{base}/token"),
        revocation_endpoint: format!("{base}/revoke"),
        scope: SCOPES.into(),
        granted: String::new(),
    }
}

/// Tokens for a server that already issued refresh token `R1`, with nothing cached.
fn returning_user(base: &str, shared: &Shared, secrets: Arc<dyn SecretStore>) -> Tokens {
    {
        let mut s = shared.lock().unwrap();
        s.rotations = 1;
        s.valid_refresh = "R1".into();
    }
    secrets.set(REFRESH_TOKEN.into(), "R1".into()).unwrap();
    Tokens::new(client_for(base), secrets)
}

// ---- flow tests ----

#[tokio::test]
async fn sign_in_round_trip_then_refresh_rotates() {
    let (base, shared) = start_server().await;
    let secrets = Arc::new(MemorySecrets::default());
    let pending = begin_sign_in(&format!("{base}/jmap/session"), REDIRECT, None).await.unwrap();

    // What was registered and what the browser is told.
    {
        let s = shared.lock().unwrap();
        assert_eq!(s.registrations.len(), 1);
        assert_eq!(s.registrations[0]["redirect_uris"], json!(["http://127.0.0.1/callback"]));
        assert_eq!(s.registrations[0]["client_name"], "kweb mail");
        assert_eq!(s.registrations[0]["token_endpoint_auth_method"], "none");
    }
    let auth = Url::parse(&pending.authorize_url).unwrap();
    assert_eq!(auth.path(), "/authorize");
    let q = |k: &str| auth.query_pairs().find(|(n, _)| n == k).map(|(_, v)| v.into_owned()).unwrap();
    assert_eq!(q("response_type"), "code");
    assert_eq!(q("client_id"), "client-1");
    assert_eq!(q("redirect_uri"), REDIRECT);
    assert_eq!(q("state"), pending.state());
    assert_eq!(q("code_challenge_method"), "S256");
    assert_eq!(q("code_challenge"), URL_SAFE_NO_PAD.encode(Sha256::digest(pending.verifier.as_bytes())));
    assert!(q("scope").contains("urn:ietf:params:oauth:scope:contacts"));

    let callback = format!("{REDIRECT}?code=good-code&state={}", pending.state());
    let verifier = pending.verifier.clone();
    let signed = complete_sign_in(pending, &callback, secrets.clone()).await.unwrap();
    assert_eq!(signed.username, "alice@example.com");
    assert_eq!(signed.client.client_id, "client-1");
    assert_eq!(signed.client.server_url, format!("{base}/jmap/session"));
    assert_eq!(signed.client.token_endpoint, format!("{base}/token"));
    assert_eq!(signed.client.revocation_endpoint, format!("{base}/revoke"));
    assert_eq!(stored(secrets.as_ref()).as_deref(), Some("R1"));
    {
        let s = shared.lock().unwrap();
        let sent = |k: &str| s.last_code_form.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone());
        assert_eq!(sent("code_verifier"), Some(verifier));
        assert_eq!(sent("redirect_uri").as_deref(), Some(REDIRECT));
    }

    // The access token from the sign-in is used without asking the server.
    let tokens = signed.tokens;
    assert!(tokens.is_signed_in());
    assert_eq!(tokens.access_token().await.unwrap().token, "A0");
    assert_eq!(shared.lock().unwrap().refresh_calls, 0);

    // After a 401 the next call refreshes and the rotated token is stored.
    tokens.invalidate().await;
    assert_eq!(tokens.access_token().await.unwrap().token, "A2");
    assert_eq!(stored(secrets.as_ref()).as_deref(), Some("R2"));
    tokens.invalidate().await;
    assert_eq!(tokens.access_token().await.unwrap().token, "A3");
    assert_eq!(stored(secrets.as_ref()).as_deref(), Some("R3"));
    assert_eq!(shared.lock().unwrap().refresh_calls, 2);
}

#[tokio::test]
async fn previous_client_is_reused_only_while_the_server_is_unchanged() {
    let (base, shared) = start_server().await;
    let first = begin_sign_in(&base, REDIRECT, None).await.unwrap();
    let again = begin_sign_in(&base, REDIRECT, Some(&first.client)).await.unwrap();
    assert_eq!(again.client.client_id, first.client.client_id);
    assert_eq!(shared.lock().unwrap().registrations.len(), 1);

    let mut other = first.client.clone();
    other.token_endpoint = "https://elsewhere.example/token".into();
    let fresh = begin_sign_in(&base, REDIRECT, Some(&other)).await.unwrap();
    assert_eq!(fresh.client.client_id, "client-2");
}

#[tokio::test]
async fn file_storage_is_asked_for_only_where_the_server_offers_it() {
    let (base, shared) = start_server().await;
    let scope_of = |p: &PendingSignIn| {
        let url = Url::parse(&p.authorize_url).unwrap();
        let found = url.query_pairs().find(|(n, _)| n == "scope").map(|(_, v)| v.into_owned());
        found.unwrap()
    };
    // A server that does not list the scope (or lists none) is not asked for it.
    let plain = begin_sign_in(&base, REDIRECT, None).await.unwrap();
    assert_eq!(scope_of(&plain), SCOPES);
    assert_eq!(shared.lock().unwrap().registrations[0]["scope"], SCOPES);

    // One that offers it is asked, and a client registered without it registers again: this
    // is also what a client stored by an older version (no scope recorded) goes through.
    shared.lock().unwrap().scopes_supported = Some(vec!["openid".into(), FILES_SCOPE.into()]);
    let with_files = begin_sign_in(&base, REDIRECT, Some(&plain.client)).await.unwrap();
    assert!(scope_of(&with_files).ends_with(FILES_SCOPE));
    assert_eq!(with_files.client.client_id, "client-2");
    assert_eq!(shared.lock().unwrap().registrations[1]["scope"], scope_of(&with_files));
    let old: OAuthClient = serde_json::from_value(json!({
        "server_url": base, "client_id": "client-2", "token_endpoint": format!("{base}/token"),
        "revocation_endpoint": "",
    }))
    .unwrap();
    assert_eq!(begin_sign_in(&base, REDIRECT, Some(&old)).await.unwrap().client.client_id, "client-3");
    // Unchanged, it is reused.
    let again = begin_sign_in(&base, REDIRECT, Some(&with_files.client)).await.unwrap();
    assert_eq!(again.client.client_id, "client-2");
}

#[tokio::test]
async fn account_scopes_are_asked_for_only_where_the_server_offers_them() {
    let (base, shared) = start_server().await;
    let scope_of = |p: &PendingSignIn| {
        let url = Url::parse(&p.authorize_url).unwrap();
        let found = url.query_pairs().find(|(n, _)| n == "scope").map(|(_, v)| v.into_owned());
        found.unwrap()
    };
    let plain = begin_sign_in(&base, REDIRECT, None).await.unwrap();
    assert_eq!(scope_of(&plain), SCOPES);

    // Only one of them offered: only that one is asked for, and the client registers again.
    shared.lock().unwrap().scopes_supported = Some(vec!["openid".into(), SETTINGS_SCOPE.into()]);
    let settings = begin_sign_in(&base, REDIRECT, Some(&plain.client)).await.unwrap();
    assert_eq!(scope_of(&settings), format!("{SCOPES} {SETTINGS_SCOPE}"));
    assert_eq!(settings.client.client_id, "client-2");

    shared.lock().unwrap().scopes_supported =
        Some(vec!["openid".into(), FILES_SCOPE.into(), SECURITY_SCOPE.into(), SETTINGS_SCOPE.into()]);
    let all = begin_sign_in(&base, REDIRECT, Some(&settings.client)).await.unwrap();
    assert_eq!(scope_of(&all), format!("{SCOPES} {FILES_SCOPE} {SETTINGS_SCOPE} {SECURITY_SCOPE}"));
    assert_eq!(all.client.client_id, "client-3");
    // Unchanged, it is reused.
    let again = begin_sign_in(&base, REDIRECT, Some(&all.client)).await.unwrap();
    assert_eq!(again.client.client_id, "client-3");

    let offered = account_scopes_offered(&base).await.unwrap();
    assert_eq!(offered, AccountScopes { settings: true, security: true });
    shared.lock().unwrap().scopes_supported = Some(vec!["openid".into(), SECURITY_SCOPE.into()]);
    assert_eq!(account_scopes_offered(&base).await.unwrap(), AccountScopes { settings: false, security: true });
}

#[tokio::test]
async fn the_granted_scopes_are_kept_with_the_client() {
    let (base, shared) = start_server().await;
    // The answer names what was granted: that is what counts, not what was asked.
    shared.lock().unwrap().granted = Some(format!("openid {SETTINGS_SCOPE}"));
    let signed = sign_in(&base, Arc::new(MemorySecrets::default())).await;
    assert!(signed.client.was_granted(SETTINGS_SCOPE));
    assert!(!signed.client.was_granted(SECURITY_SCOPE));
    assert!(!signed.client.was_granted("urn:stalwart:oauth:scope:sett"));

    // An answer without the field means what was asked for was granted.
    shared.lock().unwrap().granted = None;
    shared.lock().unwrap().scopes_supported = Some(vec![SECURITY_SCOPE.into()]);
    let signed = sign_in(&base, Arc::new(MemorySecrets::default())).await;
    assert!(signed.client.was_granted(SECURITY_SCOPE));
    assert!(signed.client.was_granted("openid"));

    // A client stored by an older version has no record: it counts as granted none.
    let old: OAuthClient = serde_json::from_value(json!({
        "server_url": base, "client_id": "client-2", "token_endpoint": format!("{base}/token"),
        "revocation_endpoint": "", "scope": SCOPES,
    }))
    .unwrap();
    assert!(!old.was_granted("openid"));
    assert!(!old.was_granted(SETTINGS_SCOPE));
}

#[tokio::test]
async fn custom_scheme_redirect_is_registered_as_given() {
    let (base, shared) = start_server().await;
    let pending = begin_sign_in(&base, "ovh.kweb.mail:/oauth", None).await.unwrap();
    assert_eq!(shared.lock().unwrap().registrations[0]["redirect_uris"], json!(["ovh.kweb.mail:/oauth"]));
    let callback = format!("ovh.kweb.mail:/oauth?code=good-code&state={}", pending.state());
    let signed = complete_sign_in(pending, &callback, Arc::new(MemorySecrets::default())).await.unwrap();
    assert!(signed.tokens.is_signed_in());
}

#[tokio::test]
async fn endpoints_that_are_not_https_are_refused() {
    let (base, shared) = start_server().await;
    shared.lock().unwrap().token_endpoint_override = Some("http://evil.example/token".into());
    let err = begin_sign_in(&base, REDIRECT, None).await.err().unwrap();
    assert!(matches!(err, Error::Invalid { .. }), "{err:?}");
    assert!(matches!(begin_sign_in("http://mail.example.com", REDIRECT, None).await, Err(Error::Invalid { .. })));
    assert!(matches!(begin_sign_in(&base, "not a url", None).await, Err(Error::Invalid { .. })));
}

#[tokio::test]
async fn wrong_state_is_refused_without_contacting_the_server() {
    let (base, shared) = start_server().await;
    let pending = begin_sign_in(&base, REDIRECT, None).await.unwrap();
    let err = complete_sign_in(
        pending,
        &format!("{REDIRECT}?code=good-code&state=forged"),
        Arc::new(MemorySecrets::default()),
    )
    .await
    .err()
    .unwrap();
    assert!(matches!(err, Error::Invalid { .. }), "{err:?}");
    assert_eq!(shared.lock().unwrap().code_calls, 0);
}

#[tokio::test]
async fn access_denied_callback_is_an_error() {
    let (base, shared) = start_server().await;
    let secrets = Arc::new(MemorySecrets::default());
    let pending = begin_sign_in(&base, REDIRECT, None).await.unwrap();
    let callback = format!("{REDIRECT}?error=access_denied&state={}", pending.state());
    let err = complete_sign_in(pending, &callback, secrets.clone()).await.err().unwrap();
    assert!(matches!(err, Error::SignInNeeded { .. }), "{err:?}");
    assert!(err.to_string().contains("access_denied"));
    assert_eq!(shared.lock().unwrap().code_calls, 0);
    assert_eq!(stored(secrets.as_ref()), None);
}

#[tokio::test]
async fn invalid_grant_forgets_the_grant_and_asks_for_sign_in() {
    let (base, shared) = start_server().await;
    let secrets = Arc::new(MemorySecrets::default());
    let tokens = returning_user(&base, &shared, secrets.clone());
    assert!(tokens.is_signed_in());
    shared.lock().unwrap().reject = true;
    let err = tokens.access_token().await.err().unwrap();
    assert!(matches!(err, Error::SignInNeeded { .. }), "{err:?}");
    assert_eq!(stored(secrets.as_ref()), None);
    assert!(!tokens.is_signed_in());
    // And it stays that way without another request.
    let calls = shared.lock().unwrap().refresh_calls;
    assert!(matches!(tokens.access_token().await, Err(Error::SignInNeeded { .. })));
    assert_eq!(shared.lock().unwrap().refresh_calls, calls);
}

#[tokio::test]
async fn refused_save_keeps_the_rotated_token_and_saves_it_later() {
    let (base, shared) = start_server().await;
    let secrets = Arc::new(FlakySecrets::default());
    let tokens = returning_user(&base, &shared, secrets.clone());

    secrets.fail_set.store(true, Ordering::SeqCst);
    assert_eq!(tokens.access_token().await.unwrap().token, "A2"); // R1 -> R2, save refused
    assert_eq!(stored(secrets.as_ref()).as_deref(), Some("R1")); // the store still has the retired one
    assert!(tokens.is_signed_in());

    // The next refresh must use R2 from memory, not R1 from the store (the server would refuse R1).
    tokens.invalidate().await;
    assert_eq!(tokens.access_token().await.unwrap().token, "A3"); // R2 -> R3, save refused again
    assert_eq!(shared.lock().unwrap().refresh_calls, 2);

    // Once the store works, any call that finds a valid cached token saves the pending one.
    secrets.fail_set.store(false, Ordering::SeqCst);
    assert_eq!(tokens.access_token().await.unwrap().token, "A3");
    assert_eq!(stored(secrets.as_ref()).as_deref(), Some("R3"));
    assert_eq!(shared.lock().unwrap().refresh_calls, 2);
}

#[tokio::test]
async fn unreadable_store_is_temporary_not_a_sign_out() {
    struct Unreadable;
    impl SecretStore for Unreadable {
        fn get(&self, _: String) -> std::result::Result<Option<String>, SecretError> {
            Err(SecretError::Failed { msg: "locked".into() })
        }
        fn set(&self, _: String, _: String) -> std::result::Result<(), SecretError> {
            Ok(())
        }
        fn delete(&self, _: String) -> std::result::Result<(), SecretError> {
            Ok(())
        }
    }
    let tokens = Tokens::new(client_for("https://mail.example.com"), Arc::new(Unreadable));
    assert!(tokens.is_signed_in());
    assert!(matches!(tokens.access_token().await, Err(Error::Temporary { .. })));
}

#[tokio::test]
async fn no_stored_token_means_signed_out() {
    let tokens = Tokens::new(client_for("https://mail.example.com"), Arc::new(MemorySecrets::default()));
    assert!(!tokens.is_signed_in());
    assert!(matches!(tokens.access_token().await, Err(Error::SignInNeeded { .. })));
}

#[tokio::test]
async fn ten_concurrent_callers_cause_one_refresh() {
    let (base, shared) = start_server().await;
    let secrets = Arc::new(MemorySecrets::default());
    let tokens = returning_user(&base, &shared, secrets.clone());
    shared.lock().unwrap().delay = Duration::from_millis(100);
    let calls = (0..10).map(|_| tokens.access_token());
    let results = futures_util::future::join_all(calls).await;
    assert!(results.iter().all(|r| r.as_ref().unwrap().token == "A2"));
    assert_eq!(shared.lock().unwrap().refresh_calls, 1);
    assert_eq!(stored(secrets.as_ref()).as_deref(), Some("R2"));
}

#[tokio::test]
async fn a_caller_that_gives_up_mid_refresh_does_not_lose_the_rotated_token() {
    let (base, shared) = start_server().await;
    let secrets = Arc::new(MemorySecrets::default());
    let tokens = returning_user(&base, &shared, secrets.clone());
    shared.lock().unwrap().delay = Duration::from_millis(300);

    let gave_up = tokio::time::timeout(Duration::from_millis(50), tokens.access_token()).await;
    assert!(gave_up.is_err(), "the caller should have been cut off");
    assert_eq!(stored(secrets.as_ref()).as_deref(), Some("R1")); // not yet answered

    tokio::time::sleep(Duration::from_millis(700)).await;
    assert_eq!(stored(secrets.as_ref()).as_deref(), Some("R2"));
    // And the renewed access token is there for the next caller, with no second refresh.
    assert_eq!(tokens.access_token().await.unwrap().token, "A2");
    assert_eq!(shared.lock().unwrap().refresh_calls, 1);
}

#[tokio::test]
async fn sign_out_revokes_and_forgets() {
    let (base, shared) = start_server().await;
    let secrets = Arc::new(MemorySecrets::default());
    let signed = sign_in(&base, secrets.clone()).await;
    assert!(signed.tokens.sign_out().await);
    assert_eq!(shared.lock().unwrap().revocations, 1);
    assert_eq!(stored(secrets.as_ref()), None);
    assert!(!signed.tokens.is_signed_in());
    assert!(matches!(signed.tokens.access_token().await, Err(Error::SignInNeeded { .. })));
}
