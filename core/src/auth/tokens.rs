//! The access token in memory and the refresh-token rotation behind it.

use std::{
    sync::{Arc, Mutex},
    time::Instant,
};

use crate::{
    secrets::{SecretError, SecretStore, REFRESH_TOKEN},
    Error, Result,
};

use super::{form, http, keep, secure_url, token_request, AccessToken, OAuthClient, TokenResponse};

/// Cheap to clone. Owns the in-memory access token and the renewal logic.
#[derive(Clone)]
pub struct Tokens {
    inner: Arc<Inner>,
}

struct Inner {
    client: OAuthClient,
    secrets: Arc<dyn SecretStore>,
    /// The cached access token. Held across a refresh: one rotation at a time.
    access: tokio::sync::Mutex<Option<AccessToken>>,
    /// A rotated refresh token the secret store would not take. The server has already retired
    /// the previous one, so this is the only valid grant: used before the store's and saved
    /// again later. Never held across an await, so `is_signed_in` can read it without waiting.
    unsaved: Mutex<Option<String>>,
}

impl Tokens {
    /// For a client signed in earlier: the refresh token is read from `secrets` when needed.
    pub fn new(client: OAuthClient, secrets: Arc<dyn SecretStore>) -> Self {
        Self::build(client, secrets, None)
    }

    /// Right after a sign-in: the access token is already in hand.
    pub(super) fn started(
        client: OAuthClient,
        secrets: Arc<dyn SecretStore>,
        token: &TokenResponse,
        refresh: &str,
    ) -> Self {
        let tokens = Self::build(client, secrets, Some(keep(token)));
        // The grant is good even if the store is not: keep it in memory and retry later.
        tokens.inner.save_refresh(refresh);
        tokens
    }

    fn build(client: OAuthClient, secrets: Arc<dyn SecretStore>, access: Option<AccessToken>) -> Self {
        let inner = Inner { client, secrets, access: tokio::sync::Mutex::new(access), unsaved: Mutex::new(None) };
        Tokens { inner: Arc::new(inner) }
    }

    pub fn client(&self) -> &OAuthClient {
        &self.inner.client
    }

    /// False only when there is certainly no grant. A secret store that cannot be read right
    /// now is not a sign-out.
    pub fn is_signed_in(&self) -> bool {
        !self.inner.client.client_id.is_empty() && !matches!(self.inner.current_refresh(), Ok(None))
    }

    /// A valid access token, refreshing (and rotating the stored refresh token) when needed.
    /// The refresh runs as its own task: if the caller gives up half-way (a timeout, a restarted
    /// watcher), a token the server has already rotated is still stored instead of being lost.
    pub async fn access_token(&self) -> Result<AccessToken> {
        let inner = self.inner.clone();
        tokio::spawn(async move { inner.refresh_or_cached().await })
            .await
            .unwrap_or_else(|e| Err(Error::temporary(format!("token task: {e}"))))
    }

    /// The server answered 401 with the cached token: drop it so the next call refreshes. If a
    /// refresh is running, this waits for it and then drops its result too; the cost is one
    /// extra refresh, never a lost token.
    pub async fn invalidate(&self) {
        *self.inner.access.lock().await = None;
    }

    /// Revokes the refresh token on the server (best effort) and forgets everything locally.
    /// Holds the token lock throughout, so a refresh in flight can't store a new grant
    /// afterwards. Returns whether the server confirmed the revocation.
    pub async fn sign_out(&self) -> bool {
        let mut cached = self.inner.access.lock().await;
        let revoked = self.inner.revoke().await;
        self.inner.forget(&mut cached);
        revoked
    }
}

impl Inner {
    fn save_refresh(&self, token: &str) {
        let saved = self.secrets.set(REFRESH_TOKEN.to_string(), token.to_string());
        *self.unsaved.lock().unwrap() = saved.is_err().then(|| token.to_string());
    }

    /// The refresh token in use: one the store refused to take, else the store's. An error
    /// means the store could not be read just now (not that there is no token).
    fn current_refresh(&self) -> std::result::Result<Option<String>, SecretError> {
        if let Some(t) = self.unsaved.lock().unwrap().clone() {
            return Ok(Some(t));
        }
        self.secrets.get(REFRESH_TOKEN.to_string())
    }

    /// Tries again to save a refresh token the store refused earlier.
    fn retry_unsaved(&self) {
        let mut unsaved = self.unsaved.lock().unwrap();
        if let Some(token) = unsaved.as_deref() {
            if self.secrets.set(REFRESH_TOKEN.to_string(), token.to_string()).is_ok() {
                *unsaved = None;
            }
        }
    }

    /// Forgets the grant locally (the caller holds the token lock).
    fn forget(&self, cached: &mut Option<AccessToken>) {
        // If delete fails the token stays in the store, but the next sign-in overwrites it.
        let _ = self.secrets.delete(REFRESH_TOKEN.to_string());
        *self.unsaved.lock().unwrap() = None;
        *cached = None;
    }

    async fn refresh_or_cached(&self) -> Result<AccessToken> {
        let mut cached = self.access.lock().await; // held across the refresh: one rotation at a time
        if let Some(t) = cached.as_ref().filter(|t| Instant::now() < t.renew_at) {
            self.retry_unsaved();
            return Ok(t.clone());
        }
        let refresh = match self.current_refresh() {
            Ok(Some(r)) if !self.client.client_id.is_empty() => r,
            Ok(_) => return Err(Error::sign_in_needed("not signed in")),
            Err(e) => return Err(Error::temporary(format!("secret store: {e}"))),
        };
        let token = match token_request(
            &self.client.token_endpoint,
            &[("grant_type", "refresh_token"), ("refresh_token", &refresh), ("client_id", &self.client.client_id)],
        )
        .await
        {
            Ok(t) => t,
            Err(e @ Error::SignInNeeded { .. }) => {
                // The server no longer takes this grant: drop it, so the app offers sign-in again.
                self.forget(&mut cached);
                return Err(e);
            }
            Err(e) => return Err(e),
        };
        // The new access token is good whatever happens to the store below.
        let t = keep(&token);
        *cached = Some(t.clone());
        if let Some(rotated) = token.refresh_token.as_deref() {
            // The server has retired the old refresh token: the rotated one must not be lost.
            self.save_refresh(rotated);
        }
        Ok(t)
    }

    async fn revoke(&self) -> bool {
        let Ok(Some(refresh)) = self.current_refresh() else { return false };
        let Ok(endpoint) = secure_url(&self.client.revocation_endpoint) else { return false };
        let Ok(client) = http() else { return false };
        let resp = client
            .post(endpoint)
            .header(reqwest::header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .body(form(&[
                ("token", &refresh),
                ("token_type_hint", "refresh_token"),
                ("client_id", &self.client.client_id),
            ]))
            .send()
            .await;
        matches!(resp, Ok(r) if r.status().is_success())
    }
}
