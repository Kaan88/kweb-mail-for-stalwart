//! The server's notices for the signed-in account: whether to show them, and reading them by hand
//! (the cli, the Delivery check). The pass that shows them is `notices::deliver`.

use super::Core;
use crate::{model::Notice, notices, Result};

#[uniffi::export]
impl Core {
    /// Whether the notices of the server (invitations, changes, cancellations, shares) are shown for this account.
    /// On unless the user switched them off.
    pub fn notices_enabled(&self) -> Result<bool> {
        notices::enabled(&self.store, &self.account_id()?)
    }

    /// Switches the notices for this account on or off. Off: nothing is read from the server for them.
    pub fn set_notices_enabled(&self, on: bool) -> Result<()> {
        notices::set_enabled(&self.store, &self.account_id()?, on)
    }

    /// What the server holds for the user now, oldest first, without showing or acknowledging anything.
    pub fn server_notices(&self) -> Result<Vec<Notice>> {
        self.with_connection(|conn| async move { conn.notices().await })
    }

    /// Tells the server these notices (by [`Notice::id`]) were seen: they are destroyed there.
    pub fn acknowledge_notices(&self, ids: Vec<String>) -> Result<()> {
        self.with_connection(|conn| {
            let ids = ids.clone();
            async move { conn.acknowledge_notices(&ids).await }
        })
    }
}
