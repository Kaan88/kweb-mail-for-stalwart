//! What the user may set for the assistant, and what a change does to the consent. Pure: the
//! storage (the account's key/value table, and the secret store for the key) is in `mod.rs`.

use serde::{Deserialize, Serialize};
use url::{Host, Url};

use crate::{
    model::{AiEffort, AiProvider, AiSettings},
    Error, Result,
};

/// The account's key/value table holds the settings as JSON under this key (never an API key).
pub const SETTINGS_KEY: &str = "ai_settings";
/// Where stage 1 kept its one key in the secret store. Moved to the name of its service the
/// first time the settings are read, and then removed.
pub const LEGACY_KEY_NAME: &str = "ai_key";
/// Where a new Anthropic profile starts.
pub const ANTHROPIC_URL: &str = "https://api.anthropic.com/v1";

/// Every API key lives in the host's secret store under the name of its own service.
pub const KEY_NAMES: [&str; 2] = ["ai_key_openai", "ai_key_anthropic"];

pub fn key_name(provider: AiProvider) -> &'static str {
    match provider {
        AiProvider::OpenAiCompatible => KEY_NAMES[0],
        AiProvider::Anthropic => KEY_NAMES[1],
    }
}

/// What one kind of service keeps for itself.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Profile {
    pub base_url: String,
    pub model: String,
    pub effort: AiEffort,
    pub consented: bool,
}

/// What is stored under [`SETTINGS_KEY`]: the shared switches, the service in use, and a
/// profile for each kind of service. No key is in here.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Stored {
    pub enabled: bool,
    pub provider: AiProvider,
    pub allow_summaries: bool,
    pub allow_drafts: bool,
    pub allow_rewrites: bool,
    pub openai: Profile,
    pub anthropic: Profile,
}

impl Default for Stored {
    fn default() -> Self {
        Stored {
            enabled: false,
            provider: AiProvider::OpenAiCompatible,
            allow_summaries: true,
            allow_drafts: true,
            allow_rewrites: true,
            openai: Profile::default(),
            anthropic: Profile::default(),
        }
    }
}

impl Stored {
    pub fn profile(&self, provider: AiProvider) -> &Profile {
        match provider {
            AiProvider::OpenAiCompatible => &self.openai,
            AiProvider::Anthropic => &self.anthropic,
        }
    }

    pub fn profile_mut(&mut self, provider: AiProvider) -> &mut Profile {
        match provider {
            AiProvider::OpenAiCompatible => &mut self.openai,
            AiProvider::Anthropic => &mut self.anthropic,
        }
    }

    /// The settings with `provider` as the service in use. `has_key` is left false: the secret
    /// store says whether there is a key.
    pub fn view(&self, provider: AiProvider) -> AiSettings {
        let p = self.profile(provider);
        // The Anthropic address is shown as the default until a profile is stored, but it is
        // not the address any key was entered for (see `Profile::base_url`).
        let base_url = match (provider, p.base_url.is_empty()) {
            (AiProvider::Anthropic, true) => ANTHROPIC_URL.to_string(),
            _ => p.base_url.clone(),
        };
        AiSettings {
            enabled: self.enabled,
            provider,
            base_url,
            model: p.model.clone(),
            effort: p.effort,
            has_key: false,
            consented: p.consented,
            allow_summaries: self.allow_summaries,
            allow_drafts: self.allow_drafts,
            allow_rewrites: self.allow_rewrites,
        }
    }
}

/// Reads a stored value. Stage 1 stored the flat settings of its one service: that becomes the
/// profile of its provider (consent included), and the provider is returned, because the old
/// key belongs to it. `None` when the text cannot be read.
pub fn parse(json: &str) -> Option<(Stored, Option<AiProvider>)> {
    let value: serde_json::Value = serde_json::from_str(json).ok()?;
    if value.get("base_url").is_none() {
        return Some((serde_json::from_value(value).ok()?, None));
    }
    let old: AiSettings = serde_json::from_value(value).ok()?;
    let mut stored = Stored {
        enabled: old.enabled,
        provider: old.provider,
        allow_summaries: old.allow_summaries,
        allow_drafts: old.allow_drafts,
        allow_rewrites: old.allow_rewrites,
        ..Stored::default()
    };
    *stored.profile_mut(old.provider) =
        Profile { base_url: old.base_url, model: old.model, effort: old.effort, consented: old.consented };
    Some((stored, Some(old.provider)))
}

/// How the assistant is set up before the user has done anything: off, nothing agreed to.
pub fn defaults() -> AiSettings {
    Stored::default().view(AiProvider::OpenAiCompatible)
}

/// The settings as the user gave them, made fit to store. `has_key` is left false: the secret
/// store, not the settings, says whether there is a key.
///
/// `consented` is taken as given here; [`consent_after`] decides what survives a change.
pub fn normalise(mut s: AiSettings) -> Result<AiSettings> {
    s.base_url = s.base_url.trim().trim_end_matches('/').to_string();
    s.model = s.model.trim().to_string();
    s.has_key = false;
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
fn is_private(host: &Host<&str>) -> bool {
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
        let s = normalise(on("  https://api.example.com/v1//  ", "  gpt  ")).unwrap();
        assert_eq!((s.base_url.as_str(), s.model.as_str()), ("https://api.example.com/v1", "gpt"));
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
    fn a_new_anthropic_profile_starts_at_its_own_address_and_the_other_is_empty() {
        let s = Stored::default();
        assert_eq!(s.view(AiProvider::Anthropic).base_url, ANTHROPIC_URL);
        assert_eq!(s.view(AiProvider::OpenAiCompatible), defaults());
        assert_eq!(defaults().base_url, "");
    }

    #[test]
    fn stage_one_settings_become_the_profile_of_their_provider() {
        let flat = r#"{"enabled":true,"provider":"Anthropic","base_url":"https://a.example/v1","model":"m",
            "has_key":false,"consented":true,"allow_summaries":true,"allow_drafts":false,"allow_rewrites":true}"#;
        let (stored, old) = parse(flat).unwrap();
        assert_eq!(old, Some(AiProvider::Anthropic));
        let view = stored.view(AiProvider::Anthropic);
        assert!(view.enabled && view.consented && !view.allow_drafts);
        assert_eq!(
            (view.base_url.as_str(), view.model.as_str(), view.effort),
            ("https://a.example/v1", "m", AiEffort::Default)
        );
        assert_eq!(stored.openai, Profile::default());
        // The new form round-trips and says nothing about an old key.
        let json = serde_json::to_string(&stored).unwrap();
        assert_eq!(parse(&json), Some((stored, None)));
        assert_eq!(parse("{not json"), None);
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
