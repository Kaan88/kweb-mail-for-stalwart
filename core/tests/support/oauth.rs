//! The OAuth side of the mock: client registration, authorization codes with PKCE, access
//! tokens with a lifetime, and refresh tokens that rotate on every use.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use sha2::{Digest, Sha256};
use url::Url;

pub struct Client {
    pub redirect_uris: Vec<String>,
}

pub struct Code {
    pub client_id: String,
    pub redirect_uri: String,
    pub challenge: String,
}

#[derive(Default)]
pub struct Oauth {
    pub clients: HashMap<String, Client>,
    pub codes: HashMap<String, Code>,
    /// refresh token -> client id. A used token is removed, which is the rotation.
    pub refresh: HashMap<String, String>,
    /// access token -> the moment it stops working.
    pub access: HashMap<String, Instant>,
    pub lifetime_secs: u64,
    client_counter: u64,
}

pub struct Issued {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_in: u64,
}

pub fn random_token(prefix: &str) -> String {
    let mut buf = [0u8; 24];
    getrandom::fill(&mut buf).expect("system randomness");
    format!("{prefix}{}", URL_SAFE_NO_PAD.encode(buf))
}

fn is_loopback(url: &Url) -> bool {
    matches!(url.host_str(), Some("127.0.0.1" | "localhost" | "[::1]"))
}

/// RFC 8252 section 7.3: a loopback redirect matches a registration whatever the port is.
fn redirect_matches(registered: &str, requested: &str) -> bool {
    if registered == requested {
        return true;
    }
    match (Url::parse(registered), Url::parse(requested)) {
        (Ok(a), Ok(b)) => {
            is_loopback(&a)
                && is_loopback(&b)
                && a.scheme() == b.scheme()
                && a.host_str() == b.host_str()
                && a.path() == b.path()
        }
        _ => false,
    }
}

impl Oauth {
    pub fn new() -> Oauth {
        Oauth { lifetime_secs: 3600, ..Default::default() }
    }

    pub fn register(&mut self, redirect_uris: Vec<String>) -> String {
        self.client_counter += 1;
        let id = format!("mock-client-{}", self.client_counter);
        self.clients.insert(id.clone(), Client { redirect_uris });
        id
    }

    /// The browser step. Returns the callback URL to send the user back to.
    pub fn authorize(&mut self, params: &HashMap<String, String>) -> Result<String, String> {
        let get = |k: &str| params.get(k).map(String::as_str).unwrap_or("");
        let client =
            self.clients.get(get("client_id")).ok_or_else(|| format!("unknown client_id '{}'", get("client_id")))?;
        let redirect = get("redirect_uri");
        if !client.redirect_uris.iter().any(|r| redirect_matches(r, redirect)) {
            return Err(format!("redirect_uri '{redirect}' is not registered"));
        }
        let mut back = Url::parse(redirect).map_err(|e| e.to_string())?;
        let state = params.get("state").cloned();
        let fail = |mut back: Url, error: &str| {
            {
                let mut q = back.query_pairs_mut();
                q.append_pair("error", error);
                if let Some(s) = &state {
                    q.append_pair("state", s);
                }
            }
            Ok(back.to_string())
        };
        if get("response_type") != "code" {
            return fail(back, "unsupported_response_type");
        }
        // A public client without PKCE is refused, as Stalwart does.
        if get("code_challenge").is_empty() || get("code_challenge_method") != "S256" {
            return fail(back, "invalid_request");
        }
        let code = random_token("code-");
        self.codes.insert(
            code.clone(),
            Code {
                client_id: get("client_id").to_string(),
                redirect_uri: redirect.to_string(),
                challenge: get("code_challenge").to_string(),
            },
        );
        {
            let mut q = back.query_pairs_mut();
            q.append_pair("code", &code);
            if let Some(s) = &state {
                q.append_pair("state", s);
            }
        }
        Ok(back.to_string())
    }

    fn issue(&mut self, client_id: &str) -> Issued {
        let access_token = random_token("at-");
        let refresh_token = random_token("rt-");
        self.access.insert(access_token.clone(), Instant::now() + Duration::from_secs(self.lifetime_secs));
        self.refresh.insert(refresh_token.clone(), client_id.to_string());
        Issued { access_token, refresh_token, expires_in: self.lifetime_secs }
    }

    /// An access token with no OAuth round trip, for tests that are about something else.
    pub fn mint_access_token(&mut self) -> String {
        let t = random_token("at-");
        self.access.insert(t.clone(), Instant::now() + Duration::from_secs(self.lifetime_secs));
        t
    }

    /// Err carries the OAuth error code and a description.
    pub fn token(&mut self, form: &HashMap<String, String>) -> Result<Issued, (&'static str, String)> {
        let get = |k: &str| form.get(k).map(String::as_str).unwrap_or("");
        match get("grant_type") {
            "authorization_code" => {
                // Single use: the code is gone whether or not the rest checks out.
                let code = self
                    .codes
                    .remove(get("code"))
                    .ok_or(("invalid_grant", "unknown or already used code".to_string()))?;
                if code.client_id != get("client_id") {
                    return Err(("invalid_grant", "code was issued to another client".into()));
                }
                if code.redirect_uri != get("redirect_uri") {
                    return Err(("invalid_grant", "redirect_uri does not match the authorization request".into()));
                }
                let verifier = get("code_verifier");
                if verifier.is_empty() {
                    return Err(("invalid_request", "code_verifier is required".into()));
                }
                let computed = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
                if computed != code.challenge {
                    return Err(("invalid_grant", "PKCE verification failed".into()));
                }
                Ok(self.issue(&code.client_id))
            }
            "refresh_token" => {
                let client_id = self
                    .refresh
                    .remove(get("refresh_token"))
                    .ok_or(("invalid_grant", "refresh token is unknown, used or revoked".to_string()))?;
                Ok(self.issue(&client_id))
            }
            other => Err(("unsupported_grant_type", format!("grant_type '{other}'"))),
        }
    }

    pub fn revoke(&mut self, token: &str) {
        self.refresh.remove(token);
        self.access.remove(token);
    }

    pub fn access_valid(&self, token: &str) -> bool {
        self.access.get(token).is_some_and(|until| *until > Instant::now())
    }

    pub fn expire_access_tokens(&mut self) {
        let now = Instant::now();
        for until in self.access.values_mut() {
            *until = now;
        }
    }
}
