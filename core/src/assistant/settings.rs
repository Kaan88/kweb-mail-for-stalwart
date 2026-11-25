//! What the user may set for the assistant, and what a change does to the consent. Pure: the
//! storage (the account's key/value table, and the secret store for the keys) is in `mod.rs`.
//!
//! The user keeps a list of services. Every one speaks the OpenAI chat-completions protocol and
//! keeps its own address, model, effort, key and consent; one of them is the service in use.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use url::{Host, Url};

use crate::{
    model::{AiActionMode, AiEffort, AiModelList, AiService, AiSettings, AI_INSTRUCTIONS_MAX},
    Error, Result,
};

/// The account's key/value table holds the settings as JSON under this key (never an API key).
pub const SETTINGS_KEY: &str = "ai_settings";
/// Where stage 1 kept its one key in the secret store. Moved to the service it belonged to the
/// first time the settings are read, and then removed.
pub const LEGACY_KEY_NAME: &str = "ai_key";
/// Where round 2 kept the key of its Anthropic service. That service is gone: the key is removed
/// the first time the older settings are read.
pub const ANTHROPIC_KEY_NAME: &str = "ai_key_anthropic";
/// The id of the service that older settings had (the one OpenAI-compatible service). Its key
/// keeps the name it had (`ai_key_openai`), so an update needs no new key.
pub const FIRST_ID: &str = "openai";
/// The most services one account keeps. New ones are `s1` to `s20`.
pub const MAX_SERVICES: usize = 20;
/// The longest name of a service (characters).
pub const NAME_MAX: usize = 60;

/// Every API key lives in the host's secret store under the name of its own service.
pub fn key_name(id: &str) -> String {
    format!("ai_key_{id}")
}

/// Every name a key of the assistant can have had: each service id there can be, and the names of
/// before. Sign-out and the reset remove all of them, without reading the settings.
pub fn all_key_names() -> Vec<String> {
    let mut names: Vec<String> = std::iter::once(FIRST_ID.to_string())
        .chain((1..=MAX_SERVICES).map(|n| format!("s{n}")))
        .map(|id| key_name(&id))
        .collect();
    names.push(ANTHROPIC_KEY_NAME.into());
    names.push(LEGACY_KEY_NAME.into());
    names
}

/// What one service keeps for itself.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Profile {
    /// [`FIRST_ID`] or `s1` to `s20`, made by the core; never empty once stored.
    pub id: String,
    pub name: String,
    pub base_url: String,
    pub model: String,
    pub effort: AiEffort,
    pub consented: bool,
    /// The second yes, for the chat: it sends what it looks up to this service and model. Never
    /// true without `consented`.
    pub chat_consented: bool,
    /// The chat found that this address and model take no tools. Kept only while both stay the same,
    /// and cleared when a later chat turn used tools.
    pub tools_unsupported: bool,
    /// The effort levels the service listed, from the last list of models it gave. `effort` is
    /// sent only when these say the model at this address takes it.
    pub levels: Levels,
}

/// The most models whose levels are kept.
const MAX_LEVELS: usize = 2000;

/// The effort levels a service named for its models, kept from the last list it gave for one
/// address. Nothing else of the list is kept, and only models that named a level are in here.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Levels {
    /// The address the list came from, as it was asked (trimmed, no trailing slash).
    pub base_url: String,
    /// Model id to its levels, lowest first, without `Default`.
    pub models: BTreeMap<String, Vec<AiEffort>>,
}

impl Levels {
    /// The levels of `list`, which `base_url` gave.
    pub fn of(base_url: &str, list: &AiModelList) -> Levels {
        let models = list
            .models
            .iter()
            .filter(|m| !m.efforts.is_empty())
            .take(MAX_LEVELS)
            .map(|m| (m.id.clone(), m.efforts.iter().copied().filter(|e| *e != AiEffort::Default).collect()))
            .collect();
        Levels { base_url: base_url.trim().trim_end_matches('/').to_string(), models }
    }

    /// Whether `effort` may be sent to `model` at `base_url`: `Default` always (it is not sent),
    /// anything else only when the service listed it for exactly that address and model.
    pub fn allows(&self, base_url: &str, model: &str, effort: AiEffort) -> bool {
        effort == AiEffort::Default
            || (self.base_url == base_url.trim().trim_end_matches('/')
                && self.models.get(model.trim()).is_some_and(|levels| levels.contains(&effort)))
    }
}

/// What is stored under [`SETTINGS_KEY`]: the shared switches, the service in use, and the list of
/// services. No key is in here.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Stored {
    pub enabled: bool,
    /// The id of the service in use; empty when there is none.
    pub active: String,
    pub allow_summaries: bool,
    pub allow_drafts: bool,
    pub allow_rewrites: bool,
    pub allow_ideas: bool,
    pub allow_extract: bool,
    pub allow_explain: bool,
    pub allow_chat: bool,
    pub actions: AiActionMode,
    /// The user's own instructions (see `AiSettings::instructions`).
    pub instructions: String,
    /// In the order they were added.
    pub services: Vec<Profile>,
}

impl Default for Stored {
    fn default() -> Self {
        Stored {
            enabled: false,
            active: String::new(),
            allow_summaries: true,
            allow_drafts: true,
            allow_rewrites: true,
            allow_ideas: false,
            allow_extract: false,
            allow_explain: false,
            allow_chat: false,
            actions: AiActionMode::default(),
            instructions: String::new(),
            services: Vec::new(),
        }
    }
}

impl Stored {
    pub fn profile(&self, id: &str) -> Option<&Profile> {
        self.services.iter().find(|p| !id.is_empty() && p.id == id)
    }

    pub fn profile_mut(&mut self, id: &str) -> Option<&mut Profile> {
        self.services.iter_mut().find(|p| !id.is_empty() && p.id == id)
    }

    /// The id for a new service: the lowest of `s1` to `s20` no service has. `None` when the list is full.
    pub fn new_id(&self) -> Option<String> {
        if self.services.len() >= MAX_SERVICES {
            return None;
        }
        (1..=MAX_SERVICES).map(|n| format!("s{n}")).find(|id| self.profile(id).is_none())
    }

    /// The settings with the service `id` as the one shown (the active one, or one the screen asks
    /// for); an empty or unknown id shows a new, empty service. Every `has_key` is left false: the
    /// secret store says whether there is a key.
    pub fn view(&self, id: &str) -> AiSettings {
        let blank = Profile::default();
        let p = self.profile(id).unwrap_or(&blank);
        // A level the service never named for this address and model is not sent, so it is not shown.
        let effort = if p.levels.allows(&p.base_url, &p.model, p.effort) { p.effort } else { AiEffort::Default };
        AiSettings {
            enabled: self.enabled,
            service_id: p.id.clone(),
            service_name: p.name.clone(),
            base_url: p.base_url.clone(),
            model: p.model.clone(),
            effort,
            has_key: false,
            consented: p.consented,
            allow_summaries: self.allow_summaries,
            allow_drafts: self.allow_drafts,
            allow_rewrites: self.allow_rewrites,
            allow_ideas: self.allow_ideas,
            allow_extract: self.allow_extract,
            allow_explain: self.allow_explain,
            allow_chat: self.allow_chat,
            chat_consented: p.consented && p.chat_consented,
            actions: self.actions,
            tools_unsupported: p.tools_unsupported,
            instructions: self.instructions.clone(),
            services: self
                .services
                .iter()
                .map(|s| AiService {
                    id: s.id.clone(),
                    name: s.name.clone(),
                    base_url: s.base_url.clone(),
                    model: s.model.clone(),
                    has_key: false,
                    consented: s.consented,
                    active: s.id == self.active,
                })
                .collect(),
        }
    }
}

/// What reading older settings means for the keys in the secret store.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Moved {
    /// The text was in an older form: write it again in the new one, and remove the Anthropic key.
    pub old_form: bool,
    /// Stage 1's one key (`ai_key`) belongs to the service [`FIRST_ID`]; otherwise it is removed.
    pub legacy_key_is_first: bool,
}

/// The flat settings of stage 1: one service, the provider by name.
#[derive(Deserialize)]
#[serde(default)]
struct Flat {
    enabled: bool,
    provider: String,
    base_url: String,
    model: String,
    effort: AiEffort,
    consented: bool,
    allow_summaries: bool,
    allow_drafts: bool,
    allow_rewrites: bool,
}

impl Default for Flat {
    fn default() -> Self {
        Flat {
            enabled: false,
            provider: String::new(),
            base_url: String::new(),
            model: String::new(),
            effort: AiEffort::Default,
            consented: false,
            allow_summaries: true,
            allow_drafts: true,
            allow_rewrites: true,
        }
    }
}

/// The settings of round 2: the shared switches and one profile per kind of service.
#[derive(Deserialize)]
#[serde(default)]
struct Pair {
    enabled: bool,
    provider: String,
    allow_summaries: bool,
    allow_drafts: bool,
    allow_rewrites: bool,
    allow_ideas: bool,
    allow_extract: bool,
    allow_explain: bool,
    allow_chat: bool,
    actions: AiActionMode,
    instructions: String,
    openai: Profile,
}

impl Default for Pair {
    fn default() -> Self {
        let d = Stored::default();
        Pair {
            enabled: d.enabled,
            provider: String::new(),
            allow_summaries: d.allow_summaries,
            allow_drafts: d.allow_drafts,
            allow_rewrites: d.allow_rewrites,
            allow_ideas: d.allow_ideas,
            allow_extract: d.allow_extract,
            allow_explain: d.allow_explain,
            allow_chat: d.allow_chat,
            actions: d.actions,
            instructions: d.instructions,
            openai: Profile::default(),
        }
    }
}

/// The name older settings used for the OpenAI-compatible service.
const OPENAI_PROVIDER: &str = "OpenAiCompatible";

/// The one service older settings had becomes the first of the list, with its consent. An empty
/// one (no address) is no service.
fn first_of(mut p: Profile) -> Vec<Profile> {
    if p.base_url.trim().is_empty() {
        return Vec::new();
    }
    p.id = FIRST_ID.into();
    p.name = default_name(&p.base_url);
    p.chat_consented &= p.consented;
    vec![p]
}

/// Reads a stored value, and says what the form it was in means for the keys. Two older forms
/// become the new one: stage 1's flat settings of one service, and round 2's two profiles (one
/// OpenAI-compatible, one Anthropic). The OpenAI-compatible service becomes the first of the list
/// and keeps its consent; the Anthropic one is dropped. When the Anthropic service was the one in
/// use, the assistant is switched off, so nothing goes to the other service until the user turns
/// it on again. `None` when the text cannot be read.
pub fn parse(json: &str) -> Option<(Stored, Moved)> {
    let value: serde_json::Value = serde_json::from_str(json).ok()?;
    if !value.is_object() {
        return None;
    }
    if value.get("services").is_some() {
        let mut stored: Stored = serde_json::from_value(value).ok()?;
        stored.services.retain(|p| !p.id.is_empty());
        if stored.profile(&stored.active).is_none() {
            stored.active.clear();
        }
        return Some((stored, Moved::default()));
    }
    if value.get("base_url").is_some() {
        let old: Flat = serde_json::from_value(value).ok()?;
        let openai = old.provider == OPENAI_PROVIDER;
        let services = if openai {
            first_of(Profile {
                base_url: old.base_url,
                model: old.model,
                effort: old.effort,
                consented: old.consented,
                ..Profile::default()
            })
        } else {
            Vec::new()
        };
        let stored = Stored {
            enabled: old.enabled && openai,
            active: services.first().map(|p| p.id.clone()).unwrap_or_default(),
            allow_summaries: old.allow_summaries,
            allow_drafts: old.allow_drafts,
            allow_rewrites: old.allow_rewrites,
            services,
            ..Stored::default()
        };
        let first = !stored.services.is_empty();
        return Some((stored, Moved { old_form: true, legacy_key_is_first: first }));
    }
    let old: Pair = serde_json::from_value(value).ok()?;
    let openai = old.provider.is_empty() || old.provider == OPENAI_PROVIDER;
    let services = first_of(old.openai);
    let stored = Stored {
        enabled: old.enabled && openai,
        active: services.first().map(|p| p.id.clone()).unwrap_or_default(),
        allow_summaries: old.allow_summaries,
        allow_drafts: old.allow_drafts,
        allow_rewrites: old.allow_rewrites,
        allow_ideas: old.allow_ideas,
        allow_extract: old.allow_extract,
        allow_explain: old.allow_explain,
        allow_chat: old.allow_chat,
        actions: old.actions,
        instructions: old.instructions,
        services,
    };
    Some((stored, Moved { old_form: true, legacy_key_is_first: false }))
}

/// The name of a service the user did not name: the well-known services by their name, any other
/// by its host (and port).
pub fn default_name(base_url: &str) -> String {
    const KNOWN: [(&str, &str); 6] = [
        ("api.openai.com", "OpenAI"),
        ("openrouter.ai", "OpenRouter"),
        ("generativelanguage.googleapis.com", "Google Gemini"),
        ("api.mistral.ai", "Mistral"),
        ("api.deepseek.com", "DeepSeek"),
        ("api.groq.com", "Groq"),
    ];
    let Ok(url) = Url::parse(base_url.trim()) else { return "Service".into() };
    let host = url.host_str().unwrap_or_default().trim_end_matches('.').to_ascii_lowercase();
    match KNOWN.iter().find(|(known, _)| *known == host) {
        Some((_, name)) => name.to_string(),
        None if host.is_empty() => "Service".into(),
        None => host_label(&url),
    }
}

/// A name as it is stored: one line, surrounding blanks dropped. Longer than [`NAME_MAX`]
/// characters is refused.
pub fn clean_name(name: &str) -> Result<String> {
    let line: String = name.chars().map(|c| if c.is_control() { ' ' } else { c }).collect();
    let line = line.split_whitespace().collect::<Vec<_>>().join(" ");
    if line.chars().count() > NAME_MAX {
        return Err(Error::invalid(format!("the name is longer than {NAME_MAX} characters")));
    }
    Ok(line)
}

/// The instructions as they are stored: line ends made \n, surrounding blanks dropped, nothing else changed.
/// Longer than [`AI_INSTRUCTIONS_MAX`] characters is refused (the text is not quoted in the error).
pub fn clean_instructions(text: &str) -> Result<String> {
    let text = text.replace("\r\n", "\n").replace('\r', "\n");
    let text = text.trim();
    if text.chars().count() > AI_INSTRUCTIONS_MAX {
        return Err(Error::invalid(format!("the instructions are longer than {AI_INSTRUCTIONS_MAX} characters")));
    }
    Ok(text.to_string())
}

/// How the assistant is set up before the user has done anything: off, no service, nothing agreed to.
pub fn defaults() -> AiSettings {
    Stored::default().view("")
}

/// The settings as the user gave them, made fit to store. `has_key` is left false: the secret
/// store, not the settings, says whether there is a key.
///
/// `consented` is taken as given here; [`consent_after`] decides what survives a change.
pub fn normalise(mut s: AiSettings) -> Result<AiSettings> {
    s.base_url = s.base_url.trim().trim_end_matches('/').to_string();
    s.model = s.model.trim().to_string();
    s.service_id = s.service_id.trim().to_string();
    s.service_name = clean_name(&s.service_name)?;
    s.has_key = false;
    s.tools_unsupported = false;
    s.services.clear();
    s.instructions = clean_instructions(&s.instructions)?;
    if s.base_url.is_empty() {
        if s.enabled {
            return Err(Error::invalid("enter the address of the service first"));
        }
    } else {
        check_base_url(&s.base_url)?;
    }
    if s.enabled && s.model.is_empty() {
        return Err(Error::invalid("enter the name of the model first"));
    }
    Ok(s)
}

/// The consent to store for a service: a yes survives only while its address and model are the
/// ones stored for it. The core cannot tell a caller that asked again from one that copied the
/// old value, so a change of either always clears it, and the host grants it in a second call
/// that repeats the (now stored) values, right after it showed what goes where.
pub fn consent_after(before: &Profile, new: &AiSettings) -> bool {
    new.consented
        && !new.base_url.is_empty()
        && !new.model.is_empty()
        && (&before.base_url, &before.model) == (&new.base_url, &new.model)
}

/// Whether a key entered for the address `before` may be sent to `new`: the same scheme, host
/// and port (a different path on the same host keeps it).
pub fn same_origin(before: &str, new: &str) -> bool {
    let origin = |s: &str| Url::parse(s).ok().map(|u| u.origin());
    origin(before) == origin(new)
}

/// An address the assistant may talk to: https, or plain http to this device or a private
/// network (a model on the user's own machine), with no name, password, query or fragment.
pub fn check_base_url(raw: &str) -> Result<Url> {
    let url = Url::parse(raw).map_err(|_| Error::invalid("the address of the service is not a valid web address"))?;
    let host = url.host().ok_or_else(|| Error::invalid("the address of the service has no host"))?;
    match url.scheme() {
        "https" => {}
        "http" if is_private(&host) => {}
        "http" => {
            return Err(Error::invalid(
                "the address must start with https:// (plain http is only allowed to this device or a private network)",
            ))
        }
        _ => return Err(Error::invalid("the address must start with https://")),
    }
    if !url.username().is_empty() || url.password().is_some() || url.query().is_some() || url.fragment().is_some() {
        return Err(Error::invalid("the address must not contain a name, password, query or fragment"));
    }
    Ok(url)
}

/// This device or a private network: 127/8, 10/8, 172.16/12, 192.168/16, `::1`, `fc00::/7`,
/// `fe80::/10`, `localhost` and names ending in `.local`.
pub(crate) fn is_private(host: &Host<&str>) -> bool {
    match host {
        Host::Domain(name) => {
            let name = name.trim_end_matches('.').to_ascii_lowercase();
            name == "localhost" || name.ends_with(".localhost") || name.ends_with(".local")
        }
        Host::Ipv4(a) => a.is_loopback() || a.is_private(),
        Host::Ipv6(a) => {
            let first = a.segments()[0];
            a.is_loopback()
                || first & 0xfe00 == 0xfc00
                || first & 0xffc0 == 0xfe80
                || a.to_ipv4_mapped().is_some_and(|v4| v4.is_loopback() || v4.is_private())
        }
    }
}

/// `host` or `host:port`, for the user to read in the confirmation.
pub fn host_label(url: &Url) -> String {
    let host = url.host_str().unwrap_or_default();
    match url.port() {
        Some(port) => format!("{host}:{port}"),
        None => host.to_string(),
    }
}

/// A key goes into a header: visible ASCII only. The message never repeats the key.
pub fn check_key(key: &str) -> Result<()> {
    if key.chars().all(|c| c.is_ascii_graphic()) {
        Ok(())
    } else {
        Err(Error::invalid("the key has characters that cannot be part of a key"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn on(url: &str, model: &str) -> AiSettings {
        AiSettings { enabled: true, base_url: url.into(), model: model.into(), ..defaults() }
    }

    fn ok(url: &str) -> bool {
        check_base_url(url).is_ok()
    }

    #[test]
    fn https_is_allowed_anywhere_and_http_only_to_this_device_or_a_private_network() {
        assert!(ok("https://api.openai.com/v1"));
        assert!(ok("https://203.0.113.9/v1"));
        for local in [
            "http://localhost:11434/v1",
            "http://LOCALHOST/v1",
            "http://127.0.0.1:1234/v1",
            "http://127.9.9.9/v1",
            "http://10.1.2.3/v1",
            "http://172.16.0.1/v1",
            "http://172.31.255.255/v1",
            "http://192.168.1.20:11434/v1",
            "http://[::1]:8080/v1",
            "http://[fc00::1]/v1",
            "http://[fd12:3456::9]/v1",
            "http://[fe80::1]/v1",
            "http://[::ffff:192.168.1.5]/v1",
            "http://box.local/v1",
            "http://llm.localhost/v1",
        ] {
            assert!(ok(local), "{local} should be allowed");
        }
        for public in [
            "http://api.openai.com/v1",
            "http://8.8.8.8/v1",
            "http://172.32.0.1/v1",
            "http://172.15.0.1/v1",
            "http://192.169.0.1/v1",
            "http://11.0.0.1/v1",
            "http://[2001:db8::1]/v1",
            "http://[fec0::1]/v1",
            "http://localhost.example.com/v1",
            "http://notlocal/v1",
        ] {
            assert!(!ok(public), "{public} should be refused");
        }
    }

    #[test]
    fn other_shapes_of_address_are_refused() {
        for bad in [
            "",
            "api.openai.com/v1",
            "ftp://localhost/v1",
            "file:///etc/passwd",
            "https://",
            "https://user:pw@api.example.com/v1",
            "https://user@api.example.com/v1",
            "https://api.example.com/v1?key=1",
            "https://api.example.com/v1#x",
        ] {
            assert!(matches!(check_base_url(bad), Err(Error::Invalid { .. })), "{bad:?} should be refused");
        }
    }

    #[test]
    fn normalise_trims_and_drops_trailing_slashes() {
        let s = normalise(AiSettings {
            service_name: "  My \n box ".into(),
            ..on("  https://api.example.com/v1//  ", "  gpt  ")
        })
        .unwrap();
        assert_eq!((s.base_url.as_str(), s.model.as_str()), ("https://api.example.com/v1", "gpt"));
        assert_eq!(s.service_name, "My box");
        let long = AiSettings { service_name: "x".repeat(NAME_MAX + 1), ..on("https://a.example/v1", "m") };
        assert!(matches!(normalise(long), Err(Error::Invalid { .. })));
    }

    #[test]
    fn enabled_needs_an_address_and_a_model_but_off_may_be_empty() {
        assert!(matches!(normalise(on("", "m")), Err(Error::Invalid { .. })));
        assert!(matches!(normalise(on("https://a.example/v1", "  ")), Err(Error::Invalid { .. })));
        assert_eq!(normalise(defaults()).unwrap(), defaults());
        // An address that is there is checked even while the assistant is off.
        let bad = AiSettings { base_url: "http://api.example.com".into(), ..defaults() };
        assert!(matches!(normalise(bad), Err(Error::Invalid { .. })));
    }

    #[test]
    fn consent_survives_only_an_unchanged_service() {
        let agreed = AiSettings { consented: true, ..on("https://a.example/v1", "m1") };
        let stored = Profile { base_url: agreed.base_url.clone(), model: "m1".into(), ..Profile::default() };
        assert!(consent_after(&stored, &agreed));
        assert!(!consent_after(&stored, &AiSettings { consented: false, ..agreed.clone() }));
        // Never given by a call that changes either, even when it says yes.
        assert!(!consent_after(&stored, &AiSettings { model: "m2".into(), ..agreed.clone() }));
        assert!(!consent_after(&stored, &AiSettings { base_url: "https://b.example/v1".into(), ..agreed.clone() }));
        // A first setup (nothing stored) never starts agreed, and an empty service is never agreed.
        assert!(!consent_after(&Profile::default(), &agreed));
        let empty = AiSettings { consented: true, ..defaults() };
        assert!(!consent_after(&Profile::default(), &empty));
    }

    #[test]
    fn a_key_stays_with_its_origin() {
        let before = "https://a.example/v1";
        assert!(same_origin(before, "https://a.example/v2/other"));
        for moved in ["https://b.example/v1", "http://a.example/v1", "https://a.example:8443/v1"] {
            assert!(!same_origin(before, moved), "{moved}");
        }
        assert!(!same_origin("", before));
        assert!(same_origin("", ""));
    }

    #[test]
    fn nothing_set_up_is_off_with_no_service() {
        let d = defaults();
        assert!(!d.enabled && d.service_id.is_empty() && d.base_url.is_empty() && d.services.is_empty());
        assert!(!d.allow_ideas && !d.allow_extract && !d.allow_explain && !d.allow_chat && !d.chat_consented);
        assert_eq!(d.actions, AiActionMode::AskFirst);
    }

    #[test]
    fn new_ids_are_the_lowest_free_ones_up_to_twenty() {
        let mut s = Stored::default();
        assert_eq!(s.new_id().as_deref(), Some("s1"));
        s.services.push(Profile { id: FIRST_ID.into(), ..Profile::default() });
        s.services.push(Profile { id: "s1".into(), ..Profile::default() });
        s.services.push(Profile { id: "s3".into(), ..Profile::default() });
        assert_eq!(s.new_id().as_deref(), Some("s2"));
        while let Some(id) = s.new_id() {
            s.services.push(Profile { id, ..Profile::default() });
        }
        assert_eq!(s.services.len(), MAX_SERVICES);
        // Every id there can be has its key's name in the list sign-out removes.
        let names = all_key_names();
        assert!(s.services.iter().all(|p| names.contains(&key_name(&p.id))));
        assert!(names.contains(&key_name("s20")) && names.contains(&"ai_key_anthropic".to_string()));
        assert!(s.profile("").is_none());
    }

    #[test]
    fn the_view_shows_one_service_and_lists_all() {
        let stored = Stored {
            enabled: true,
            active: "s1".into(),
            services: vec![
                Profile {
                    id: FIRST_ID.into(),
                    name: "OpenAI".into(),
                    base_url: "https://api.openai.com/v1".into(),
                    model: "a".into(),
                    consented: true,
                    ..Profile::default()
                },
                Profile {
                    id: "s1".into(),
                    name: "Box".into(),
                    base_url: "http://10.0.0.2:11434/v1".into(),
                    model: "b".into(),
                    ..Profile::default()
                },
            ],
            ..Stored::default()
        };
        let view = stored.view(&stored.active);
        assert_eq!((view.service_id.as_str(), view.service_name.as_str(), view.model.as_str()), ("s1", "Box", "b"));
        assert!(!view.consented);
        assert_eq!(
            view.services.iter().map(|s| (s.id.as_str(), s.active)).collect::<Vec<_>>(),
            [("openai", false), ("s1", true)]
        );
        let first = stored.view(FIRST_ID);
        assert!(first.consented && first.service_name == "OpenAI");
        // An unknown id is a new, empty service with the shared switches.
        let fresh = stored.view("s9");
        assert!(fresh.enabled && fresh.service_id.is_empty() && fresh.base_url.is_empty() && !fresh.consented);
    }

    #[test]
    fn stage_one_openai_settings_become_the_first_service_with_their_consent() {
        let flat = r#"{"enabled":true,"provider":"OpenAiCompatible","base_url":"https://a.example/v1","model":"m",
            "has_key":true,"consented":true,"allow_summaries":false,"allow_drafts":true,"allow_rewrites":true,
            "allow_ideas":true,"allow_extract":true,"allow_explain":true,"allow_chat":true,"chat_consented":true,
            "actions":"Off"}"#;
        let (stored, moved) = parse(flat).unwrap();
        assert_eq!(moved, Moved { old_form: true, legacy_key_is_first: true });
        // Stage 1 never wrote the newer switches, so they are not believed.
        assert!(!stored.allow_ideas && !stored.allow_extract && !stored.allow_explain && !stored.allow_chat);
        assert_eq!(stored.actions, AiActionMode::AskFirst);
        let view = stored.view(&stored.active);
        assert!(view.enabled && view.consented && !view.chat_consented && !view.allow_summaries);
        assert_eq!((view.service_id.as_str(), view.service_name.as_str()), (FIRST_ID, "a.example"));
        // The new form round-trips and says nothing about old keys.
        let json = serde_json::to_string(&stored).unwrap();
        assert_eq!(parse(&json), Some((stored, Moved::default())));
        assert_eq!(parse("{not json"), None);
        assert_eq!(parse("[]"), None);
    }

    #[test]
    fn stage_one_anthropic_settings_leave_no_service_and_the_assistant_off() {
        let flat = r#"{"enabled":true,"provider":"Anthropic","base_url":"https://a.example/v1","model":"m",
            "has_key":false,"consented":true,"allow_summaries":true,"allow_drafts":false,"allow_rewrites":true}"#;
        let (stored, moved) = parse(flat).unwrap();
        assert_eq!(moved, Moved { old_form: true, legacy_key_is_first: false });
        assert!(stored.services.is_empty() && stored.active.is_empty() && !stored.enabled && !stored.allow_drafts);
    }

    #[test]
    fn round_two_settings_keep_the_openai_service_and_drop_anthropic() {
        let old = r#"{"enabled":true,"provider":"OpenAiCompatible","allow_summaries":true,"allow_drafts":false,
            "allow_rewrites":true,"allow_chat":true,"instructions":"Be brief.","openai":{"base_url":"http://localhost:1/v1",
            "model":"o","effort":"High","consented":true,"chat_consented":true,"levels":{"base_url":"http://localhost:1/v1",
            "models":{"o":["High"]}}},"anthropic":{"base_url":"https://a.example/v1","model":"m","effort":"Default",
            "consented":true}}"#;
        let (stored, moved) = parse(old).unwrap();
        assert_eq!(moved, Moved { old_form: true, legacy_key_is_first: false });
        assert_eq!(stored.services.len(), 1);
        let view = stored.view(&stored.active);
        assert_eq!((view.service_id.as_str(), view.service_name.as_str()), (FIRST_ID, "localhost:1"));
        assert_eq!(
            (view.base_url.as_str(), view.model.as_str(), view.effort),
            ("http://localhost:1/v1", "o", AiEffort::High)
        );
        assert!(view.enabled && view.consented && view.chat_consented && view.allow_chat && !view.allow_drafts);
        assert_eq!(view.instructions, "Be brief.");

        // With Anthropic in use the other service stays, with its consent, and the assistant is off.
        let (stored, _) = parse(&old.replace(r#""provider":"OpenAiCompatible""#, r#""provider":"Anthropic""#)).unwrap();
        assert!(!stored.enabled && stored.view(&stored.active).consented && stored.active == FIRST_ID);
        // An empty OpenAI-compatible profile is no service.
        let (stored, _) = parse(
            r#"{"enabled":false,"provider":"Anthropic","openai":{},"anthropic":{"base_url":"https://x.example/v1"}}"#,
        )
        .unwrap();
        assert!(stored.services.is_empty() && stored.active.is_empty());
        // A stored chat yes without the first one is never taken.
        let odd = r#"{"provider":"OpenAiCompatible","openai":{"base_url":"https://a.example/v1","model":"m","consented":false,"chat_consented":true}}"#;
        let (stored, _) = parse(odd).unwrap();
        assert!(!stored.services[0].chat_consented);
    }

    #[test]
    fn the_new_form_round_trips_and_drops_what_does_not_fit() {
        let stored = Stored {
            enabled: true,
            active: "s2".into(),
            allow_ideas: true,
            allow_chat: true,
            actions: AiActionMode::Off,
            services: vec![Profile {
                id: "s2".into(),
                name: "Mine".into(),
                base_url: "https://a.example/v1".into(),
                model: "m".into(),
                effort: AiEffort::Max,
                consented: true,
                chat_consented: true,
                tools_unsupported: false,
                levels: Levels {
                    base_url: "https://a.example/v1".into(),
                    models: [("m".to_string(), vec![AiEffort::Low, AiEffort::Max])].into(),
                },
            }],
            ..Stored::default()
        };
        let json = serde_json::to_string(&stored).unwrap();
        assert_eq!(parse(&json), Some((stored.clone(), Moved::default())));
        let view = stored.view("s2");
        assert!(view.allow_ideas && view.allow_chat && view.chat_consented && view.effort == AiEffort::Max);
        // A service without an id is dropped, and an active id that names none is no service in use.
        let odd = r#"{"active":"s5","services":[{"name":"x"},{"id":"s1","base_url":"https://b.example/v1"}]}"#;
        let (stored, _) = parse(odd).unwrap();
        assert_eq!(stored.services.len(), 1);
        assert!(stored.active.is_empty());
    }

    #[test]
    fn a_service_without_a_name_is_named_after_its_address() {
        assert_eq!(default_name("https://api.openai.com/v1"), "OpenAI");
        assert_eq!(default_name("https://openrouter.ai/api/v1"), "OpenRouter");
        assert_eq!(default_name("https://generativelanguage.googleapis.com/v1beta/openai"), "Google Gemini");
        assert_eq!(default_name("https://API.Mistral.ai/v1"), "Mistral");
        assert_eq!(default_name("http://192.168.1.20:11434/v1"), "192.168.1.20:11434");
        assert_eq!(default_name(""), "Service");
    }

    #[test]
    fn an_effort_is_shown_and_sent_only_where_the_service_listed_it() {
        use crate::model::AiModel;
        let model =
            |id: &str, efforts: &[AiEffort]| AiModel { id: id.into(), name: id.into(), efforts: efforts.to_vec() };
        let list = AiModelList { models: vec![model("a", &[AiEffort::Low, AiEffort::High]), model("plain", &[])] };
        let levels = Levels::of("https://r.example/v1/ ", &list);
        assert_eq!(levels.base_url, "https://r.example/v1");
        assert_eq!(levels.models.keys().collect::<Vec<_>>(), ["a"], "a model without levels is not kept");
        assert!(levels.allows("https://r.example/v1", "a", AiEffort::High));
        assert!(levels.allows("https://r.example/v1/", " a ", AiEffort::Low));
        assert!(!levels.allows("https://r.example/v1", "a", AiEffort::Max), "not listed for the model");
        assert!(!levels.allows("https://r.example/v1", "plain", AiEffort::Low), "the service named none");
        assert!(!levels.allows("https://r.example/v1", "typed", AiEffort::Low), "never listed");
        assert!(!levels.allows("https://other.example/v1", "a", AiEffort::High), "another address");
        assert!(levels.allows("https://other.example/v1", "x", AiEffort::Default));
        assert!(!Levels::default().allows("https://r.example/v1", "a", AiEffort::High));

        let mut stored = Stored {
            active: "s1".into(),
            services: vec![Profile {
                id: "s1".into(),
                base_url: "https://r.example/v1".into(),
                model: "a".into(),
                effort: AiEffort::High,
                levels: levels.clone(),
                ..Profile::default()
            }],
            ..Stored::default()
        };
        assert_eq!(stored.view("s1").effort, AiEffort::High);
        stored.profile_mut("s1").unwrap().effort = AiEffort::Max;
        assert_eq!(stored.view("s1").effort, AiEffort::Default);
        let profile = stored.profile_mut("s1").unwrap();
        profile.effort = AiEffort::High;
        profile.model = "plain".into();
        assert_eq!(stored.view("s1").effort, AiEffort::Default);
        // The level itself is kept: back on a model that lists it, it shows again.
        stored.profile_mut("s1").unwrap().model = "a".into();
        assert_eq!(stored.view("s1").effort, AiEffort::High);
        stored.profile_mut("s1").unwrap().levels = Levels::default();
        assert_eq!(stored.view("s1").effort, AiEffort::Default);
    }

    #[test]
    fn a_key_with_spaces_or_control_characters_is_refused_without_being_repeated() {
        assert!(check_key("sk-abc_123.XYZ").is_ok());
        for bad in ["sk abc", "sk\nabc", "sk\u{e9}", "tab\t"] {
            let Err(Error::Invalid { msg }) = check_key(bad) else { panic!("{bad:?} accepted") };
            assert!(!msg.contains("sk"));
        }
    }

    #[test]
    fn the_host_label_shows_a_port_only_when_there_is_one() {
        assert_eq!(host_label(&Url::parse("https://api.example.com/v1").unwrap()), "api.example.com");
        assert_eq!(host_label(&Url::parse("http://192.168.1.20:11434/v1").unwrap()), "192.168.1.20:11434");
    }
}
