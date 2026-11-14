//! Turns jmap-client errors into the core's `Error`, keeping two failures recognisable:
//! an expired access token (HTTP 401) and a `/changes` call the server cannot answer.
//!
//! Both are encoded as `Error::Temporary` with a message prefix, so the rest of the core
//! keeps one error type. Callers ask `is_unauthorized` / `is_cannot_calculate_changes`.

use jmap_client::{
    core::error::{MethodErrorType, ProblemDetails},
    core::set::SetErrorType,
    Error as JmapError,
};

use crate::Error;

const UNAUTHORIZED_PREFIX: &str = "unauthorized: ";
const FORBIDDEN_PREFIX: &str = "forbidden: ";
const CANNOT_CALCULATE_PREFIX: &str = "cannot calculate changes: ";
const STATE_MISMATCH_PREFIX: &str = "state mismatch: ";
const NO_ACCOUNT_PREFIX: &str = "no such account: ";

/// A method the server answered with `accountNotFound`, `accountNotSupportedByMethod` or
/// `accountReadOnly`: the account named in the request is not there for this user (any more),
/// or does not hold this kind of data. Only calls into another user's account can see this.
pub(super) fn no_account(detail: impl std::fmt::Display) -> Error {
    Error::temporary(format!("{NO_ACCOUNT_PREFIX}{detail}"))
}

pub(super) fn is_no_account(e: &Error) -> bool {
    matches!(e, Error::Temporary { msg } if msg.starts_with(NO_ACCOUNT_PREFIX))
}

/// A `/set` the server refused because `ifInState` no longer matched: the object was changed
/// meanwhile. The caller reads it again and rebuilds the change.
pub(super) fn is_state_mismatch(e: &Error) -> bool {
    matches!(e, Error::Temporary { msg } if msg.starts_with(STATE_MISMATCH_PREFIX))
}

pub(super) fn state_mismatch(detail: impl std::fmt::Display) -> Error {
    Error::temporary(format!("{STATE_MISMATCH_PREFIX}{detail}"))
}

/// The server refused the access token (HTTP 401). The caller renews the token and retries;
/// this is not a reason to ask the user to sign in again.
pub fn is_unauthorized(e: &Error) -> bool {
    matches!(e, Error::Temporary { msg } if msg.starts_with(UNAUTHORIZED_PREFIX))
}

/// The server answered a request as a whole with HTTP 403 (a key limited to other addresses,
/// an account that may not sign in). Still `Error::Temporary` for every caller that does not
/// ask; the admin code turns it into a refusal of the key.
pub fn is_forbidden(e: &Error) -> bool {
    matches!(e, Error::Temporary { msg } if msg.starts_with(FORBIDDEN_PREFIX))
}

/// A 403 seen by code that does not go through jmap-client.
pub(super) fn forbidden(detail: impl std::fmt::Display) -> Error {
    Error::temporary(format!("{FORBIDDEN_PREFIX}{detail}"))
}

/// A `/changes` call the server answered with `cannotCalculateChanges`: the caller must
/// fall back to a full resync.
pub fn is_cannot_calculate_changes(e: &Error) -> bool {
    matches!(e, Error::Temporary { msg } if msg.starts_with(CANNOT_CALCULATE_PREFIX))
}

/// A 401 seen by code that does not go through jmap-client (the plain calendar request).
pub(super) fn unauthorized(detail: impl std::fmt::Display) -> Error {
    Error::temporary(format!("{UNAUTHORIZED_PREFIX}{detail}"))
}

#[derive(Debug, PartialEq)]
enum Kind {
    Unauthorized,
    Forbidden,
    CannotCalculateChanges,
    NotFound,
    /// The server refused the change and will refuse it again (a mailbox that is gone, no right
    /// to do it).
    Refused,
    NoAccount,
    Other,
}

/// Set errors that trying again cannot cure. Quota and rate limits can pass, so they are not here.
fn is_permanent(t: &SetErrorType) -> bool {
    use SetErrorType::*;
    matches!(t, Forbidden | InvalidPatch | InvalidProperties | WillDestroy | TooManyKeywords | TooManyMailboxes)
}

fn problem_is_401(p: &ProblemDetails) -> bool {
    p.status() == Some(401)
}

fn kind(e: &JmapError) -> Kind {
    match e {
        JmapError::Problem(p) if problem_is_401(p) => Kind::Unauthorized,
        // handle_error() formats the bare status for non-JSON error bodies: "401 Unauthorized".
        JmapError::Server(s) if s.split_whitespace().next() == Some("401") => Kind::Unauthorized,
        JmapError::Transport(t) if t.status().is_some_and(|s| s.as_u16() == 401) => Kind::Unauthorized,
        JmapError::Problem(p) if p.status() == Some(403) => Kind::Forbidden,
        JmapError::Server(s) if s.split_whitespace().next() == Some("403") => Kind::Forbidden,
        JmapError::Transport(t) if t.status().is_some_and(|s| s.as_u16() == 403) => Kind::Forbidden,
        JmapError::Method(m) if *m.error() == MethodErrorType::CannotCalculateChanges => Kind::CannotCalculateChanges,
        JmapError::Method(m)
            if matches!(m.error(), MethodErrorType::AccountNotFound | MethodErrorType::AccountNotSupportedByMethod) =>
        {
            Kind::NoAccount
        }
        JmapError::Set(s) if *s.error() == SetErrorType::NotFound => Kind::NotFound,
        JmapError::Set(s) if is_permanent(s.error()) => Kind::Refused,
        _ => Kind::Other,
    }
}

pub(super) fn from_jmap(e: JmapError) -> Error {
    match kind(&e) {
        Kind::Unauthorized => Error::temporary(format!("{UNAUTHORIZED_PREFIX}{e}")),
        Kind::Forbidden => Error::temporary(format!("{FORBIDDEN_PREFIX}{e}")),
        Kind::CannotCalculateChanges => Error::temporary(format!("{CANNOT_CALCULATE_PREFIX}{e}")),
        Kind::NoAccount => no_account(e),
        Kind::NotFound => Error::invalid(format!("not found on the server: {e}")),
        Kind::Refused => Error::invalid(format!("refused by the server: {e}")),
        Kind::Other => Error::temporary(e.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn problem(json: &str) -> JmapError {
        JmapError::Problem(Box::new(serde_json::from_str(json).unwrap()))
    }

    #[test]
    fn recognises_401_in_both_shapes() {
        let bare = JmapError::Server("401 Unauthorized".into());
        assert!(is_unauthorized(&from_jmap(bare)));
        let json = problem(r#"{"type":"about:blank","status":401,"title":"Unauthorized"}"#);
        assert!(is_unauthorized(&from_jmap(json)));
    }

    #[test]
    fn other_statuses_are_plain_temporary() {
        let e = from_jmap(JmapError::Server("503 Service Unavailable".into()));
        assert!(matches!(e, Error::Temporary { .. }));
        assert!(!is_unauthorized(&e));
        assert!(!is_cannot_calculate_changes(&e));
        let e = from_jmap(problem(r#"{"type":"about:blank","status":403}"#));
        assert!(!is_unauthorized(&e));
        assert!(is_forbidden(&e));
    }

    #[test]
    fn recognises_cannot_calculate_changes() {
        let m: jmap_client::core::error::MethodError =
            serde_json::from_str(r#"{"type":"cannotCalculateChanges"}"#).unwrap();
        let e = from_jmap(JmapError::Method(m));
        assert!(is_cannot_calculate_changes(&e));
        assert!(!is_unauthorized(&e));
    }

    #[test]
    fn other_method_errors_are_not_special() {
        let m: jmap_client::core::error::MethodError = serde_json::from_str(r#"{"type":"serverFail"}"#).unwrap();
        let e = from_jmap(JmapError::Method(m));
        assert!(!is_cannot_calculate_changes(&e));
    }

    #[test]
    fn a_change_the_server_will_never_take_is_invalid_and_a_passing_one_is_not() {
        let set = |json: &str| {
            JmapError::Set(serde_json::from_str::<jmap_client::core::set::SetError<String>>(json).unwrap())
        };
        for kind in ["invalidProperties", "forbidden", "invalidPatch", "tooManyMailboxes"] {
            let e = from_jmap(set(&format!(r#"{{"type":"{kind}"}}"#)));
            assert!(matches!(e, Error::Invalid { .. }), "{kind}");
        }
        for kind in ["overQuota", "rateLimit"] {
            let e = from_jmap(set(&format!(r#"{{"type":"{kind}"}}"#)));
            assert!(matches!(e, Error::Temporary { .. }), "{kind}");
        }
    }

    #[test]
    fn set_not_found_is_invalid() {
        let s: jmap_client::core::set::SetError<String> = serde_json::from_str(r#"{"type":"notFound"}"#).unwrap();
        assert!(matches!(from_jmap(JmapError::Set(s)), Error::Invalid { .. }));
    }
}
