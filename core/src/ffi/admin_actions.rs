//! The server's Actions (the web admin's "Actions" page): the simple actions with what they return, the DMARC
//! check and the spam check. The server does the work; the host asks the user first.
//!
//! As in `admin.rs`: the account's admin key, nothing cached, and what the key may not do is `Error::Invalid`
//! with words for the user. A message pasted for a check is sent to the server this account is signed in to
//! and is never kept, logged or put into an error (see `core/src/jmap/admin/actions.rs`).

use super::{Core, Maintenance};
use crate::Result;

/// What a simple action returned.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct ActionOutcome {
    /// Warnings of a settings reload (licence problems and the like); empty for the other actions.
    pub warnings: Vec<String>,
}

/// The result of one authentication check, in plain words.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct AuthResult {
    /// "pass", "fail", "soft fail", "temporary error", "permanent error", "neutral" or "none".
    pub outcome: String,
    /// The check passed.
    pub good: bool,
    /// What the server said about it; empty when nothing.
    pub details: String,
}

/// What the DMARC check is asked about: one SMTP session, as the server would see it.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct DmarcCheck {
    pub remote_ip: String,
    pub ehlo_domain: String,
    pub mail_from: String,
    pub spf_ehlo_domain: String,
    pub spf_mail_from_domain: String,
    /// Recipients, separated by commas or spaces; may be empty.
    pub rcpt_to: String,
    /// The raw message; may be empty.
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct DmarcCheckResult {
    pub spf_ehlo: AuthResult,
    pub spf_mail_from: AuthResult,
    pub dkim_pass: bool,
    pub dmarc: AuthResult,
    pub dmarc_pass: bool,
    /// "none", "quarantine", "reject" or "unspecified".
    pub dmarc_policy: String,
    pub elapsed_ms: u64,
}

/// What the spam check is asked about: a raw message and the SMTP session it came in on.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct SpamCheck {
    pub message: String,
    pub remote_ip: String,
    pub ehlo_domain: String,
    pub mail_from: String,
    /// Recipients, separated by commas or spaces; at least one.
    pub rcpt_to: String,
    pub is_tls: bool,
    /// The login the sender authenticated as; may be empty.
    pub authenticated_as: String,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct SpamTag {
    pub name: String,
    pub score: f64,
    /// "score", "reject" or "discard".
    pub disposition: String,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct SpamCheckResult {
    pub score: f64,
    /// "spam", "ham", "reject" or "discard".
    pub result: String,
    /// What counts most first.
    pub tags: Vec<SpamTag>,
    /// The AI classifier's gate and verdict (Enterprise); empty when the server did not say.
    pub ai_gate: String,
    pub ai_verdict: String,
}

#[uniffi::export]
impl Core {
    /// Starts a simple action and returns what it said (the warnings of a settings reload). The host asks first.
    pub fn admin_action(&self, what: Maintenance) -> Result<ActionOutcome> {
        self.admin_run(move |conn| async move { conn.run_action(what).await })
    }

    /// Asks the server what it makes of one SMTP session for SPF, DKIM and DMARC. Nothing is sent to anyone.
    pub fn admin_dmarc_check(&self, check: DmarcCheck) -> Result<DmarcCheckResult> {
        self.admin_run(move |conn| async move { conn.dmarc_check(&check).await })
    }

    /// Has the server's spam filter judge a pasted message. Nothing is delivered or kept.
    pub fn admin_spam_check(&self, check: SpamCheck) -> Result<SpamCheckResult> {
        self.admin_run(move |conn| async move { conn.spam_check(&check).await })
    }
}
