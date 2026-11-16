//! The reading choices kept on the device: remote pictures (a setting and an allow list of senders).

use super::Store;
use crate::reading::{sender_key, RemotePictures};
use crate::Result;

const MODE_KEY: &str = "remote_pictures";

impl Store {
    pub fn remote_pictures(&self, account_id: &str) -> Result<RemotePictures> {
        Ok(RemotePictures::from_code(self.kv_get(account_id, MODE_KEY)?.as_deref()))
    }

    pub fn set_remote_pictures(&self, account_id: &str, mode: RemotePictures) -> Result<()> {
        match mode {
            RemotePictures::Off => self.kv_delete(account_id, MODE_KEY),
            other => self.kv_set(account_id, MODE_KEY, other.code()),
        }
    }

    /// The allowed senders, lower case, in order.
    pub fn remote_allowed_senders(&self, account_id: &str) -> Result<Vec<String>> {
        self.read(|c| {
            let mut stmt = c.prepare("SELECT sender FROM remote_allow WHERE account_id = ?1 ORDER BY sender")?;
            let rows = stmt.query_map([account_id], |r| r.get(0))?;
            Ok(rows.collect::<rusqlite::Result<_>>()?)
        })
    }

    pub fn remote_sender_allowed(&self, account_id: &str, sender: &str) -> Result<bool> {
        let key = sender_key(sender);
        self.read(|c| {
            Ok(c.query_row(
                "SELECT 1 FROM remote_allow WHERE account_id = ?1 AND sender = ?2",
                [account_id, key.as_str()],
                |_| Ok(()),
            )
            .is_ok())
        })
    }

    pub fn remote_allow_sender(&self, account_id: &str, sender: &str) -> Result<()> {
        let key = sender_key(sender);
        if key.is_empty() {
            return Ok(());
        }
        self.write(|c| {
            c.execute(
                "INSERT OR IGNORE INTO remote_allow (account_id, sender) VALUES (?1, ?2)",
                [account_id, key.as_str()],
            )?;
            Ok(())
        })
    }

    pub fn remote_forget_sender(&self, account_id: &str, sender: &str) -> Result<()> {
        let key = sender_key(sender);
        self.write(|c| {
            c.execute("DELETE FROM remote_allow WHERE account_id = ?1 AND sender = ?2", [account_id, key.as_str()])?;
            Ok(())
        })
    }
}
