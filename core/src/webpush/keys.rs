//! The key pair and the auth secret of an account's subscription, in the secret store and
//! nowhere else. Either both are there or neither.

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use p256::SecretKey;

use super::ece::public_point;
use crate::{
    secrets::{SecretError, SecretStore},
    Error, Result,
};

pub const PRIVATE_KEY: &str = "web_push_private_key";
pub const AUTH: &str = "web_push_auth";

/// The subscription's keys. `Debug` says nothing of them.
pub struct Keys {
    secret: SecretKey,
    auth: [u8; 16],
}

impl std::fmt::Debug for Keys {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Keys { .. }")
    }
}

impl Keys {
    pub fn secret(&self) -> &SecretKey {
        &self.secret
    }

    pub fn auth(&self) -> &[u8] {
        &self.auth
    }

    /// The `p256dh` the server wants: the 65-byte public point, base64url without padding.
    pub fn p256dh(&self) -> String {
        URL_SAFE_NO_PAD.encode(public_point(&self.secret))
    }

    /// The `auth` the server wants.
    pub fn auth_text(&self) -> String {
        URL_SAFE_NO_PAD.encode(self.auth)
    }
}

fn unreadable(_: SecretError) -> Error {
    Error::store("the keys for push could not be read")
}

fn unkept(_: SecretError) -> Error {
    Error::store("the keys for push could not be kept")
}

fn random(buf: &mut [u8]) -> Result<()> {
    getrandom::fill(buf).map_err(|_| Error::temporary("random numbers are not available"))
}

/// A random version 4 UUID in text form.
pub fn uuid_v4() -> Result<String> {
    let mut b = [0u8; 16];
    random(&mut b)?;
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    let hex: String = b.iter().map(|x| format!("{x:02x}")).collect();
    Ok(format!("{}-{}-{}-{}-{}", &hex[..8], &hex[8..12], &hex[12..16], &hex[16..20], &hex[20..]))
}

/// The keys of this account. `None` when there are none, or when what is stored is not a pair
/// that can be used (it is then made again by [`create`]).
pub fn load(secrets: &dyn SecretStore) -> Result<Option<Keys>> {
    let private = secrets.get(PRIVATE_KEY.into()).map_err(unreadable)?;
    let auth = secrets.get(AUTH.into()).map_err(unreadable)?;
    let (Some(private), Some(auth)) = (private, auth) else { return Ok(None) };
    let (Ok(private), Ok(auth)) = (URL_SAFE_NO_PAD.decode(private), URL_SAFE_NO_PAD.decode(auth)) else {
        return Ok(None);
    };
    let (Ok(secret), Ok(auth)) = (SecretKey::from_slice(&private), <[u8; 16]>::try_from(auth.as_slice())) else {
        return Ok(None);
    };
    Ok(Some(Keys { secret, auth }))
}

/// Makes a key pair and an auth secret and keeps them. If the second cannot be kept the first
/// is taken away again.
pub fn create(secrets: &dyn SecretStore) -> Result<Keys> {
    let secret = loop {
        let mut bytes = [0u8; 32];
        random(&mut bytes)?;
        if let Ok(key) = SecretKey::from_slice(&bytes) {
            break key;
        }
    };
    let mut auth = [0u8; 16];
    random(&mut auth)?;
    secrets.set(PRIVATE_KEY.into(), URL_SAFE_NO_PAD.encode(secret.to_bytes())).map_err(unkept)?;
    if let Err(e) = secrets.set(AUTH.into(), URL_SAFE_NO_PAD.encode(auth)) {
        let _ = secrets.delete(PRIVATE_KEY.into());
        return Err(unkept(e));
    }
    Ok(Keys { secret, auth })
}

/// Deletes both, whatever happens to the first; the first error is returned.
pub fn forget(secrets: &dyn SecretStore) -> Result<()> {
    let first = secrets.delete(PRIVATE_KEY.into());
    let second = secrets.delete(AUTH.into());
    first.and(second).map_err(|_| Error::store("the keys for push could not be deleted"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secrets::MemorySecrets;

    #[test]
    fn keys_are_made_once_and_loaded_after() {
        let store = MemorySecrets::default();
        assert!(load(&store).unwrap().is_none());
        let made = create(&store).unwrap();
        let loaded = load(&store).unwrap().unwrap();
        assert_eq!(made.p256dh(), loaded.p256dh());
        assert_eq!(made.auth_text(), loaded.auth_text());
        assert_eq!(URL_SAFE_NO_PAD.decode(loaded.p256dh()).unwrap().len(), 65);
        assert_eq!(URL_SAFE_NO_PAD.decode(loaded.auth_text()).unwrap().len(), 16);
        forget(&store).unwrap();
        assert!(load(&store).unwrap().is_none());
    }

    #[test]
    fn half_a_pair_is_no_pair() {
        let store = MemorySecrets::default();
        create(&store).unwrap();
        store.delete(AUTH.into()).unwrap();
        assert!(load(&store).unwrap().is_none());
        store.set(AUTH.into(), "not base64 !".into()).unwrap();
        assert!(load(&store).unwrap().is_none());
    }

    #[test]
    fn a_uuid_looks_like_one() {
        let id = uuid_v4().unwrap();
        assert_eq!(id.len(), 36);
        assert_eq!(&id[14..15], "4");
        assert_ne!(id, uuid_v4().unwrap());
    }

    #[test]
    fn debug_shows_nothing() {
        let store = MemorySecrets::default();
        let keys = create(&store).unwrap();
        assert_eq!(format!("{keys:?}"), "Keys { .. }");
    }
}
