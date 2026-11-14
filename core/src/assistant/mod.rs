//! The AI assistant (M7, stage 1): summarise, draft a reply, rewrite. The core calls the
//! service the user chose, from here, and nothing is sent unless the assistant is on, the user
//! agreed to this exact service, and this kind of task is allowed.
//!
//! [`settings`] holds the rules for what may be set, [`request`] and [`prompts`] decide what is
//! sent and in what words, [`provider`] and [`sse`] speak to the service. This file keeps the
//! settings in the cache and the key in the host's secret store, and guards the one task that
//! may run at a time.

pub mod models;
pub mod prompts;
pub mod provider;
pub mod request;
pub mod settings;
pub mod sse;

use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};

use tokio::sync::Notify;

use crate::{
    model::{AiProvider, AiSettings, AiTask},
    secrets::SecretStore,
    store::Store,
    Error, Result,
};
use provider::Endpoint;
use settings::{key_name, Profile, Stored, KEY_NAMES, LEGACY_KEY_NAME, SETTINGS_KEY};

/// The settings as stored, with the service in use and its `has_key` read from the secret
/// store. A stored value that cannot be read is the same as nothing stored: the assistant is
/// off and nothing is agreed to.
pub fn load(store: &Store, secrets: &dyn SecretStore, account_id: &str) -> Result<AiSettings> {
    let stored = read(store, secrets, account_id)?;
    view(&stored, stored.provider, secrets)
}

/// What [`load`] would give if `provider` were the one in use. Changes nothing.
pub fn service(store: &Store, secrets: &dyn SecretStore, account_id: &str, provider: AiProvider) -> Result<AiSettings> {
    view(&read(store, secrets, account_id)?, provider, secrets)
}

fn view(stored: &Stored, provider: AiProvider, secrets: &dyn SecretStore) -> Result<AiSettings> {
    let mut s = stored.view(provider);
    s.has_key = stored_key(secrets, provider)?.is_some();
    Ok(s)
}

/// The stored settings. A key that stage 1 left under its one name is moved to the name of its
/// service first.
fn read(store: &Store, secrets: &dyn SecretStore, account_id: &str) -> Result<Stored> {
    let raw = store.kv_get(account_id, SETTINGS_KEY)?;
    let (stored, old_provider) = raw.as_deref().and_then(settings::parse).unwrap_or_default();
    move_old_key(secrets, old_provider)?;
    Ok(stored)
}

/// Stage 1 had one key, under one name, for the one service it stored. It goes to that
/// service's name (unless that service already has a key), and the old name is removed. When
/// nothing says which service it was for, it is removed and not used.
fn move_old_key(secrets: &dyn SecretStore, old_provider: Option<AiProvider>) -> Result<()> {
    let Some(old) = secrets.get(LEGACY_KEY_NAME.into()).map_err(|_| key_store_error())? else { return Ok(()) };
    if let Some(provider) = old_provider.filter(|_| !old.is_empty()) {
        if stored_key(secrets, provider)?.is_none() {
            secrets.set(key_name(provider).into(), old).map_err(|_| key_store_error())?;
        }
    }
    secrets.delete(LEGACY_KEY_NAME.into()).map_err(|_| key_store_error())
}

/// Checks and stores `new` as the settings of the service `new.provider`, which becomes the one
/// in use, and changes that service's key as `key` says (`None` keeps it, a blank text removes
/// it, anything else replaces it). The other service's profile, key and consent are never
/// touched. Nothing is stored when something is wrong.
///
/// A change of this service's address or model clears its `consented` (the host asks again and
/// saves once more with the same values), and a key that is kept is dropped when the scheme,
/// host or port of the address changes, so it is never sent somewhere it was not meant for.
pub fn save(
    store: &Store,
    secrets: &dyn SecretStore,
    account_id: &str,
    new: AiSettings,
    key: Option<String>,
) -> Result<AiSettings> {
    let mut stored = read(store, secrets, account_id)?;
    let provider = new.provider;
    let mut new = settings::normalise(new)?;
    let before = stored.profile(provider).clone();
    new.consented = settings::consent_after(&before, &new);
    let key = match key.map(|k| k.trim().to_string()) {
        None if !settings::same_origin(&before.base_url, &new.base_url) => Some(String::new()),
        key => key,
    };
    if let Some(k) = &key {
        settings::check_key(k)?;
    }
    stored.enabled = new.enabled;
    stored.provider = provider;
    stored.allow_summaries = new.allow_summaries;
    stored.allow_drafts = new.allow_drafts;
    stored.allow_rewrites = new.allow_rewrites;
    *stored.profile_mut(provider) =
        Profile { base_url: new.base_url, model: new.model, effort: new.effort, consented: new.consented };
    let json = serde_json::to_string(&stored).map_err(|e| Error::store(format!("settings: {e}")))?;
    match key {
        Some(k) if k.is_empty() => secrets.delete(key_name(provider).into()).map_err(|_| key_store_error())?,
        Some(k) => secrets.set(key_name(provider).into(), k).map_err(|_| key_store_error())?,
        None => {}
    }
    store.kv_set(account_id, SETTINGS_KEY, &json)?;
    load(store, secrets, account_id)
}

/// Removes every key from the secret store (sign-out), the one stage 1 used included.
pub fn forget_key(secrets: &dyn SecretStore) -> Result<()> {
    let mut failed = false;
    for name in KEY_NAMES.iter().chain([&LEGACY_KEY_NAME]) {
        failed |= secrets.delete((*name).into()).is_err();
    }
    if failed {
        Err(key_store_error())
    } else {
        Ok(())
    }
}

fn stored_key(secrets: &dyn SecretStore, provider: AiProvider) -> Result<Option<String>> {
    Ok(secrets.get(key_name(provider).into()).map_err(|_| key_store_error())?.filter(|k| !k.is_empty()))
}

/// The key to ask a service for its models with. A key the user just typed is used as given
/// (a blank one means none). Otherwise the stored key of that provider, and only when the
/// address has the scheme, host and port the stored profile has: a stored key is never sent
/// to an address it was not entered for.
pub fn models_key(
    store: &Store,
    secrets: &dyn SecretStore,
    account_id: &str,
    asked: &AiSettings,
    typed: Option<String>,
) -> Result<Option<String>> {
    if let Some(typed) = typed {
        let typed = typed.trim();
        settings::check_key(typed)?;
        return Ok(Some(typed.to_string()).filter(|k| !k.is_empty()));
    }
    let stored = read(store, secrets, account_id)?;
    if !settings::same_origin(&stored.profile(asked.provider).base_url, &asked.base_url) {
        return Ok(None);
    }
    stored_key(secrets, asked.provider)
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
        effort: s.effort,
        key: stored_key(secrets, s.provider)?,
    })
}

/// How many stops for runs that were not running are remembered.
const REMEMBERED: usize = 16;

#[derive(Default)]
struct SlotState {
    /// The run that holds the slot: its id and the way to stop it.
    running: Option<(String, Arc<Notify>)>,
    /// Runs that were told to stop before they began, newest last.
    stopped: VecDeque<String>,
}

/// The one task that may run at a time, and the way to stop it by name.
#[derive(Default)]
pub struct Slot(Mutex<SlotState>);

impl Slot {
    fn state(&self) -> std::sync::MutexGuard<'_, SlotState> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Takes the slot for the run `run_id`, or says that a task is already running. A run that
    /// was stopped before it began is refused at once, as cancelled, and forgotten.
    pub fn begin(&self, run_id: &str) -> Result<Running<'_>> {
        let mut state = self.state();
        if let Some(at) = state.stopped.iter().position(|id| id == run_id) {
            state.stopped.remove(at);
            return Err(Error::invalid("cancelled"));
        }
        if state.running.is_some() {
            return Err(Error::invalid("the assistant is already working on something"));
        }
        let cancel = Arc::new(Notify::new());
        state.running = Some((run_id.to_string(), cancel.clone()));
        Ok(Running { slot: self, cancel })
    }

    /// Stops the run `run_id` if it is the one running (a stop that comes before it first looks
    /// is kept). If it is not running, the stop is remembered for when it begins. Another run is
    /// never touched.
    pub fn cancel(&self, run_id: &str) {
        let mut state = self.state();
        match &state.running {
            Some((id, cancel)) if id == run_id => cancel.notify_one(),
            _ => {
                if !state.stopped.iter().any(|id| id == run_id) {
                    if state.stopped.len() == REMEMBERED {
                        state.stopped.pop_front();
                    }
                    state.stopped.push_back(run_id.to_string());
                }
            }
        }
    }

    /// Stops whatever runs (sign-out), whatever its name.
    pub fn cancel_all(&self) {
        if let Some((_, cancel)) = &self.state().running {
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
        self.slot.state().running = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        model::{Account, AiEffort},
        secrets::MemorySecrets,
    };

    const OPENAI: &str = "ai_key_openai";
    const ANTHROPIC: &str = "ai_key_anthropic";

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

    fn other() -> AiSettings {
        AiSettings {
            provider: AiProvider::OpenAiCompatible,
            base_url: "https://llm.example.org/v1".into(),
            model: "o".into(),
            effort: AiEffort::High,
            ..on()
        }
    }

    fn key(secrets: &MemorySecrets, name: &str) -> Option<String> {
        secrets.get(name.into()).unwrap()
    }

    /// Saves twice, the second call being the grant.
    fn agreed(store: &Store, secrets: &MemorySecrets, s: AiSettings, k: Option<String>) -> AiSettings {
        save(store, secrets, "a", s.clone(), k).unwrap();
        save(store, secrets, "a", s, None).unwrap()
    }

    #[test]
    fn nothing_stored_is_off_and_not_agreed() {
        let (store, secrets) = rig();
        assert_eq!(load(&store, &secrets, "a").unwrap(), settings::defaults());
        store.kv_set("a", SETTINGS_KEY, "{not json").unwrap();
        assert_eq!(load(&store, &secrets, "a").unwrap(), settings::defaults());
    }

    #[test]
    fn the_key_goes_to_the_secret_store_and_never_into_the_cache_a_debug_text_or_an_error() {
        let (store, secrets) = rig();
        let saved = save(&store, &secrets, "a", on(), Some(" sk-123 ".into())).unwrap();
        assert!(saved.has_key);
        assert_eq!(key(&secrets, ANTHROPIC).as_deref(), Some("sk-123"));
        assert_eq!(key(&secrets, OPENAI), None);
        let json = store.kv_get("a", SETTINGS_KEY).unwrap().unwrap();
        assert!(!json.contains("sk-123") && !json.contains("has_key"));
        assert!(!format!("{saved:?}").contains("sk-123"));
        // None keeps it, a blank removes it.
        assert!(save(&store, &secrets, "a", on(), None).unwrap().has_key);
        assert!(!save(&store, &secrets, "a", on(), Some("  ".into())).unwrap().has_key);
        assert_eq!(key(&secrets, ANTHROPIC), None);
        // A refused key is not repeated.
        let refused = save(&store, &secrets, "a", on(), Some("sk-bad key".into())).unwrap_err();
        assert!(!refused.to_string().contains("sk-bad"));
    }

    #[test]
    fn consent_is_granted_by_repeating_the_stored_values_and_cleared_by_a_change() {
        let (store, secrets) = rig();
        // A first setup never starts agreed, whatever the caller says.
        assert!(!save(&store, &secrets, "a", on(), None).unwrap().consented);
        // The second call repeats the stored values: that is the grant.
        assert!(save(&store, &secrets, "a", on(), None).unwrap().consented);
        // A value copied from the stored settings survives a save that changes nothing else...
        let quiet = AiSettings { allow_drafts: false, effort: AiEffort::Max, ..on() };
        assert!(save(&store, &secrets, "a", quiet, None).unwrap().consented);
        // ...but not a change of the model or the address.
        for moved in [
            AiSettings { model: "m2".into(), ..on() },
            AiSettings { base_url: "https://other.example.com/v1".into(), ..on() },
        ] {
            agreed(&store, &secrets, on(), None);
            assert!(!save(&store, &secrets, "a", moved, None).unwrap().consented);
        }
    }

    #[test]
    fn a_service_keeps_its_own_profile_key_and_consent() {
        let (store, secrets) = rig();
        agreed(&store, &secrets, on(), Some("sk-a".into()));
        let before = service(&store, &secrets, "a", AiProvider::Anthropic).unwrap();
        // Saving the other service changes nothing of the first.
        let second = agreed(&store, &secrets, other(), Some("sk-o".into()));
        assert!(second.consented && second.has_key && second.effort == AiEffort::High);
        assert_eq!(load(&store, &secrets, "a").unwrap().provider, AiProvider::OpenAiCompatible);
        let after = service(&store, &secrets, "a", AiProvider::Anthropic).unwrap();
        assert_eq!(after, AiSettings { provider: AiProvider::Anthropic, ..before.clone() });
        assert!(after.consented && after.has_key);
        assert_eq!(
            (key(&secrets, ANTHROPIC).as_deref(), key(&secrets, OPENAI).as_deref()),
            (Some("sk-a"), Some("sk-o"))
        );
        // Changing the second one's model clears only its consent.
        let moved = AiSettings { model: "o2".into(), ..other() };
        assert!(!save(&store, &secrets, "a", moved, None).unwrap().consented);
        assert!(service(&store, &secrets, "a", AiProvider::Anthropic).unwrap().consented);
        // Switching back and forth with nothing changed keeps the consent of the first.
        let back = save(&store, &secrets, "a", before.clone(), None).unwrap();
        assert!(back.consented && back.has_key && back.provider == AiProvider::Anthropic);
        assert!(!service(&store, &secrets, "a", AiProvider::OpenAiCompatible).unwrap().consented);
        // The shared switches are shared.
        let quiet = save(&store, &secrets, "a", AiSettings { allow_rewrites: false, ..before }, None).unwrap();
        assert!(!quiet.allow_rewrites);
        assert!(!service(&store, &secrets, "a", AiProvider::OpenAiCompatible).unwrap().allow_rewrites);
    }

    #[test]
    fn ai_service_changes_nothing_and_a_new_anthropic_profile_has_its_own_address() {
        let (store, secrets) = rig();
        let fresh = service(&store, &secrets, "a", AiProvider::Anthropic).unwrap();
        assert_eq!(fresh.base_url, "https://api.anthropic.com/v1");
        assert_eq!(
            (fresh.provider, fresh.model.as_str(), fresh.consented, fresh.has_key),
            (AiProvider::Anthropic, "", false, false)
        );
        assert_eq!(service(&store, &secrets, "a", AiProvider::OpenAiCompatible).unwrap().base_url, "");
        assert_eq!(store.kv_get("a", SETTINGS_KEY).unwrap(), None);
        assert_eq!(load(&store, &secrets, "a").unwrap(), settings::defaults());
    }

    #[test]
    fn a_kept_key_does_not_follow_a_change_of_address() {
        let (store, secrets) = rig();
        save(&store, &secrets, "a", on(), Some("sk-1".into())).unwrap();
        // The same host with another path or model keeps it.
        let path = AiSettings { base_url: "https://api.example.com/other".into(), model: "m2".into(), ..on() };
        assert!(save(&store, &secrets, "a", path, None).unwrap().has_key);
        // Another host, port or scheme drops it, unless a new key comes with it.
        let moved = |url: &str| AiSettings { base_url: url.into(), ..on() };
        for url in ["https://api.other.com/v1", "https://api.example.com:8443/v1", "http://127.0.0.1:1/v1"] {
            save(&store, &secrets, "a", on(), Some("sk-1".into())).unwrap();
            assert!(!save(&store, &secrets, "a", moved(url), None).unwrap().has_key, "{url}");
            assert_eq!(key(&secrets, ANTHROPIC), None);
        }
        save(&store, &secrets, "a", on(), Some("sk-1".into())).unwrap();
        let fresh = save(&store, &secrets, "a", moved("https://api.other.com/v1"), Some("sk-2".into())).unwrap();
        assert!(fresh.has_key);
        assert_eq!(key(&secrets, ANTHROPIC).as_deref(), Some("sk-2"));
        // Another service's key is not the one that goes.
        save(&store, &secrets, "a", other(), Some("sk-o".into())).unwrap();
        save(&store, &secrets, "a", AiSettings { base_url: "https://x.example/v1".into(), ..other() }, None).unwrap();
        assert_eq!(key(&secrets, OPENAI), None);
        assert_eq!(key(&secrets, ANTHROPIC).as_deref(), Some("sk-2"));
    }

    #[test]
    fn a_refused_change_stores_nothing() {
        let (store, secrets) = rig();
        save(&store, &secrets, "a", on(), Some("old".into())).unwrap();
        save(&store, &secrets, "a", other(), Some("old-o".into())).unwrap();
        let stored = store.kv_get("a", SETTINGS_KEY).unwrap();
        let bad = AiSettings { base_url: "http://api.example.com".into(), ..on() };
        assert!(matches!(save(&store, &secrets, "a", bad, Some("new".into())), Err(Error::Invalid { .. })));
        assert!(matches!(save(&store, &secrets, "a", on(), Some("bad key".into())), Err(Error::Invalid { .. })));
        assert!(matches!(
            save(&store, &secrets, "a", AiSettings { model: " ".into(), ..on() }, Some("new".into())),
            Err(Error::Invalid { .. })
        ));
        assert_eq!(key(&secrets, ANTHROPIC).as_deref(), Some("old"));
        assert_eq!(key(&secrets, OPENAI).as_deref(), Some("old-o"));
        assert_eq!(store.kv_get("a", SETTINGS_KEY).unwrap(), stored);
    }

    #[test]
    fn stage_one_data_moves_to_the_provider_it_had_and_its_consent_survives() {
        for (provider, name, flat_name) in [
            (AiProvider::Anthropic, ANTHROPIC, "Anthropic"),
            (AiProvider::OpenAiCompatible, OPENAI, "OpenAiCompatible"),
        ] {
            let (store, secrets) = rig();
            let flat = format!(
                r#"{{"enabled":true,"provider":"{flat_name}","base_url":"https://api.example.com/v1","model":"m",
                "has_key":false,"consented":true,"allow_summaries":true,"allow_drafts":true,"allow_rewrites":true}}"#
            );
            store.kv_set("a", SETTINGS_KEY, &flat).unwrap();
            secrets.set(LEGACY_KEY_NAME.into(), "sk-old".into()).unwrap();
            let loaded = load(&store, &secrets, "a").unwrap();
            assert_eq!(loaded, AiSettings { has_key: true, provider, ..on() });
            assert_eq!(key(&secrets, LEGACY_KEY_NAME), None);
            assert_eq!(key(&secrets, name).as_deref(), Some("sk-old"));
            let rest = if provider == AiProvider::Anthropic { OPENAI } else { ANTHROPIC };
            assert_eq!(key(&secrets, rest), None);
            // A save keeps the migrated profile and writes the new form.
            let saved = save(&store, &secrets, "a", loaded.clone(), None).unwrap();
            assert_eq!(saved, loaded);
            let json = store.kv_get("a", SETTINGS_KEY).unwrap().unwrap();
            assert_eq!(settings::parse(&json).map(|(_, old)| old), Some(None), "written in the new form");
        }
        // An old key with nothing to say which service it was for is removed, not used.
        let (store, secrets) = rig();
        secrets.set(LEGACY_KEY_NAME.into(), "sk-old".into()).unwrap();
        assert!(!load(&store, &secrets, "a").unwrap().has_key);
        assert_eq!(key(&secrets, LEGACY_KEY_NAME), None);
    }

    #[test]
    fn sign_out_removes_every_key() {
        let (_store, secrets) = rig();
        for name in [OPENAI, ANTHROPIC, LEGACY_KEY_NAME] {
            secrets.set(name.into(), "k".into()).unwrap();
        }
        forget_key(&secrets).unwrap();
        assert!([OPENAI, ANTHROPIC, LEGACY_KEY_NAME].iter().all(|n| key(&secrets, n).is_none()));
    }

    #[test]
    fn the_key_for_the_model_list_is_a_typed_one_or_the_stored_one_for_the_same_origin() {
        let (store, secrets) = rig();
        agreed(&store, &secrets, on(), Some("sk-a".into()));
        let ask = |s: AiSettings, typed: Option<&str>| {
            models_key(&store, &secrets, "a", &s, typed.map(String::from)).unwrap()
        };
        assert_eq!(ask(on(), None).as_deref(), Some("sk-a"));
        let path = AiSettings { base_url: "https://api.example.com/other".into(), ..on() };
        assert_eq!(ask(path, None).as_deref(), Some("sk-a"));
        for url in ["https://elsewhere.example/v1", "http://api.example.com/v1", "https://api.example.com:8443/v1"] {
            assert_eq!(ask(AiSettings { base_url: url.into(), ..on() }, None), None, "{url}");
        }
        // The other provider's key is not this one's.
        assert_eq!(ask(AiSettings { provider: AiProvider::OpenAiCompatible, ..on() }, None), None);
        // A typed key is used as given, wherever it goes; blank is none.
        let elsewhere = AiSettings { base_url: "https://elsewhere.example/v1".into(), ..on() };
        assert_eq!(ask(elsewhere.clone(), Some(" sk-t ")).as_deref(), Some("sk-t"));
        assert_eq!(ask(on(), Some("  ")), None);
        assert!(models_key(&store, &secrets, "a", &elsewhere, Some("sk bad".into())).is_err());
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

    fn waits(rt: &tokio::runtime::Runtime, run: &Running<'_>, ms: u64) -> bool {
        rt.block_on(async { tokio::time::timeout(std::time::Duration::from_millis(ms), run.cancel.notified()).await })
            .is_ok()
    }

    fn runtime() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap()
    }

    #[test]
    fn only_one_task_runs_and_a_stop_by_name_stops_that_run() {
        let slot = Slot::default();
        let rt = runtime();
        let running = slot.begin("a").unwrap();
        assert!(matches!(slot.begin("b"), Err(Error::Invalid { msg }) if msg.contains("already working")));
        // Another run's stop does not touch it.
        slot.cancel("zzz");
        assert!(!waits(&rt, &running, 50));
        // Its own does, even before it looks.
        slot.cancel("a");
        assert!(waits(&rt, &running, 1000));
        drop(running);
        assert!(slot.begin("b").is_ok(), "freed again");
    }

    #[test]
    fn a_stop_that_comes_before_its_run_is_remembered_once_and_hits_nothing_else() {
        let slot = Slot::default();
        let rt = runtime();
        // The old race: the stop for "a" arrives, then "b" starts. "b" is not stopped.
        slot.cancel("a");
        let b = slot.begin("b").unwrap();
        assert!(!waits(&rt, &b, 50));
        // "a" starts later and is refused as cancelled; "c" cannot start while "b" runs, and "a"
        // is forgotten after that.
        assert!(matches!(slot.begin("a"), Err(Error::Invalid { msg }) if msg == "cancelled"));
        drop(b);
        assert!(slot.begin("a").is_ok());
        // Stopping the same id twice is remembered once.
        let slot = Slot::default();
        slot.cancel("x");
        slot.cancel("x");
        assert!(slot.begin("x").is_err());
        assert!(slot.begin("x").is_ok());
    }

    #[test]
    fn only_the_last_sixteen_stops_are_remembered() {
        let slot = Slot::default();
        for n in 0..17 {
            slot.cancel(&format!("r{n}"));
        }
        assert!(slot.begin("r16").is_err());
        assert!(slot.begin("r1").is_err());
        assert!(slot.begin("r0").is_ok(), "the oldest was forgotten");
    }

    #[test]
    fn cancel_all_stops_whatever_runs() {
        let slot = Slot::default();
        let rt = runtime();
        slot.cancel_all(); // nothing runs: no effect on what comes next
        let running = slot.begin("a").unwrap();
        assert!(!waits(&rt, &running, 50));
        slot.cancel_all();
        assert!(waits(&rt, &running, 1000));
    }
}
