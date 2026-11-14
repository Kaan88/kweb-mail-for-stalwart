//! The AI assistant (M7, stage 1): summarise, draft a reply, rewrite. The core calls the
//! service the user chose, from here, and nothing is sent unless the assistant is on, the user
//! agreed to this exact service, and this kind of task is allowed.
//!
//! [`settings`] holds the rules for what may be set, [`request`] and [`prompts`] decide what is
//! sent and in what words, [`provider`] and [`sse`] speak to the service. This file keeps the
//! settings in the cache and the key in the host's secret store, and guards the one task that
//! may run at a time.

pub mod prompts;
pub mod provider;
pub mod request;
pub mod settings;
pub mod sse;

use std::sync::{Arc, Mutex};

use tokio::sync::Notify;

use crate::{
    model::{AiSettings, AiTask},
    secrets::SecretStore,
    store::Store,
    Error, Result,
};
use provider::Endpoint;
use settings::{KEY_NAME, SETTINGS_KEY};

/// The settings as stored, with `has_key` read from the secret store. A stored value that
/// cannot be read is the same as nothing stored: the assistant is off and nothing is agreed to.
pub fn load(store: &Store, secrets: &dyn SecretStore, account_id: &str) -> Result<AiSettings> {
    let mut s = stored_settings(store, account_id)?;
    s.has_key = stored_key(secrets)?.is_some();
    Ok(s)
}

fn stored_settings(store: &Store, account_id: &str) -> Result<AiSettings> {
    Ok(store
        .kv_get(account_id, SETTINGS_KEY)?
        .and_then(|json| serde_json::from_str::<AiSettings>(&json).ok())
        .unwrap_or_else(settings::defaults))
}

/// Checks and stores `new`, and changes the key as `key` says (`None` keeps it, a blank text
/// removes it, anything else replaces it). Nothing is stored when something is wrong.
///
/// A change of provider, address or model clears `consented` (the host asks again and saves
/// once more with the same values), and a key that is kept is dropped when the service it was
/// entered for is no longer the one set, so it is never sent somewhere it was not meant for.
pub fn save(
    store: &Store,
    secrets: &dyn SecretStore,
    account_id: &str,
    new: AiSettings,
    key: Option<String>,
) -> Result<AiSettings> {
    let before = stored_settings(store, account_id)?;
    let mut stored = settings::normalise(new)?;
    stored.consented = settings::consent_after(&before, &stored);
    let key = match key.map(|k| k.trim().to_string()) {
        None if !settings::keeps_key(&before, &stored) => Some(String::new()),
        key => key,
    };
    if let Some(k) = &key {
        settings::check_key(k)?;
    }
    let json = serde_json::to_string(&stored).map_err(|e| Error::store(format!("settings: {e}")))?;
    match key {
        Some(k) if k.is_empty() => forget_key(secrets)?,
        Some(k) => secrets.set(KEY_NAME.into(), k).map_err(|_| key_store_error())?,
        None => {}
    }
    store.kv_set(account_id, SETTINGS_KEY, &json)?;
    load(store, secrets, account_id)
}

/// Removes the key from the secret store (sign-out).
pub fn forget_key(secrets: &dyn SecretStore) -> Result<()> {
    secrets.delete(KEY_NAME.into()).map_err(|_| key_store_error())
}

fn stored_key(secrets: &dyn SecretStore) -> Result<Option<String>> {
    Ok(secrets.get(KEY_NAME.into()).map_err(|_| key_store_error())?.filter(|k| !k.is_empty()))
}

fn key_store_error() -> Error {
    Error::temporary("the secret store could not be used just now")
}

/// Whether `task` may be sent under these settings: the assistant is on, the user agreed to
/// exactly this service, and the kind of task is not switched off.
pub fn check_allowed(s: &AiSettings, task: &AiTask) -> Result<()> {
    if !s.enabled {
        return Err(Error::invalid("the assistant is switched off in the settings"));
    }
    if !s.consented {
        return Err(Error::invalid("you have not yet agreed to send mail to this service"));
    }
    match task {
        AiTask::SummarizeMessage { .. } | AiTask::SummarizeThread { .. } if !s.allow_summaries => {
            Err(Error::invalid("summaries are switched off in the settings"))
        }
        AiTask::DraftReply { .. } if !s.allow_drafts => {
            Err(Error::invalid("drafting replies is switched off in the settings"))
        }
        AiTask::Rewrite { .. } if !s.allow_rewrites => Err(Error::invalid("rewriting is switched off in the settings")),
        _ => Ok(()),
    }
}

/// Where requests go: the settings, and the key if there is one.
pub fn endpoint(s: &AiSettings, secrets: &dyn SecretStore) -> Result<Endpoint> {
    settings::check_base_url(&s.base_url)?;
    Ok(Endpoint {
        provider: s.provider,
        base_url: s.base_url.clone(),
        model: s.model.clone(),
        key: stored_key(secrets)?,
    })
}

/// The one task that may run at a time, and the way to stop it.
#[derive(Default)]
pub struct Slot(Mutex<Option<Arc<Notify>>>);

impl Slot {
    /// Takes the slot, or says that a task is already running.
    pub fn begin(&self) -> Result<Running<'_>> {
        let mut busy = self.0.lock().unwrap_or_else(|e| e.into_inner());
        if busy.is_some() {
            return Err(Error::invalid("the assistant is already working on something"));
        }
        let cancel = Arc::new(Notify::new());
        *busy = Some(cancel.clone());
        Ok(Running { slot: self, cancel })
    }

    /// Stops the running task, if any. A stop that comes before the task first looks is kept.
    pub fn cancel(&self) {
        if let Some(cancel) = self.0.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
            cancel.notify_one();
        }
    }
}

/// A task that holds the slot; dropping it frees the slot.
pub struct Running<'a> {
    slot: &'a Slot,
    pub cancel: Arc<Notify>,
}

impl Drop for Running<'_> {
    fn drop(&mut self) {
        *self.slot.0.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        model::{Account, AiProvider},
        secrets::MemorySecrets,
    };

    fn rig() -> (Store, MemorySecrets) {
        let store = Store::open_in_memory().unwrap();
        store
            .save_account(&Account {
                id: "a".into(),
                server_url: "http://x".into(),
                username: "u".into(),
                signed_in_at: 0,
            })
            .unwrap();
        (store, MemorySecrets::default())
    }

    fn on() -> AiSettings {
        AiSettings {
            enabled: true,
            provider: AiProvider::Anthropic,
            base_url: "https://api.example.com/v1".into(),
            model: "m".into(),
            consented: true,
            ..settings::defaults()
        }
    }

    #[test]
    fn nothing_stored_is_off_and_not_agreed() {
        let (store, secrets) = rig();
        assert_eq!(load(&store, &secrets, "a").unwrap(), settings::defaults());
        store.kv_set("a", SETTINGS_KEY, "{not json").unwrap();
        assert_eq!(load(&store, &secrets, "a").unwrap(), settings::defaults());
    }

    #[test]
    fn the_key_goes_to_the_secret_store_and_never_into_the_cache() {
        let (store, secrets) = rig();
        let saved = save(&store, &secrets, "a", on(), Some(" sk-123 ".into())).unwrap();
        assert!(saved.has_key);
        assert_eq!(secrets.get(KEY_NAME.into()).unwrap().as_deref(), Some("sk-123"));
        let json = store.kv_get("a", SETTINGS_KEY).unwrap().unwrap();
        assert!(!json.contains("sk-123") && json.contains("\"has_key\":false"));
        // None keeps it, a blank removes it.
        assert!(save(&store, &secrets, "a", on(), None).unwrap().has_key);
        assert!(!save(&store, &secrets, "a", on(), Some("  ".into())).unwrap().has_key);
        assert_eq!(secrets.get(KEY_NAME.into()).unwrap(), None);
    }

    #[test]
    fn consent_is_granted_by_repeating_the_stored_values_and_cleared_by_a_change() {
        let (store, secrets) = rig();
        // A first setup never starts agreed, whatever the caller says.
        assert!(!save(&store, &secrets, "a", on(), None).unwrap().consented);
        // The second call repeats the stored values: that is the grant.
        assert!(save(&store, &secrets, "a", on(), None).unwrap().consented);
        // A value copied from the stored settings survives a save that changes nothing...
        assert!(save(&store, &secrets, "a", AiSettings { allow_drafts: false, ..on() }, None).unwrap().consented);
        // ...but not a change of the model, the address or the provider.
        for moved in [
            AiSettings { model: "m2".into(), ..on() },
            AiSettings { base_url: "https://other.example.com/v1".into(), ..on() },
            AiSettings { provider: AiProvider::OpenAiCompatible, ..on() },
        ] {
            save(&store, &secrets, "a", on(), None).unwrap(); // back to the base service...
            assert!(save(&store, &secrets, "a", on(), None).unwrap().consented); // ...and agreed again
            assert!(!save(&store, &secrets, "a", moved, None).unwrap().consented);
        }
    }

    #[test]
    fn a_kept_key_does_not_follow_a_change_of_service() {
        let (store, secrets) = rig();
        save(&store, &secrets, "a", on(), Some("sk-1".into())).unwrap();
        // The same host with another path or model keeps it.
        let path = AiSettings { base_url: "https://api.example.com/other".into(), model: "m2".into(), ..on() };
        assert!(save(&store, &secrets, "a", path, None).unwrap().has_key);
        // Another host, port, scheme or provider drops it, unless a new key comes with it.
        let moved = |url: &str| AiSettings { base_url: url.into(), ..on() };
        for url in ["https://api.other.com/v1", "https://api.example.com:8443/v1", "http://127.0.0.1:1/v1"] {
            save(&store, &secrets, "a", on(), Some("sk-1".into())).unwrap();
            assert!(!save(&store, &secrets, "a", moved(url), None).unwrap().has_key, "{url}");
            assert_eq!(secrets.get(KEY_NAME.into()).unwrap(), None);
        }
        save(&store, &secrets, "a", on(), Some("sk-1".into())).unwrap();
        let other = AiSettings { provider: AiProvider::OpenAiCompatible, ..on() };
        assert!(!save(&store, &secrets, "a", other, None).unwrap().has_key);
        save(&store, &secrets, "a", on(), Some("sk-1".into())).unwrap();
        let fresh = save(&store, &secrets, "a", moved("https://api.other.com/v1"), Some("sk-2".into())).unwrap();
        assert!(fresh.has_key);
        assert_eq!(secrets.get(KEY_NAME.into()).unwrap().as_deref(), Some("sk-2"));
    }

    #[test]
    fn a_refused_change_stores_nothing() {
        let (store, secrets) = rig();
        save(&store, &secrets, "a", on(), Some("old".into())).unwrap();
        let bad = AiSettings { base_url: "http://api.example.com".into(), ..on() };
        assert!(matches!(save(&store, &secrets, "a", bad, Some("new".into())), Err(Error::Invalid { .. })));
        assert!(matches!(save(&store, &secrets, "a", on(), Some("bad key".into())), Err(Error::Invalid { .. })));
        assert_eq!(secrets.get(KEY_NAME.into()).unwrap().as_deref(), Some("old"));
        assert_eq!(load(&store, &secrets, "a").unwrap().base_url, "https://api.example.com/v1");
    }

    #[test]
    fn each_refusal_has_its_own_words() {
        let task = |t: AiTask| check_allowed(&on(), &t).map_err(|e| e.to_string());
        let summary = AiTask::SummarizeMessage { message_id: "m".into() };
        let draft = AiTask::DraftReply { message_id: "m".into(), instruction: String::new() };
        let rewrite = AiTask::Rewrite { text: "x".into(), style: crate::model::RewriteStyle::Improve };
        assert!([&summary, &draft, &rewrite].iter().all(|t| task((*t).clone()).is_ok()));

        let refused = |s: AiSettings, t: &AiTask| check_allowed(&s, t).unwrap_err().to_string();
        assert!(refused(AiSettings { enabled: false, ..on() }, &summary).contains("switched off"));
        assert!(refused(AiSettings { consented: false, ..on() }, &summary).contains("agreed"));
        assert!(refused(AiSettings { allow_summaries: false, ..on() }, &summary).contains("summaries"));
        assert!(refused(
            AiSettings { allow_summaries: false, ..on() },
            &AiTask::SummarizeThread { thread_id: "t".into() }
        )
        .contains("summaries"));
        assert!(refused(AiSettings { allow_drafts: false, ..on() }, &draft).contains("replies"));
        assert!(refused(AiSettings { allow_rewrites: false, ..on() }, &rewrite).contains("rewriting"));
        // A switch for one kind leaves the others alone.
        assert!(check_allowed(&AiSettings { allow_drafts: false, ..on() }, &summary).is_ok());
    }

    #[test]
    fn only_one_task_runs_and_a_stop_is_remembered_until_it_looks() {
        let slot = Slot::default();
        slot.cancel();
        let running = slot.begin().unwrap();
        assert!(matches!(slot.begin(), Err(Error::Invalid { .. })));
        slot.cancel();
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        rt.block_on(async { running.cancel.notified().await });
        drop(running);
        assert!(slot.begin().is_ok(), "freed again");
        // A stop with no task running does not hit the next one.
        let slot = Slot::default();
        slot.cancel();
        let next = slot.begin().unwrap();
        let waited = rt.block_on(async {
            tokio::time::timeout(std::time::Duration::from_millis(50), next.cancel.notified()).await
        });
        assert!(waited.is_err());
    }
}
