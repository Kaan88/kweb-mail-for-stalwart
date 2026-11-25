//! The AI assistant (M7). Stage 1: summarise, draft a reply, rewrite. Stage 2: reply ideas,
//! to-dos and dates, explain. Stages 3 and 4: the chat, which reads with tools and only ever
//! suggests changes. The core calls the service the user chose, from here, and nothing is sent
//! unless the assistant is on, the user agreed to this exact service, and this kind of task is
//! allowed.
//!
//! [`settings`] holds the rules for what may be set, [`request`] and [`prompts`] decide what is
//! sent and in what words, [`provider`] and [`sse`] speak to the service, [`insights`] reads the
//! stage 2 answers. [`wire`] is what the provider and the chat loop pass each other, [`chat`] is
//! the loop, [`tools`] what it may read and [`changes`] what a suggestion does once the user
//! approves it. This file keeps the settings in the cache and the key in the host's secret store,
//! and guards the one task that may run at a time.

pub mod catchup;
pub mod changes;
pub mod chat;
pub mod insights;
pub mod models;
pub mod prompts;
pub mod provider;
pub mod request;
pub mod settings;
pub mod sparkle;
pub mod sse;
pub mod tools;
pub mod wire;

use std::{
    collections::VecDeque,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
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
///
/// The yes for the chat is never taken from `new.chat_consented`: it stays only while it was
/// stored and the first yes survived this save, so a new address or model, or a withdrawn yes,
/// clears it too. Only [`chat_consent`] gives it.
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
    stored.allow_ideas = new.allow_ideas;
    stored.allow_extract = new.allow_extract;
    stored.allow_explain = new.allow_explain;
    stored.allow_chat = new.allow_chat;
    stored.actions = new.actions;
    stored.instructions = new.instructions.clone();
    let (base_url, model) = (new.base_url.clone(), new.model.clone());
    *stored.profile_mut(provider) = Profile {
        base_url: new.base_url,
        model: new.model,
        effort: new.effort,
        consented: new.consented,
        // Never taken from the caller: the yes for the chat lives only as long as the first one.
        chat_consented: before.chat_consented && new.consented,
        // Only a chat finds this out; it holds for this address and model and no other.
        tools_unsupported: before.tools_unsupported
            && (before.base_url.as_str(), before.model.as_str()) == (base_url.as_str(), model.as_str()),
    };
    let json = serde_json::to_string(&stored).map_err(|e| Error::store(format!("settings: {e}")))?;
    match key {
        Some(k) if k.is_empty() => secrets.delete(key_name(provider).into()).map_err(|_| key_store_error())?,
        Some(k) => secrets.set(key_name(provider).into(), k).map_err(|_| key_store_error())?,
        None => {}
    }
    store.kv_set(account_id, SETTINGS_KEY, &json)?;
    load(store, secrets, account_id)
}

/// Puts the account's assistant settings back to how it is before the user has done anything
/// (off, nothing agreed to), and removes every stored key, the stage 1 one included. The keys go
/// first: when one cannot be removed nothing else changes and the error says so, so the screen
/// never shows settings that look reset while a key is still stored. Local: sends nothing, and
/// no key is read.
pub fn reset(store: &Store, secrets: &dyn SecretStore, account_id: &str) -> Result<AiSettings> {
    forget_key(secrets)?;
    store.kv_delete(account_id, SETTINGS_KEY)?;
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
        AiTask::ReplyIdeas { .. } if !s.allow_ideas => {
            Err(Error::invalid("reply ideas are switched off in the settings"))
        }
        AiTask::Extract { .. } | AiTask::ExtractThread { .. } if !s.allow_extract => {
            Err(Error::invalid("finding to-dos and dates is switched off in the settings"))
        }
        AiTask::Explain { .. } if !s.allow_explain => Err(Error::invalid("explaining is switched off in the settings")),
        AiTask::SummarizeMessage { .. }
        | AiTask::SummarizeThread { .. }
        | AiTask::DraftReply { .. }
        | AiTask::Rewrite { .. }
        | AiTask::ReplyIdeas { .. }
        | AiTask::Extract { .. }
        | AiTask::ExtractThread { .. }
        | AiTask::Explain { .. } => Ok(()),
    }
}

/// Whether the chat may send anything under these settings: the assistant is on, the user agreed
/// to exactly this service, the chat is switched on, and the user gave the chat its own yes (it
/// chooses itself what it reads and sends there).
pub fn check_chat(s: &AiSettings) -> Result<()> {
    if !s.enabled {
        return Err(Error::invalid("the assistant is switched off in the settings"));
    }
    if !s.consented {
        return Err(Error::invalid("you have not yet agreed to send mail to this service"));
    }
    if !s.allow_chat {
        return Err(Error::invalid("the chat is switched off in the settings"));
    }
    if !s.chat_consented {
        return Err(Error::invalid("you have not yet agreed to let the chat read your mail and calendar"));
    }
    Ok(())
}

/// Gives (`agree`) or takes back the yes for the chat, for the service in use, and returns the
/// settings. Giving it needs `consented` (else Invalid "you have not yet agreed to send mail to
/// this service"). Only this function sets `Profile::chat_consented`; a save never does. Local.
pub fn chat_consent(store: &Store, secrets: &dyn SecretStore, account_id: &str, agree: bool) -> Result<AiSettings> {
    let mut stored = read(store, secrets, account_id)?;
    let provider = stored.provider;
    let profile = stored.profile_mut(provider);
    if agree && !profile.consented {
        return Err(Error::invalid("you have not yet agreed to send mail to this service"));
    }
    if profile.chat_consented != agree {
        profile.chat_consented = agree;
        let json = serde_json::to_string(&stored).map_err(|e| Error::store(format!("settings: {e}")))?;
        store.kv_set(account_id, SETTINGS_KEY, &json)?;
    }
    view(&stored, provider, secrets)
}

/// Remembers (`unsupported`) or forgets that the model of the service `provider` takes no tools, for
/// the address and model the chat was held with. Nothing changes when the profile has moved on to
/// another address or model meanwhile. Local.
pub fn note_tools(
    store: &Store,
    secrets: &dyn SecretStore,
    account_id: &str,
    origin: &chat::Origin,
    unsupported: bool,
) -> Result<()> {
    let mut stored = read(store, secrets, account_id)?;
    let profile = stored.profile_mut(origin.provider);
    if profile.base_url != origin.base_url || profile.model != origin.model || profile.tools_unsupported == unsupported
    {
        return Ok(());
    }
    profile.tools_unsupported = unsupported;
    let json = serde_json::to_string(&stored).map_err(|e| Error::store(format!("settings: {e}")))?;
    store.kv_set(account_id, SETTINGS_KEY, &json)
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
    /// The run that holds the slot: its id, the way to stop it while it waits, and the flag that
    /// stays set once it was stopped.
    running: Option<(String, Arc<Notify>, Arc<AtomicBool>)>,
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
        let stopped = Arc::new(AtomicBool::new(false));
        state.running = Some((run_id.to_string(), cancel.clone(), stopped.clone()));
        Ok(Running { slot: self, cancel, stopped })
    }

    /// Stops the run `run_id` if it is the one running (a stop that comes before it first looks
    /// is kept). If it is not running, the stop is remembered for when it begins. Another run is
    /// never touched.
    pub fn cancel(&self, run_id: &str) {
        let mut state = self.state();
        match &state.running {
            Some((id, cancel, stopped)) if id == run_id => {
                stopped.store(true, Ordering::SeqCst);
                cancel.notify_one();
            }
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
        if let Some((_, cancel, stopped)) = &self.state().running {
            stopped.store(true, Ordering::SeqCst);
            cancel.notify_one();
        }
    }
}

/// A task that holds the slot; dropping it frees the slot.
pub struct Running<'a> {
    slot: &'a Slot,
    pub cancel: Arc<Notify>,
    stopped: Arc<AtomicBool>,
}

impl Running<'_> {
    /// Sticky: true from the first cancel on (the Notify permit is single use, so a loop that
    /// waits on `cancel` more than once asks here between its steps).
    pub fn is_cancelled(&self) -> bool {
        self.stopped.load(Ordering::SeqCst)
    }
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
        model::{Account, AiActionMode, AiEffort},
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
    fn a_reset_puts_back_the_defaults_and_removes_every_key_and_consent() {
        let (store, secrets) = rig();
        agreed(&store, &secrets, other(), Some("sk-o".into()));
        agreed(&store, &secrets, AiSettings { allow_ideas: true, allow_chat: true, ..on() }, Some("sk-a".into()));
        chat_consent(&store, &secrets, "a", true).unwrap();
        secrets.set(LEGACY_KEY_NAME.into(), "sk-old".into()).unwrap();
        assert!(load(&store, &secrets, "a").unwrap().chat_consented);

        let after = reset(&store, &secrets, "a").unwrap();
        assert_eq!(after, settings::defaults());
        for name in [OPENAI, ANTHROPIC, LEGACY_KEY_NAME] {
            assert_eq!(key(&secrets, name), None, "{name}");
        }
        // Both services are back to empty, with no consent left on either.
        for provider in [AiProvider::OpenAiCompatible, AiProvider::Anthropic] {
            let s = service(&store, &secrets, "a", provider).unwrap();
            assert!(!s.has_key && !s.consented && !s.chat_consented && s.model.is_empty(), "{provider:?}");
        }
        assert_eq!(store.kv_get("a", SETTINGS_KEY).unwrap(), None);
        // Doing it again changes nothing and is not an error.
        assert_eq!(reset(&store, &secrets, "a").unwrap(), settings::defaults());
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
        let draft = AiTask::DraftReply { message_id: "m".into(), instruction: String::new(), presets: vec![] };
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
        assert!(!running.is_cancelled());
        slot.cancel("a");
        assert!(waits(&rt, &running, 1000));
        // The permit is used up, the flag stays.
        assert!(!waits(&rt, &running, 50));
        assert!(running.is_cancelled());
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
        assert!(running.is_cancelled());
    }

    #[test]
    fn the_chat_needs_both_yeses_and_its_switch() {
        let ready = AiSettings { allow_chat: true, chat_consented: true, ..on() };
        assert!(check_chat(&ready).is_ok());
        let refused = |s: AiSettings| check_chat(&s).unwrap_err().to_string();
        assert!(refused(AiSettings { enabled: false, ..ready.clone() })
            .ends_with("the assistant is switched off in the settings"));
        assert!(refused(AiSettings { consented: false, ..ready.clone() })
            .ends_with("you have not yet agreed to send mail to this service"));
        assert!(refused(AiSettings { allow_chat: false, ..ready.clone() })
            .ends_with("the chat is switched off in the settings"));
        assert!(refused(AiSettings { chat_consented: false, ..ready })
            .ends_with("you have not yet agreed to let the chat read your mail and calendar"));
    }

    /// The yes for the chat, for the service in use.
    fn chat_yes(store: &Store, secrets: &MemorySecrets) -> AiSettings {
        chat_consent(store, secrets, "a", true).unwrap()
    }

    fn said(result: Result<AiSettings>) -> String {
        match result {
            Err(Error::Invalid { msg }) => msg,
            other => panic!("expected Invalid, got {other:?}"),
        }
    }

    #[test]
    fn the_new_switches_are_stored_shared_and_off_until_set() {
        let (store, secrets) = rig();
        let first = agreed(&store, &secrets, on(), None);
        assert!(!first.allow_ideas && !first.allow_extract && !first.allow_explain && !first.allow_chat);
        assert_eq!(first.actions, AiActionMode::AskFirst);
        let wanted = AiSettings {
            allow_ideas: true,
            allow_extract: true,
            allow_explain: true,
            allow_chat: true,
            actions: AiActionMode::Off,
            ..on()
        };
        let saved = save(&store, &secrets, "a", wanted.clone(), None).unwrap();
        assert_eq!(saved, wanted, "switching them on changes nothing else, and keeps the consent");
        assert_eq!(load(&store, &secrets, "a").unwrap(), wanted);
        // Shared by both services, like the stage 1 switches.
        let there = service(&store, &secrets, "a", AiProvider::OpenAiCompatible).unwrap();
        assert!(there.allow_ideas && there.allow_extract && there.allow_explain && there.allow_chat);
        assert_eq!(there.actions, AiActionMode::Off);
        // Each is its own switch.
        let one = save(&store, &secrets, "a", AiSettings { allow_extract: true, ..on() }, None).unwrap();
        assert!(one.allow_extract && !one.allow_ideas && !one.allow_explain && !one.allow_chat);
        assert_eq!(one.actions, AiActionMode::AskFirst);
    }

    #[test]
    fn stored_settings_from_before_the_chat_read_with_everything_new_off() {
        // What the app stored before stage 2: profiles, and none of the new fields.
        let (store, secrets) = rig();
        let old = r#"{"enabled":true,"provider":"Anthropic","allow_summaries":true,"allow_drafts":false,
            "allow_rewrites":true,"openai":{"base_url":"","model":"","effort":"Default","consented":false},
            "anthropic":{"base_url":"https://api.example.com/v1","model":"m","effort":"Default","consented":true}}"#;
        store.kv_set("a", SETTINGS_KEY, old).unwrap();
        let loaded = load(&store, &secrets, "a").unwrap();
        assert_eq!(loaded, AiSettings { allow_drafts: false, ..on() });
        assert!(!loaded.chat_consented && !loaded.allow_chat);
        // The flat stage 1 form, even one that claims the new things, gives none of them.
        let flat = r#"{"enabled":true,"provider":"Anthropic","base_url":"https://api.example.com/v1","model":"m",
            "has_key":false,"consented":true,"allow_summaries":true,"allow_drafts":true,"allow_rewrites":true,
            "allow_chat":true,"chat_consented":true,"allow_ideas":true,"actions":"Off"}"#;
        store.kv_set("a", SETTINGS_KEY, flat).unwrap();
        assert_eq!(load(&store, &secrets, "a").unwrap(), on());
        assert!(check_chat(&load(&store, &secrets, "a").unwrap()).is_err());
    }

    #[test]
    fn the_chat_yes_needs_the_first_yes_and_only_its_own_call_gives_it() {
        let (store, secrets) = rig();
        // Nothing set up, and set up but not agreed: refused with the stage 1 sentence.
        let sentence = "you have not yet agreed to send mail to this service";
        assert_eq!(said(chat_consent(&store, &secrets, "a", true)), sentence);
        assert_eq!(store.kv_get("a", SETTINGS_KEY).unwrap(), None, "a refusal stores nothing");
        save(&store, &secrets, "a", on(), None).unwrap();
        assert_eq!(said(chat_consent(&store, &secrets, "a", true)), sentence);
        // A save that says yes for the chat does not give it, before or after the first yes.
        let claims = AiSettings { chat_consented: true, allow_chat: true, ..on() };
        assert!(!save(&store, &secrets, "a", claims.clone(), None).unwrap().chat_consented);
        assert!(!save(&store, &secrets, "a", claims.clone(), None).unwrap().chat_consented);
        let waiting = load(&store, &secrets, "a").unwrap();
        assert!(waiting.consented && waiting.allow_chat);
        assert_eq!(
            check_chat(&waiting).unwrap_err().to_string().split(": ").last(),
            Some("you have not yet agreed to let the chat read your mail and calendar")
        );
        // Its own call does, and it is what the next load says.
        let given = chat_yes(&store, &secrets);
        assert!(given.chat_consented && given.consented && given.allow_chat);
        assert_eq!(load(&store, &secrets, "a").unwrap(), given);
        assert!(check_chat(&given).is_ok());
        // A save that leaves the service alone keeps it, whatever the caller puts in the field.
        let quiet = AiSettings { chat_consented: false, allow_drafts: false, effort: AiEffort::High, ..claims };
        assert!(save(&store, &secrets, "a", quiet, None).unwrap().chat_consented);
        // Taking it back is always possible, and is no refusal when there is nothing to take.
        assert!(!chat_consent(&store, &secrets, "a", false).unwrap().chat_consented);
        assert!(!chat_consent(&store, &secrets, "a", false).unwrap().chat_consented);
        assert!(load(&store, &secrets, "a").unwrap().consented, "the first yes stays");
    }

    #[test]
    fn the_chat_yes_goes_with_a_new_model_a_new_address_and_a_withdrawn_yes() {
        let changes = [
            AiSettings { model: "m2".into(), ..on() },
            AiSettings { base_url: "https://other.example.com/v1".into(), ..on() },
            AiSettings { consented: false, ..on() },
        ];
        for changed in changes {
            let (store, secrets) = rig();
            agreed(&store, &secrets, on(), None);
            assert!(chat_yes(&store, &secrets).chat_consented);
            // The caller copies the old yes for the chat: it does not count.
            let changed = AiSettings { chat_consented: true, ..changed };
            let after = save(&store, &secrets, "a", changed.clone(), None).unwrap();
            assert!(!after.chat_consented && !after.consented, "{changed:?}");
            // Agreeing to the service again does not bring the chat's yes back...
            let again = save(&store, &secrets, "a", AiSettings { consented: true, ..changed.clone() }, None).unwrap();
            assert!(again.consented && !again.chat_consented, "{changed:?}");
            // ...and neither does going back to the old values.
            assert!(!agreed(&store, &secrets, AiSettings { chat_consented: true, ..on() }, None).chat_consented);
        }
    }

    #[test]
    fn the_chat_yes_belongs_to_one_service_and_the_other_keeps_its_own() {
        let (store, secrets) = rig();
        agreed(&store, &secrets, on(), None);
        chat_yes(&store, &secrets);
        // The other service becomes the one in use: it has no yes for the chat of its own.
        let second = agreed(&store, &secrets, other(), None);
        assert!(second.consented && !second.chat_consented);
        assert!(service(&store, &secrets, "a", AiProvider::Anthropic).unwrap().chat_consented);
        // Giving it here is for this service only; changing this one's model clears only its own.
        assert!(chat_yes(&store, &secrets).chat_consented);
        let moved = save(&store, &secrets, "a", AiSettings { model: "o2".into(), ..other() }, None).unwrap();
        assert!(!moved.chat_consented);
        let first = service(&store, &secrets, "a", AiProvider::Anthropic).unwrap();
        assert!(first.chat_consented && first.consented);
        // Taking it back in the one in use leaves the other alone too.
        agreed(&store, &secrets, other(), None);
        chat_yes(&store, &secrets);
        assert!(save(&store, &secrets, "a", on(), None).unwrap().chat_consented);
        assert!(!chat_consent(&store, &secrets, "a", false).unwrap().chat_consented);
        assert!(service(&store, &secrets, "a", AiProvider::OpenAiCompatible).unwrap().chat_consented);
    }

    #[test]
    fn every_refusal_of_a_task_has_its_exact_sentence() {
        let all = AiSettings { allow_ideas: true, allow_extract: true, allow_explain: true, ..on() };
        let day = || ("2026-11-15".to_string(), "UTC".to_string());
        let tasks = [
            (AiTask::SummarizeMessage { message_id: "m".into() }, "summaries are switched off in the settings"),
            (AiTask::SummarizeThread { thread_id: "t".into() }, "summaries are switched off in the settings"),
            (
                AiTask::DraftReply { message_id: "m".into(), instruction: String::new(), presets: vec![] },
                "drafting replies is switched off in the settings",
            ),
            (
                AiTask::Rewrite { text: "x".into(), style: crate::model::RewriteStyle::Improve },
                "rewriting is switched off in the settings",
            ),
            (AiTask::ReplyIdeas { message_id: "m".into() }, "reply ideas are switched off in the settings"),
            (
                AiTask::Extract { message_id: "m".into(), today: day().0, time_zone: day().1 },
                "finding to-dos and dates is switched off in the settings",
            ),
            (
                AiTask::ExtractThread { thread_id: "t".into(), today: day().0, time_zone: day().1 },
                "finding to-dos and dates is switched off in the settings",
            ),
            (
                AiTask::Explain { message_id: Some("m".into()), text: String::new() },
                "explaining is switched off in the settings",
            ),
            (AiTask::Explain { message_id: None, text: "x".into() }, "explaining is switched off in the settings"),
        ];
        let refused = |s: &AiSettings, t: &AiTask| match check_allowed(s, t) {
            Err(Error::Invalid { msg }) => msg,
            other => panic!("expected Invalid, got {other:?}"),
        };
        for (task, off) in &tasks {
            assert!(check_allowed(&all, task).is_ok(), "{task:?}");
            // Off and not agreed come first, in that order, for every task.
            let dark = AiSettings { enabled: false, consented: false, ..all.clone() };
            assert_eq!(refused(&dark, task), "the assistant is switched off in the settings");
            let unasked = AiSettings { consented: false, ..all.clone() };
            assert_eq!(refused(&unasked, task), "you have not yet agreed to send mail to this service");
            let none = AiSettings {
                allow_summaries: false,
                allow_drafts: false,
                allow_rewrites: false,
                // The chat's switch and its yes open no task.
                allow_chat: true,
                chat_consented: true,
                ..on()
            };
            assert_eq!(refused(&none, task), *off, "{task:?}");
        }
    }

    #[test]
    fn the_chat_is_refused_in_the_order_of_its_four_gates() {
        let refused = |s: &AiSettings| match check_chat(s) {
            Err(Error::Invalid { msg }) => msg,
            other => panic!("expected Invalid, got {other:?}"),
        };
        let nothing = AiSettings { enabled: false, consented: false, ..on() };
        assert_eq!(refused(&nothing), "the assistant is switched off in the settings");
        let enabled = AiSettings { enabled: true, ..nothing };
        assert_eq!(refused(&enabled), "you have not yet agreed to send mail to this service");
        let agreed = AiSettings { consented: true, ..enabled };
        assert_eq!(refused(&agreed), "the chat is switched off in the settings");
        let switched = AiSettings { allow_chat: true, ..agreed };
        assert_eq!(refused(&switched), "you have not yet agreed to let the chat read your mail and calendar");
        // The stage 2 switches and the action mode are not gates of the chat.
        let ready = AiSettings { chat_consented: true, actions: AiActionMode::Off, ..switched };
        assert!(check_chat(&ready).is_ok());
    }

    #[test]
    fn the_stage_two_tasks_are_off_until_switched_on() {
        let day = || ("2026-11-15".to_string(), "UTC".to_string());
        let ideas = AiTask::ReplyIdeas { message_id: "m".into() };
        let extract = AiTask::Extract { message_id: "m".into(), today: day().0, time_zone: day().1 };
        let thread = AiTask::ExtractThread { thread_id: "t".into(), today: day().0, time_zone: day().1 };
        let explain = AiTask::Explain { message_id: None, text: "x".into() };
        let refused = |t: &AiTask| check_allowed(&on(), t).unwrap_err().to_string();
        assert!(refused(&ideas).ends_with("reply ideas are switched off in the settings"));
        assert!(refused(&extract).ends_with("finding to-dos and dates is switched off in the settings"));
        assert!(refused(&thread).ends_with("finding to-dos and dates is switched off in the settings"));
        assert!(refused(&explain).ends_with("explaining is switched off in the settings"));
        let all = AiSettings { allow_ideas: true, allow_extract: true, allow_explain: true, ..on() };
        assert!([&ideas, &extract, &thread, &explain].iter().all(|t| check_allowed(&all, t).is_ok()));
        // The stage 1 switches do not open them, and they do not close the stage 1 tasks.
        assert!(check_allowed(&all, &AiTask::SummarizeMessage { message_id: "m".into() }).is_ok());
    }
}
