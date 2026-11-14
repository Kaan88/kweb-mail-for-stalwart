//! How the admin functions reach the server: with the API key the administrator gave this
//! account, never with the sign-in's own token (the server keeps admin objects away from it).

use std::future::Future;

use super::{not_signed_in, Core};
use crate::{jmap::Connection, Error, Result};

/// The secret-store name of the account's admin API key.
pub(super) const ADMIN_KEY: &str = "admin_key";
/// Whom the stored key was accepted for: the server and the account (see [`Core::admin_binding`]).
pub(super) const ADMIN_KEY_FOR: &str = "admin_key_for";
/// Set when the server turned the stored key down (401): it is not sent again until a new key
/// is pasted or this one is forgotten.
pub(super) const ADMIN_KEY_REFUSED: &str = "admin_key_refused";

/// Whether `key` can be sent as a header value at all: printable ASCII, no white space. (The
/// HTTP client panics on anything else.)
pub(super) fn is_plausible_key(key: &str) -> bool {
    !key.is_empty() && key.bytes().all(|b| (0x21..=0x7e).contains(&b))
}

impl Core {
    /// What the stored key is for: the server's address and the signed-in account.
    pub(super) fn admin_binding(&self) -> Result<String> {
        let inner = self.lock();
        let account = &inner.session.as_ref().ok_or_else(not_signed_in)?.account;
        let origin = account.server_url.trim().trim_end_matches('/').to_lowercase();
        Ok(format!("{origin}\n{}\n{}", account.id, account.username.to_lowercase()))
    }

    /// Stores `key` for the signed-in account on this server.
    pub(super) fn store_admin_key(&self, key: String) -> Result<()> {
        let binding = self.admin_binding()?;
        let set = |name: &str, value: String| self.secrets.set(name.into(), value).map_err(|_| key_store_error());
        set(ADMIN_KEY_FOR, binding)?;
        set(ADMIN_KEY, key)?;
        let _ = self.secrets.delete(ADMIN_KEY_REFUSED.into());
        Ok(())
    }

    /// Removes the key and everything kept beside it.
    pub(super) fn drop_admin_key(&self) -> Result<()> {
        let gone = |name: &str| self.secrets.delete(name.into()).map_err(|_| key_store_error());
        let refused = gone(ADMIN_KEY_REFUSED);
        let bound = gone(ADMIN_KEY_FOR);
        gone(ADMIN_KEY).and(refused).and(bound)
    }

    /// Whether the server turned the stored key down before.
    pub(super) fn admin_key_refused_before(&self) -> bool {
        self.secrets.get(ADMIN_KEY_REFUSED.into()).ok().flatten().is_some_and(|v| !v.is_empty())
    }

    pub(super) fn mark_admin_key_refused(&self) {
        let _ = self.secrets.set(ADMIN_KEY_REFUSED.into(), "1".into());
    }

    /// The stored admin key, if the account has one for this server and this account. A key
    /// that was accepted for another server or another account (the app signed in again as
    /// someone else) is deleted and not used.
    pub(super) fn admin_key(&self) -> Result<Option<String>> {
        let key = self.secrets.get(ADMIN_KEY.into()).map_err(|_| key_store_error())?;
        let Some(key) = key.filter(|k| !k.is_empty()) else { return Ok(None) };
        let binding = self.admin_binding()?;
        let stored = self.secrets.get(ADMIN_KEY_FOR.into()).map_err(|_| key_store_error())?;
        if stored.as_deref() != Some(binding.as_str()) {
            self.drop_admin_key()?;
            return Ok(None);
        }
        Ok(Some(key))
    }

    /// Runs `f` on a connection made with `key`. A key the server no longer takes is
    /// `Error::Invalid` saying so, so the host can offer to paste a new one.
    pub(super) fn admin_with_key<T, F, Fut>(&self, key: &str, f: F) -> Result<T>
    where
        F: FnOnce(Connection) -> Fut,
        Fut: Future<Output = Result<T>>,
    {
        if !is_plausible_key(key) {
            return Err(Error::invalid("this is not an API key"));
        }
        let server_url = {
            let inner = self.lock();
            inner.session.as_ref().ok_or_else(not_signed_in)?.account.server_url.clone()
        };
        self.rt.block_on(async {
            match Connection::connect(&server_url, key).await {
                Ok(conn) => f(conn).await,
                Err(e) if crate::jmap::is_unauthorized(&e) => Err(key_refused()),
                Err(e) if crate::jmap::is_forbidden(&e) => Err(key_blocked()),
                Err(e) => Err(e),
            }
        })
    }

    /// Like [`Core::admin`], but the whole call, the connection included, ends with `Ok(None)` as
    /// soon as `stop` is notified (a stop given earlier counts), so a stop is not kept waiting
    /// for a slow network and the key is not sent after it.
    pub(super) fn admin_until<T, F, Fut>(&self, stop: &tokio::sync::Notify, f: F) -> Result<Option<T>>
    where
        F: FnOnce(Connection) -> Fut,
        Fut: Future<Output = Result<T>>,
    {
        let key = self.admin_key()?.ok_or_else(|| Error::invalid("this account has no admin key yet"))?;
        if self.admin_key_refused_before() {
            return Err(key_refused());
        }
        if !is_plausible_key(&key) {
            return Err(Error::invalid("this is not an API key"));
        }
        let server_url = {
            let inner = self.lock();
            inner.session.as_ref().ok_or_else(not_signed_in)?.account.server_url.clone()
        };
        let result = self.rt.block_on(async {
            tokio::select! {
                biased;
                _ = stop.notified() => Ok(None),
                done = async {
                    match Connection::connect(&server_url, &key).await {
                        Ok(conn) => f(conn).await.map(Some),
                        Err(e) if crate::jmap::is_unauthorized(&e) => Err(key_refused()),
                        Err(e) if crate::jmap::is_forbidden(&e) => Err(key_blocked()),
                        Err(e) => Err(e),
                    }
                } => done,
            }
        });
        if let Err(e) = &result {
            if *e == key_refused() || crate::jmap::is_unauthorized(e) {
                self.mark_admin_key_refused();
            }
        }
        result
    }

    /// Runs `f` with the account's stored admin key. No key is `Error::Invalid`.
    pub(super) fn admin<T, F, Fut>(&self, f: F) -> Result<T>
    where
        F: FnOnce(Connection) -> Fut,
        Fut: Future<Output = Result<T>>,
    {
        let key = self.admin_key()?.ok_or_else(|| Error::invalid("this account has no admin key yet"))?;
        if self.admin_key_refused_before() {
            return Err(key_refused());
        }
        let result = self.admin_with_key(&key, f);
        if let Err(e) = &result {
            if *e == key_refused() || crate::jmap::is_unauthorized(e) {
                self.mark_admin_key_refused();
            }
        }
        result
    }
}

pub(super) fn key_refused() -> Error {
    Error::invalid("the server did not accept the admin key; it may have run out or been deleted")
}

/// The server answered 403 to the key as a whole: it is limited to other addresses, or its
/// account may not sign in (for example it was suspended).
pub(super) fn key_blocked() -> Error {
    Error::invalid("the server does not let this key in from here, or its account is switched off")
}

fn key_store_error() -> Error {
    Error::temporary("the secret store could not be used just now")
}
