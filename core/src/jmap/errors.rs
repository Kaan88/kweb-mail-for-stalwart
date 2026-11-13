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
const CANNOT_CALCULATE_PREFIX: &str = "cannot calculate changes: ";

/// The server refused the access token (HTTP 401). The caller renews the token and retries;
/// this is not a reason to ask the user to sign in again.
pub fn is_unauthorized(e: &Error) -> bool {
    matches!(e, Error::Temporary { msg } if msg.starts_with(UNAUTHORIZED_PREFIX))
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
    CannotCalculateChanges,
    NotFound,
    Other,
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
        JmapError::Method(m) if *m.error() == MethodErrorType::CannotCalculateChanges => Kind::CannotCalculateChanges,
        JmapError::Set(s) if *s.error() == SetErrorType::NotFound => Kind::NotFound,
        _ => Kind::Other,
    }
}

pub(super) fn from_jmap(e: JmapError) -> Error {
    match kind(&e) {
        Kind::Unauthorized => Error::temporary(format!("{UNAUTHORIZED_PREFIX}{e}")),
        Kind::CannotCalculateChanges => Error::temporary(format!("{CANNOT_CALCULATE_PREFIX}{e}")),
        Kind::NotFound => Error::invalid(format!("not found on the server: {e}")),
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
    fn set_not_found_is_invalid() {
        let s: jmap_client::core::set::SetError<String> = serde_json::from_str(r#"{"type":"notFound"}"#).unwrap();
        assert!(matches!(from_jmap(JmapError::Set(s)), Error::Invalid { .. }));
    }
}
