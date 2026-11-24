//! Admin areas added after the first ones, all in the open edition: the domains with their DNS
//! records and DKIM keys, the reports other servers sent about this one, the server's background
//! tasks, and the cluster's nodes.
//!
//! As in `admin.rs`: the account's admin key, nothing cached, and what the key may not do is
//! `Error::Invalid` with words for the user. A destructive call is guarded here, not only by the
//! host's dialog: a domain that accounts still use is not deleted, the task that finishes an
//! account's deletion is not deleted (see `core/src/jmap/admin/`).

use super::Core;
use crate::Result;

/// A domain in the list.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct DomainRow {
    pub id: String,
    pub name: String,
    /// Mail for the domain is taken (the server's `enable`).
    pub enabled: bool,
    /// Other names that lead to this domain.
    pub aliases: Vec<String>,
}

/// One DKIM key of a domain, read only.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct DkimKey {
    pub id: String,
    pub selector: String,
    /// The algorithm in words ("Ed25519 (SHA-256)").
    pub algorithm: String,
    /// "active", "pending", "retiring" or "retired".
    pub stage: String,
    /// When the key moves to its next stage (unix seconds), if the server plans it.
    pub next_change_at: Option<i64>,
    pub created_at: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct DomainDetail {
    pub id: String,
    pub name: String,
    pub enabled: bool,
    pub aliases: Vec<String>,
    /// The DNS records to publish for the domain, one per line, as the server made them.
    pub dns_zone: String,
    /// The server makes and rotates the keys by itself (otherwise they are kept by hand).
    pub dkim_automatic: bool,
    /// The key may read the DKIM keys; when false `dkim` is empty because of that.
    pub dkim_allowed: bool,
    pub dkim: Vec<DkimKey>,
}

/// The kinds of report other servers send.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum ReportKind {
    /// DMARC aggregate reports: how mail claiming to be from the domain fared at the sender.
    Dmarc,
    /// TLS reports: how secure connections to this server went.
    Tls,
    /// Abuse and failure reports about single messages.
    Arf,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct ReportRow {
    pub id: String,
    pub kind: ReportKind,
    /// The address the report came from.
    pub from: String,
    pub subject: String,
    /// Who made the report (a provider's name or its host); may be empty.
    pub organization: String,
    /// The domain the report is about; may be empty.
    pub domain: String,
    pub received_at: Option<i64>,
    /// DMARC: messages that passed; TLS: sessions that worked; ARF: none.
    pub passed: Option<u32>,
    /// DMARC: messages that failed; TLS: sessions that failed; ARF: the incidents.
    pub failed: Option<u32>,
    /// ARF: the kind of feedback in words; empty for the others.
    pub note: String,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct ReportPage {
    pub reports: Vec<ReportRow>,
    pub total: u32,
}

/// A labelled line of a report; a value may have several lines.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct ReportFact {
    pub label: String,
    pub value: String,
}

/// A part of a report with a title of its own: a sending source (DMARC) or a policy (TLS).
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct ReportGroup {
    pub title: String,
    pub facts: Vec<ReportFact>,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct ReportDetail {
    pub id: String,
    pub kind: ReportKind,
    pub from: String,
    pub to: Vec<String>,
    pub subject: String,
    pub received_at: Option<i64>,
    /// When the server deletes the report by itself.
    pub expires_at: Option<i64>,
    /// The time the report covers (DMARC and TLS).
    pub period_start: Option<i64>,
    pub period_end: Option<i64>,
    pub summary: Vec<ReportFact>,
    pub groups: Vec<ReportGroup>,
}

/// Where a task stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum TaskState {
    /// Waiting for its time.
    Waiting,
    /// It failed and will be tried again.
    Retrying,
    /// It has failed many times and is still tried.
    Stalled,
    /// It failed for good and needs someone to look at it.
    Failed,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct TaskRow {
    pub id: String,
    /// What the task does, in words.
    pub kind: String,
    /// The server's name of the type ("DkimManagement").
    pub kind_code: String,
    pub due: Option<i64>,
    pub state: TaskState,
    /// How many times it was tried and failed.
    pub attempts: u32,
    /// Why it failed last, shortened; empty when it did not.
    pub reason: String,
    /// What it is about when the core can say (a domain name); may be empty.
    pub subject: String,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct TaskPage {
    pub tasks: Vec<TaskRow>,
    pub total: u32,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct ClusterNode {
    pub id: String,
    pub node_id: u32,
    pub hostname: String,
    /// "active", "stale" or "inactive".
    pub status: String,
    pub last_renewal: Option<i64>,
}

#[uniffi::export]
impl Core {
    /// The server's domains, by name.
    pub fn admin_domain_rows(&self) -> Result<Vec<DomainRow>> {
        self.admin_run(|conn| async move { conn.domain_rows().await })
    }

    /// One domain with the DNS records to publish and its DKIM keys.
    pub fn admin_domain(&self, id: String) -> Result<DomainDetail> {
        self.admin_run(move |conn| async move { conn.domain_detail(&id).await })
    }

    /// Adds a domain by name (checked here; the server makes it lower case). One that exists is `Error::Invalid`.
    pub fn admin_domain_add(&self, name: String) -> Result<DomainRow> {
        self.admin_run(move |conn| async move { conn.domain_add(&name).await })
    }

    /// Switches mail for a domain off or on.
    pub fn admin_domain_set_enabled(&self, id: String, enabled: bool) -> Result<()> {
        self.admin_run(move |conn| async move { conn.domain_enable(&id, enabled).await })
    }

    /// Deletes a domain, with its DKIM keys. A domain an account still uses (its address or an alias)
    /// is `Error::Invalid` saying so, and so is one the key cannot check.
    pub fn admin_domain_delete(&self, id: String) -> Result<()> {
        self.admin_run(move |conn| async move { conn.domain_delete(&id).await })
    }

    /// A page of the received reports of one kind, newest first (`limit` at most 100).
    pub fn admin_reports(&self, kind: ReportKind, offset: u32, limit: u32) -> Result<ReportPage> {
        self.admin_run(move |conn| async move { conn.report_page(kind, offset as usize, limit as usize).await })
    }

    pub fn admin_report(&self, kind: ReportKind, id: String) -> Result<ReportDetail> {
        self.admin_run(move |conn| async move { conn.report_detail(kind, &id).await })
    }

    /// Deletes one received report. One that is gone already is not an error.
    pub fn admin_report_delete(&self, kind: ReportKind, id: String) -> Result<()> {
        self.admin_run(move |conn| async move { conn.report_delete(kind, &id).await })
    }

    /// The background tasks, the one due first at the top; `state` narrows the list (`limit` at most 100).
    pub fn admin_tasks(&self, state: Option<TaskState>, offset: u32, limit: u32) -> Result<TaskPage> {
        self.admin_run(move |conn| async move { conn.task_page(state, offset as usize, limit as usize).await })
    }

    /// Has the server run a task again now. A task being run at that moment is `Error::Invalid` saying so.
    pub fn admin_task_retry(&self, id: String) -> Result<()> {
        self.admin_run(move |conn| async move { conn.task_retry(&id).await })
    }

    /// Deletes a task, for good. The task that finishes the deletion of an account is refused.
    pub fn admin_task_delete(&self, id: String) -> Result<()> {
        self.admin_run(move |conn| async move { conn.task_delete(&id).await })
    }

    /// The cluster's nodes, by number.
    pub fn admin_cluster(&self) -> Result<Vec<ClusterNode>> {
        self.admin_run(|conn| async move { conn.cluster_nodes().await })
    }
}
