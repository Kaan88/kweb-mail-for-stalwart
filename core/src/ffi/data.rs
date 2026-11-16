//! What the account keeps on the phone: how far back (the sync window), clearing the cache and
//! fetching it again, and the name the user gave the account.

use super::Core;
use crate::{
    actions,
    jmap::is_unauthorized,
    sync::{self, window::SyncWindow},
    Error, Result,
};

/// The key (in the account's key/value table) of the name the user gave the account.
const NAME_KEY: &str = "account_name";

/// The longest an account name is (in characters).
pub const NAME_MAX: usize = 40;

/// The name as it is stored: trimmed, on one line, cut at [`NAME_MAX`]; `None` when nothing is left.
fn clean_name(name: &str) -> Option<String> {
    let one_line = name.split_whitespace().collect::<Vec<_>>().join(" ");
    let cut: String = one_line.chars().take(NAME_MAX).collect();
    let cut = cut.trim().to_string();
    (!cut.is_empty()).then_some(cut)
}

#[uniffi::export]
impl Core {
    /// How much mail each folder keeps on the phone.
    pub fn sync_window(&self) -> Result<SyncWindow> {
        sync::window::get(&self.store, &self.account_id()?)
    }

    /// Changes how much mail is kept. A bigger window fetches the older mail on the next pass
    /// (this call asks for one); a smaller one removes, at once, the cached mail beyond it that
    /// is read, not starred, not in Drafts and not named by a queued action or a draft.
    pub fn set_sync_window(&self, window: SyncWindow) -> Result<()> {
        let account_id = self.account_id()?;
        let store = &self.store;
        let grew = sync::window::set(store, &account_id, window)?;
        if !grew {
            self.rt.block_on(async {
                let _one_at_a_time = store.pass_lock().lock().await;
                let keep = store.referenced_message_ids(&account_id)?;
                sync::window::prune(store, &account_id, window, &keep)
            })?;
        }
        if grew {
            self.poke();
        }
        Ok(())
    }

    /// Drops the account's cached mail, bodies and windows and fetches the mail again, all of it
    /// from the server (network). Nothing of another account, of the secrets, of the settings or
    /// of the notification bookkeeping is touched, so mail that was already announced or swiped is
    /// not announced again. Queued actions are sent first; the call is refused, with the reason,
    /// when something is still waiting afterwards: a message to send, or an action that could not
    /// be sent. The cache is replaced only once everything was fetched, so a failure leaves it as
    /// it was.
    pub fn clear_cache_and_resync(&self) -> Result<()> {
        let account_id = self.account_id()?;
        let store = &self.store;
        if !store.pending_sends(&account_id)?.is_empty() {
            return Err(Error::invalid("A message is still waiting to be sent. Try again when it has gone out."));
        }
        let account = &account_id;
        self.with_connection(|conn| async move {
            // No pass runs while the cache is being replaced, so no notification plan is made from
            // a half-filled cache.
            let _one_at_a_time = store.pass_lock().lock().await;
            match actions::flush(&conn, store, account).await {
                Ok(_) => {}
                Err(e @ Error::SignInNeeded { .. }) => return Err(e),
                Err(e) if is_unauthorized(&e) => return Err(e),
                Err(_) => {}
            }
            let waiting = store.outbox_jobs(account)?.len();
            if waiting > 0 {
                return Err(Error::invalid(format!(
                    "{waiting} change{} made on this phone could not be sent yet. Try again when the connection is back.",
                    if waiting == 1 { "" } else { "s" }
                )));
            }
            sync::resync_messages(&conn, store, account).await?;
            store.delete_bodies(account)?;
            actions::reapply_pending(store, account)?;
            Ok(())
        })?;
        self.poke();
        Ok(())
    }

    /// The name the user gave the account ("Work"), if any.
    pub fn account_name(&self) -> Result<Option<String>> {
        self.store.kv_get(&self.account_id()?, NAME_KEY)
    }

    /// Gives the account a name; blank removes it. Kept on the phone only.
    pub fn set_account_name(&self, name: String) -> Result<()> {
        let account_id = self.account_id()?;
        match clean_name(&name) {
            Some(name) => self.store.kv_set(&account_id, NAME_KEY, &name),
            None => self.store.kv_delete(&account_id, NAME_KEY),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_is_trimmed_put_on_one_line_and_cut() {
        assert_eq!(clean_name("  Work \n mail "), Some("Work mail".into()));
        assert_eq!(clean_name("   "), None);
        assert_eq!(clean_name(&"x".repeat(100)).unwrap().chars().count(), NAME_MAX);
    }
}
