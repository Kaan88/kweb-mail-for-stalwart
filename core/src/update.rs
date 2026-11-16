//! The update check (M12 stage 6, item 10): is there a newer release than the one installed?
//!
//! The app is installed from a file, so nothing else tells the user. The core asks GitHub's
//! release page of the project, over its own HTTP client, so the phone-side code never touches
//! the network. The request carries nothing of the user: no account, no address, no device
//! name, no token, only the generic agent `kweb-mail`. Nothing is downloaded or installed: the
//! answer is a version and the address of the release page, which the app opens in the browser.

use std::time::Duration;

use serde_json::Value;

use crate::{Error, Result};

/// The latest published release of the project (a draft or a pre-release is never "latest").
pub const RELEASES_URL: &str = "https://api.github.com/repos/Kaan88/kweb-mail-for-stalwart/releases/latest";
/// The only place a release page may be: the project's own on GitHub.
const PAGE_PREFIX: &str = "https://github.com/Kaan88/kweb-mail-for-stalwart/";
/// What an answer may weigh. The real one is a few kilobytes.
const MAX_BODY: usize = 512 * 1024;
const LIMIT: Duration = Duration::from_secs(20);

/// What the check found.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct UpdateInfo {
    /// The newest published version, without a leading `v`; empty when no release is published.
    pub latest: String,
    /// `latest` is newer than the installed version.
    pub newer: bool,
    /// The release page, for the browser; empty when there is none.
    pub page_url: String,
}

/// Asks the project's release page and compares with `current` (the installed version name).
#[uniffi::export]
pub fn update_check(current_version: String) -> Result<UpdateInfo> {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| Error::temporary("The update check could not start."))?;
    rt.block_on(check(RELEASES_URL, &current_version))
}

/// The check against `url` (the real one is [`RELEASES_URL`]; tests use a local address).
pub async fn check(url: &str, current: &str) -> Result<UpdateInfo> {
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .referer(false)
        .no_proxy()
        .timeout(LIMIT)
        .user_agent("kweb-mail")
        .build()
        .map_err(|_| Error::temporary("The update check could not start."))?;
    let unreachable = Error::temporary("The release page could not be reached.");
    let answer = client
        .get(url)
        .header(reqwest::header::ACCEPT, "application/vnd.github+json")
        .send()
        .await
        .map_err(|_| unreachable.clone())?;
    let status = answer.status();
    if status == reqwest::StatusCode::NOT_FOUND {
        // No release is published yet (a draft is not visible to anyone else).
        return Ok(UpdateInfo { latest: String::new(), newer: false, page_url: String::new() });
    }
    if status == reqwest::StatusCode::FORBIDDEN || status == reqwest::StatusCode::TOO_MANY_REQUESTS {
        return Err(Error::temporary("The release page is busy right now. Try again later."));
    }
    if !status.is_success() {
        return Err(Error::temporary(format!("The release page answered with HTTP {}.", status.as_u16())));
    }
    if answer.content_length().is_some_and(|n| n as usize > MAX_BODY) {
        return Err(Error::temporary("The release page sent more than expected."));
    }
    let bytes = answer.bytes().await.map_err(|_| unreachable)?;
    if bytes.len() > MAX_BODY {
        return Err(Error::temporary("The release page sent more than expected."));
    }
    let json: Value = serde_json::from_slice(&bytes)
        .map_err(|_| Error::temporary("The release page sent an answer that cannot be read."))?;
    Ok(read_release(&json, current))
}

/// The answer of the releases API worked out: its tag, its page, and whether it is newer.
pub fn read_release(json: &Value, current: &str) -> UpdateInfo {
    let none = UpdateInfo { latest: String::new(), newer: false, page_url: String::new() };
    if json["draft"].as_bool() == Some(true) || json["prerelease"].as_bool() == Some(true) {
        return none;
    }
    let Some(tag) = json["tag_name"].as_str() else { return none };
    let latest = tag.trim().trim_start_matches(['v', 'V']).to_string();
    if parse(&latest).is_none() {
        return none;
    }
    let page = json["html_url"].as_str().filter(|u| u.starts_with(PAGE_PREFIX)).unwrap_or_default();
    UpdateInfo { newer: is_newer(current, &latest), latest, page_url: page.to_string() }
}

/// `latest` is a higher version than `current`. Versions are `major.minor.patch` with any
/// number of numeric parts; a pre-release suffix (`-rc1`) is lower than the same version without
/// one. A version that cannot be read is never newer.
pub fn is_newer(current: &str, latest: &str) -> bool {
    match (parse(current), parse(latest)) {
        (Some(c), Some(l)) => l > c,
        _ => false,
    }
}

/// Numbers (missing parts are zero) and the release flag (true when there is no suffix).
fn parse(version: &str) -> Option<(Vec<u64>, bool)> {
    let version = version.trim().trim_start_matches(['v', 'V']);
    let (numbers, suffix) = match version.split_once(['-', '+']) {
        Some((n, s)) => (n, Some(s)),
        None => (version, None),
    };
    let mut parts: Vec<u64> = numbers.split('.').map(|p| p.parse().ok()).collect::<Option<_>>()?;
    if parts.is_empty() || parts.len() > 6 {
        return None;
    }
    while parts.len() < 3 {
        parts.push(0);
    }
    // `+build` metadata does not make a version lower; a `-pre` suffix does.
    let release = !suffix.is_some_and(|_| version.contains('-'));
    Some((parts, release))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn versions_compare_by_number_not_by_text() {
        assert!(is_newer("0.1.0", "0.1.1"));
        assert!(is_newer("0.1.0", "v0.2.0"));
        assert!(is_newer("0.9.0", "0.10.0"));
        assert!(is_newer("0.1.0", "1"));
        assert!(!is_newer("0.1.0", "0.1.0"));
        assert!(!is_newer("0.2.0", "0.1.9"));
        assert!(is_newer("0.2.0-rc1", "0.2.0"));
        assert!(!is_newer("0.2.0", "0.2.0-rc1"));
        assert!(!is_newer("0.1.0", "nightly"));
        assert!(!is_newer("dev", "0.1.0"));
    }

    #[test]
    fn a_release_is_read_with_its_page_and_a_foreign_page_is_dropped() {
        let ok = json!({"tag_name":"v0.2.0","html_url":"https://github.com/Kaan88/kweb-mail-for-stalwart/releases/tag/v0.2.0",
            "draft":false,"prerelease":false});
        let info = read_release(&ok, "0.1.0");
        assert_eq!(info.latest, "0.2.0");
        assert!(info.newer);
        assert!(info.page_url.ends_with("/tag/v0.2.0"));
        // A page that is not the project's own is not offered to the browser.
        let foreign = json!({"tag_name":"v0.2.0","html_url":"https://evil.example/releases"});
        let info = read_release(&foreign, "0.1.0");
        assert!(info.newer && info.page_url.is_empty());
        // The same version: not newer.
        assert!(!read_release(&ok, "0.2.0").newer);
    }

    #[test]
    fn drafts_prereleases_and_odd_tags_are_no_release() {
        let none = UpdateInfo { latest: String::new(), newer: false, page_url: String::new() };
        assert_eq!(read_release(&json!({"tag_name":"v9.0.0","draft":true}), "0.1.0"), none);
        assert_eq!(read_release(&json!({"tag_name":"v9.0.0","prerelease":true}), "0.1.0"), none);
        assert_eq!(read_release(&json!({"tag_name":"latest-build"}), "0.1.0"), none);
        assert_eq!(read_release(&json!({}), "0.1.0"), none);
    }
}
