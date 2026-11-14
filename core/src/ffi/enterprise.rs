//! Admin design, step 3: what a licensed (Enterprise) server adds: the dashboard, delivery
//! history, recovery of deleted items, alerts and tenants.
//!
//! As in `admin.rs`: the account's admin key, nothing cached. On a server without a licence
//! each function is `Error::Invalid` ("this needs an Enterprise licence on the server").

mod alerts;
mod deliveries;
mod history;
mod live;
mod recovery;

use std::{
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use super::{admin_link::key_refused, Core};
use crate::{jmap::is_unauthorized, Error, Result};

/// The largest page of deliveries or deleted items asked of the server at once.
const MAX_PAGE: u32 = 100;

/// A key the server stopped taking in the middle of a call is a key the server refuses.
fn tidy(e: Error) -> Error {
    if is_unauthorized(&e) {
        key_refused()
    } else {
        e
    }
}

fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

/// The live figures of the server. One the server does not send is `None`.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct LiveFigures {
    pub queue: Option<u64>,
    pub connections: Option<u64>,
    pub users: Option<u64>,
    pub memory_bytes: Option<u64>,
}

/// Hears the live figures. Called on a core thread; it must not call a blocking function of
/// [`Core`] from inside.
#[uniffi::export(with_foreign)]
pub trait LiveListener: Send + Sync {
    fn figures(&self, figures: LiveFigures);
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct ChartPoint {
    /// Unix seconds: the start of the hour (or day) the value is for.
    pub at: i64,
    pub value: f64,
}

/// The history charts of the dashboard, oldest first, one point per hour up to 48 hours and
/// one per day beyond.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct DashboardHistory {
    pub received: Vec<ChartPoint>,
    /// What the server sent (messages from its accounts and reports).
    pub delivered: Vec<ChartPoint>,
    /// The average time a delivery took, in seconds.
    pub delivery_seconds: Vec<ChartPoint>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum DeliveryStatus {
    Delivered,
    /// Not delivered yet; the server tries again.
    Retrying,
    Failed,
    /// The events do not say.
    Unknown,
}

/// One message as the server handled it.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct Delivery {
    pub id: String,
    pub from: String,
    pub to: Vec<String>,
    pub at: i64,
    pub size: u64,
    pub status: DeliveryStatus,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct DeliveryPage {
    pub deliveries: Vec<Delivery>,
    pub total: u32,
}

/// One step of a delivery, in plain words.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct DeliveryStep {
    pub at: i64,
    /// What happened, e.g. "Accepted by their server".
    pub title: String,
    /// The useful detail, e.g. the host and its answer; may be empty.
    pub detail: String,
    /// The server's own event name and values, for the "show the server's own lines" view.
    pub raw: String,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct DeliveryDetail {
    pub delivery: Delivery,
    pub steps: Vec<DeliveryStep>,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct RecoveryState {
    /// The server keeps deleted items for a while.
    pub is_on: bool,
    /// For how many days; `None` when off or not said.
    pub keeps_days: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum DeletedKind {
    Mail,
    Event,
    Contact,
    File,
    Other,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct DeletedItem {
    pub id: String,
    pub kind: DeletedKind,
    /// The subject, title or name; may be empty.
    pub title: String,
    pub deleted_at: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct DeletedPage {
    pub items: Vec<DeletedItem>,
    pub total: u32,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct ServerAlert {
    pub id: String,
    pub name: String,
    pub is_enabled: bool,
    /// When it fires and how it tells, in plain words made by the core.
    pub summary: String,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct Tenant {
    pub id: String,
    pub name: String,
    /// The storage limit in bytes; `None` for no limit.
    pub quota: Option<u64>,
    pub used: Option<u64>,
}

#[uniffi::export]
impl Core {
    /// Follows the server's live figures: `listener` hears them about every five seconds
    /// until [`Core::admin_live_stop`] is called with the same `run_id` (a name the host makes
    /// up for this run, as for the assistant) or the connection ends. Blocks meanwhile and
    /// returns `Ok` when stopped; a connection that ends by itself is `Error::Temporary`.
    pub fn admin_live(&self, run_id: String, listener: Arc<dyn LiveListener>) -> Result<()> {
        // A stop that came before this call is a stop: there is nothing to follow.
        let Some(run) = live::begin(&run_id)? else { return Ok(()) };
        // The stop is heard from the start, while connecting and checking the licence too.
        let cancel = run.cancel.clone();
        self.admin_until(&cancel, |conn| async move {
            conn.require_enterprise(&["liveMetrics"], "follow the live figures").await?;
            live::follow(&conn, listener.as_ref()).await
        })
        .map(|_| ())
        .map_err(tidy)
    }

    /// Stops a run of [`Core::admin_live`], also one that has not started yet. Does not block.
    pub fn admin_live_stop(&self, run_id: String) {
        live::cancel(&run_id);
    }

    /// The dashboard's history for the last `hours` (24, 168 or 720; anything else is
    /// `Error::Invalid`).
    pub fn admin_history(&self, hours: u32) -> Result<DashboardHistory> {
        let now = now();
        let span = history::span(hours, now)?;
        self.admin(|conn| async move {
            conn.require_enterprise(&["sysMetricQuery", "sysMetricGet"], "read the server's history").await?;
            let mut names = vec![history::RECEIVED, history::DELIVERY_TIME];
            names.extend(history::SENT);
            let rows = conn.metric_rows(span.start, now, &names).await?;
            Ok(history::history(&rows, span))
        })
        .map_err(tidy)
    }

    /// Messages the server handled that have `text` (blank: all), since `since` (Unix seconds;
    /// `None`: as far back as the server keeps, a year at most), newest first. `text` is a whole
    /// address, domain or host name as the server indexes them (a part of an address finds
    /// nothing), or a queue id (digits only).
    pub fn admin_deliveries(&self, text: String, since: Option<i64>, offset: u32, limit: u32) -> Result<DeliveryPage> {
        let limit = limit.clamp(1, MAX_PAGE) as usize;
        self.admin(|conn| async move {
            conn.require_enterprise(&["sysTraceQuery", "sysTraceGet"], "read the delivery history").await?;
            let (traces, total) = conn.delivery_traces(&text, since, offset as usize, limit).await?;
            let deliveries = traces.iter().filter_map(deliveries::delivery_from).collect();
            Ok(DeliveryPage { deliveries, total: total.min(u64::from(u32::MAX)) as u32 })
        })
        .map_err(tidy)
    }

    pub fn admin_delivery(&self, id: String) -> Result<DeliveryDetail> {
        self.admin(|conn| async move {
            conn.require_enterprise(&["sysTraceGet"], "read the delivery history").await?;
            let trace = conn.delivery_trace(&id).await?;
            trace
                .as_ref()
                .and_then(deliveries::detail_from)
                .ok_or_else(|| Error::invalid("the server no longer keeps that delivery"))
        })
        .map_err(tidy)
    }

    pub fn admin_recovery_state(&self) -> Result<RecoveryState> {
        self.admin(|conn| async move {
            let held = conn.require_enterprise(&[], "read the deleted-item settings").await?;
            // A key that may list deleted items but not read the setting still gets the list:
            // the state is then "on, for how long not said".
            if !held.is_empty() && !held.contains("sysDataRetentionGet") {
                return Ok(RecoveryState { is_on: true, keeps_days: None });
            }
            Ok(recovery::state_from(&conn.retention_setting().await?))
        })
        .map_err(tidy)
    }

    /// Deleted items the server still keeps, newest first. `account_id`: `None` is the account
    /// the admin key belongs to, otherwise another account's id from [`Core::admin_accounts`].
    pub fn admin_deleted_items(&self, account_id: Option<String>, offset: u32, limit: u32) -> Result<DeletedPage> {
        let limit = limit.clamp(1, MAX_PAGE) as usize;
        self.admin(|conn| async move {
            let account = deleted_account(&conn, account_id).await?;
            let (items, total) = conn.archived_items(&account, offset as usize, limit).await?;
            let items = items.iter().filter_map(recovery::item_from).collect();
            Ok(DeletedPage { items, total: total.min(u64::from(u32::MAX)) as u32 })
        })
        .map_err(tidy)
    }

    /// Asks the server to put deleted items back where they were.
    pub fn admin_restore(&self, account_id: Option<String>, ids: Vec<String>) -> Result<()> {
        self.admin(|conn| async move {
            deleted_account(&conn, account_id).await?;
            conn.require_enterprise(&["sysArchivedItemUpdate"], "restore deleted items").await?;
            if ids.is_empty() {
                return Ok(());
            }
            conn.archived_restore(&ids).await
        })
        .map_err(tidy)
    }

    pub fn admin_alerts(&self) -> Result<Vec<ServerAlert>> {
        self.admin(|conn| async move {
            conn.require_enterprise(&["sysAlertGet"], "read the alerts").await?;
            Ok(conn.alert_objects().await?.iter().filter_map(alerts::alert_from).collect())
        })
        .map_err(tidy)
    }

    pub fn admin_set_alert_enabled(&self, id: String, enabled: bool) -> Result<()> {
        self.admin(|conn| async move {
            conn.require_enterprise(&["sysAlertUpdate"], "change alerts").await?;
            conn.alert_enable(&id, enabled).await
        })
        .map_err(tidy)
    }

    pub fn admin_tenants(&self) -> Result<Vec<Tenant>> {
        self.admin(|conn| async move {
            conn.require_enterprise(&["sysTenantGet"], "read the tenants").await?;
            Ok(conn.tenant_objects().await?.iter().filter_map(alerts::tenant_from).collect())
        })
        .map_err(tidy)
    }
}

/// The account a deleted-items call is about, after the licence and the key's rights: the key's
/// own account when `account_id` is `None` or names it, any other one only with `impersonate`.
async fn deleted_account(conn: &crate::jmap::Connection, account_id: Option<String>) -> Result<String> {
    let own = conn.account_id();
    let wanted = account_id.filter(|a| !a.is_empty() && *a != own);
    let held =
        conn.require_enterprise(&["sysArchivedItemQuery", "sysArchivedItemGet"], "look at deleted items").await?;
    match wanted {
        None => Ok(own),
        Some(other) if held.is_empty() || held.contains("impersonate") => Ok(other),
        Some(_) => Err(Error::invalid("this key may not look at another account's deleted items")),
    }
}
