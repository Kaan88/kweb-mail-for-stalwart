//! Where the refresh token lives. The core never picks the place: Android gives it the
//! Keystore, the desktop `cli` a file, tests a map.

use std::{collections::HashMap, sync::Mutex};

/// The one secret the core keeps: the OAuth refresh token.
pub const REFRESH_TOKEN: &str = "refresh_token";

/// Implemented by the host. Errors are plain text; a failed `get` means "could not read just
/// now", not "there is nothing".
#[uniffi::export(with_foreign)]
pub trait SecretStore: Send + Sync {
    fn get(&self, key: String) -> Result<Option<String>, SecretError>;
    fn set(&self, key: String, value: String) -> Result<(), SecretError>;
    fn delete(&self, key: String) -> Result<(), SecretError>;
}

#[derive(Debug, Clone, PartialEq, thiserror::Error, uniffi::Error)]
pub enum SecretError {
    #[error("{msg}")]
    Failed { msg: String },
}

impl From<uniffi::UnexpectedUniFFICallbackError> for SecretError {
    fn from(e: uniffi::UnexpectedUniFFICallbackError) -> Self {
        SecretError::Failed { msg: e.reason }
    }
}

/// In memory only. For tests, and for hosts with nowhere safe to keep a secret.
#[derive(Default)]
pub struct MemorySecrets(Mutex<HashMap<String, String>>);

impl SecretStore for MemorySecrets {
    fn get(&self, key: String) -> Result<Option<String>, SecretError> {
        Ok(self.0.lock().unwrap().get(&key).cloned())
    }
    fn set(&self, key: String, value: String) -> Result<(), SecretError> {
        self.0.lock().unwrap().insert(key, value);
        Ok(())
    }
    fn delete(&self, key: String) -> Result<(), SecretError> {
        self.0.lock().unwrap().remove(&key);
        Ok(())
    }
}
