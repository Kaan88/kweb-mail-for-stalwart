//! One error type for the whole core. The variants are what a caller has to tell apart.

#[derive(Debug, Clone, PartialEq, thiserror::Error, uniffi::Error)]
pub enum Error {
    /// Not signed in, or the server no longer accepts the grant: the user has to sign in again.
    #[error("sign-in needed: {msg}")]
    SignInNeeded { msg: String },
    /// Server unreachable or answered oddly: trying again later may work.
    #[error("{msg}")]
    Temporary { msg: String },
    /// The local cache could not be read or written.
    #[error("local store: {msg}")]
    Store { msg: String },
    /// The caller passed something that can never work (bad URL, unknown id).
    #[error("{msg}")]
    Invalid { msg: String },
}

pub type Result<T> = std::result::Result<T, Error>;

impl Error {
    pub fn sign_in_needed(msg: impl Into<String>) -> Self {
        Error::SignInNeeded { msg: msg.into() }
    }
    pub fn temporary(msg: impl Into<String>) -> Self {
        Error::Temporary { msg: msg.into() }
    }
    pub fn store(msg: impl Into<String>) -> Self {
        Error::Store { msg: msg.into() }
    }
    pub fn invalid(msg: impl Into<String>) -> Self {
        Error::Invalid { msg: msg.into() }
    }
}

impl From<rusqlite::Error> for Error {
    fn from(e: rusqlite::Error) -> Self {
        Error::store(e.to_string())
    }
}
