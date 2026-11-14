//! What the user may set for the assistant, and what a change does to the consent. Pure: the
//! storage (the account's key/value table, and the secret store for the key) is in `mod.rs`.

use url::{Host, Url};

use crate::{
    model::{AiProvider, AiSettings},
    Error, Result,
};

/// The account's key/value table holds the settings as JSON under this key (never the API key).
pub const SETTINGS_KEY: &str = "ai_settings";
/// The API key lives in the host's secret store under this name.
pub const KEY_NAME: &str = "ai_key";

/// How the assistant is set up before the user has done anything: off, nothing agreed to.
pub fn defaults() -> AiSettings {
    AiSettings {
        enabled: false,
        provider: AiProvider::OpenAiCompatible,
        base_url: String::new(),
        model: String::new(),
        has_key: false,
        consented: false,
        allow_summaries: true,
        allow_drafts: true,
        allow_rewrites: true,
    }
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

/// The consent to store: a yes survives only while provider, address and model are the ones
/// stored. The core cannot tell a caller that asked again from one that copied the old value,
/// so a change of the three always clears it, and the host grants it in a second call that
/// repeats the (now stored) values, right after it showed what goes where.
pub fn consent_after(before: &AiSettings, new: &AiSettings) -> bool {
    new.consented && (before.provider, &before.base_url, &before.model) == (new.provider, &new.base_url, &new.model)
}

/// Whether a key entered for `before` may be sent to `new`: same kind of service, and the
/// same scheme, host and port (a different path on the same host keeps it).
pub fn keeps_key(before: &AiSettings, new: &AiSettings) -> bool {
    let origin = |s: &AiSettings| Url::parse(&s.base_url).ok().map(|u| u.origin());
    before.provider == new.provider && origin(before) == origin(new)
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
        assert!(consent_after(&agreed, &agreed));
        assert!(!consent_after(&agreed, &AiSettings { consented: false, ..agreed.clone() }));
        // Never given by a call that changes any of the three, even when it says yes.
        assert!(!consent_after(&agreed, &AiSettings { model: "m2".into(), ..agreed.clone() }));
        assert!(!consent_after(&agreed, &AiSettings { base_url: "https://b.example/v1".into(), ..agreed.clone() }));
        let other = AiSettings { provider: AiProvider::Anthropic, ..agreed.clone() };
        assert!(!consent_after(&agreed, &other));
        // A first setup (nothing stored) never starts agreed.
        assert!(!consent_after(&defaults(), &agreed));
    }

    #[test]
    fn a_key_stays_with_its_origin_and_provider() {
        let at = |url: &str| on(url, "m");
        let before = at("https://a.example/v1");
        assert!(keeps_key(&before, &at("https://a.example/v2/other")));
        assert!(keeps_key(&before, &AiSettings { model: "m2".into(), ..before.clone() }));
        for moved in ["https://b.example/v1", "http://a.example/v1", "https://a.example:8443/v1"] {
            assert!(!keeps_key(&before, &at(moved)), "{moved}");
        }
        assert!(!keeps_key(&before, &AiSettings { provider: AiProvider::Anthropic, ..before.clone() }));
        assert!(!keeps_key(&defaults(), &before));
        assert!(keeps_key(&defaults(), &defaults()));
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
