//! The update check (M12 stage 6, item 10): is there a newer release than the one installed?
//!
//! The app is installed from a file, so nothing else tells the user. The core asks GitHub's
//! release page of the project, over its own HTTP client, so the phone-side code never touches
//! the network. The request carries nothing of the user: no account, no address, no device
//! name, no token, only the generic agent `kweb-mail`. The answer is a version, the address of the
//! release page and the release's APK (name, address, size, SHA-256 when GitHub gives it).
//!
//! The APK can then be fetched by [`update_download`]: only from the project's own release
//! address, following redirects only to GitHub's asset hosts, capped in size, checked against the
//! size and the digest GitHub published, written to a `.part` file and renamed. Installing is the
//! host's business and always goes through the system's own prompt.

use std::{io::Write, sync::Arc, time::Duration};

use sha2::{Digest, Sha256};

use serde_json::Value;

use crate::{Error, Result};

/// The latest published release of the project (a draft or a pre-release is never "latest").
pub const RELEASES_URL: &str = "https://api.github.com/repos/Kaan88/kweb-mail-for-stalwart/releases/latest";
/// The only place a release page may be: the project's own on GitHub.
const PAGE_PREFIX: &str = "https://github.com/Kaan88/kweb-mail-for-stalwart/";
/// What an answer may weigh. The real one is a few kilobytes.
const MAX_BODY: usize = 512 * 1024;
const LIMIT: Duration = Duration::from_secs(20);
/// The most an APK may weigh (the real one is about 45 MB).
pub const MAX_APK: u64 = 200 * 1024 * 1024;
/// Where the release's files are.
const DOWNLOAD_PART: &str = "releases/download/";
/// The hosts GitHub redirects a release download to (checked against the real answer of a release).
const ASSET_HOSTS: [&str; 2] = ["objects.githubusercontent.com", "release-assets.githubusercontent.com"];
/// A silent stretch of the transfer that ends it.
const IDLE: Duration = Duration::from_secs(60);
const MAX_REDIRECTS: usize = 3;

/// What the check found.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct UpdateInfo {
    /// The newest published version, without a leading `v`; empty when no release is published.
    pub latest: String,
    /// `latest` is newer than the installed version.
    pub newer: bool,
    /// The release page, for the browser; empty when there is none.
    pub page_url: String,
    /// The release's APK, when it has one: its file name (empty: none) ...
    pub apk_name: String,
    /// ... its address (always under the project's own release downloads) ...
    pub apk_url: String,
    /// ... its size in bytes (0: not told) ...
    pub apk_size: u64,
    /// ... and its SHA-256 as 64 lower-case hex digits (empty: GitHub gave none).
    pub apk_sha256: String,
}

impl UpdateInfo {
    fn none() -> Self {
        UpdateInfo {
            latest: String::new(),
            newer: false,
            page_url: String::new(),
            apk_name: String::new(),
            apk_url: String::new(),
            apk_size: 0,
            apk_sha256: String::new(),
        }
    }
}

/// Asks the project's release page and compares with `current` (the installed version name).
#[uniffi::export]
pub fn update_check(current_version: String) -> Result<UpdateInfo> {
    let rt = runtime()?;
    rt.block_on(check(RELEASES_URL, &current_version))
}

/// For a debug build only: as [`update_check`] against a release answer served at
/// `<origin>/releases/latest`, where `origin` is a local address (`http://127.0.0.1:port` or
/// `http://10.0.2.2:port`). Anything else is refused, so even a misuse cannot reach outside.
#[uniffi::export]
pub fn update_check_local(origin: String, current_version: String) -> Result<UpdateInfo> {
    let origin = local_origin(&origin)?;
    let rt = runtime()?;
    rt.block_on(check_with(&format!("{origin}/releases/latest"), &current_version, &format!("{origin}/")))
}

fn runtime() -> Result<tokio::runtime::Runtime> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| Error::temporary("The update could not start."))
}

/// `http://<loopback or emulator host>:<port>` without a path, or an error.
fn local_origin(origin: &str) -> Result<String> {
    let origin = origin.trim().trim_end_matches('/');
    let url = reqwest::Url::parse(origin).map_err(|_| Error::invalid("not a local address"))?;
    let local = matches!(url.host_str(), Some("127.0.0.1" | "localhost" | "10.0.2.2"));
    if url.scheme() != "http" || !local || url.path() != "/" || url.query().is_some() {
        return Err(Error::invalid("not a local address"));
    }
    Ok(origin.to_string())
}

/// The check against `url` (the real one is [`RELEASES_URL`]; tests use a local address).
pub async fn check(url: &str, current: &str) -> Result<UpdateInfo> {
    check_with(url, current, PAGE_PREFIX).await
}

/// As [`check`], with `base` as the address every page and file of the release must be under.
pub async fn check_with(url: &str, current: &str, base: &str) -> Result<UpdateInfo> {
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
        return Ok(UpdateInfo::none());
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
    Ok(read_release_at(&json, current, base))
}

/// The answer of the releases API worked out: its tag, its page, and whether it is newer.
pub fn read_release(json: &Value, current: &str) -> UpdateInfo {
    read_release_at(json, current, PAGE_PREFIX)
}

fn read_release_at(json: &Value, current: &str, base: &str) -> UpdateInfo {
    let none = UpdateInfo::none();
    if json["draft"].as_bool() == Some(true) || json["prerelease"].as_bool() == Some(true) {
        return none;
    }
    let Some(tag) = json["tag_name"].as_str() else { return none };
    let latest = tag.trim().trim_start_matches(['v', 'V']).to_string();
    if parse(&latest).is_none() {
        return none;
    }
    let page = json["html_url"].as_str().filter(|u| u.starts_with(base)).unwrap_or_default();
    let mut info = UpdateInfo { newer: is_newer(current, &latest), latest, page_url: page.to_string(), ..none };
    let files = format!("{base}{DOWNLOAD_PART}");
    let apk = json["assets"].as_array().into_iter().flatten().find(|a| {
        a["name"].as_str().is_some_and(|n| n.to_ascii_lowercase().ends_with(".apk"))
            && a["browser_download_url"].as_str().is_some_and(|u| u.starts_with(&files))
    });
    if let Some(apk) = apk {
        info.apk_name = apk["name"].as_str().unwrap_or_default().to_string();
        info.apk_url = apk["browser_download_url"].as_str().unwrap_or_default().to_string();
        info.apk_size = apk["size"].as_u64().unwrap_or(0);
        info.apk_sha256 = apk["digest"]
            .as_str()
            .and_then(|d| d.strip_prefix("sha256:"))
            .map(str::to_ascii_lowercase)
            .filter(|h| h.len() == 64 && h.bytes().all(|b| b.is_ascii_hexdigit()))
            .unwrap_or_default();
    }
    info
}

/// Where a download may start and where it may be sent on to.
struct Trust {
    /// The first address must start with this.
    start: String,
    /// Hosts (with a port when not the default) a redirect may lead to.
    hosts: Vec<String>,
    /// Redirect targets must be `https` (false only for the local test origin).
    https_only: bool,
}

fn host_key(url: &reqwest::Url) -> String {
    match (url.host_str(), url.port()) {
        (Some(h), Some(p)) => format!("{h}:{p}"),
        (Some(h), None) => h.to_string(),
        _ => String::new(),
    }
}

/// Downloads the release's APK from `url` (the `apk_url` of [`update_check`]) to `dest_path`,
/// reporting progress to `listener`; stoppable with `cancel`. `true` when the file is complete,
/// its size equals `expected_size` (when not 0) and its SHA-256 equals `expected_sha256` (when not
/// empty); `false` when cancelled. A refusal leaves nothing at `dest_path`.
#[uniffi::export]
pub fn update_download(
    url: String,
    dest_path: String,
    expected_size: u64,
    expected_sha256: String,
    cancel: Arc<crate::ffi::DownloadCancel>,
    listener: Option<Arc<dyn crate::ffi::DownloadListener>>,
) -> Result<bool> {
    let trust = Trust {
        start: format!("{PAGE_PREFIX}{DOWNLOAD_PART}"),
        hosts: ASSET_HOSTS.iter().map(|h| h.to_string()).collect(),
        https_only: true,
    };
    let rt = runtime()?;
    rt.block_on(download_with(&trust, &url, &dest_path, expected_size, &expected_sha256, &cancel, listener.as_deref()))
}

/// For a debug build only: as [`update_download`] for a file served by the local test origin.
#[uniffi::export]
pub fn update_download_local(
    origin: String,
    url: String,
    dest_path: String,
    expected_size: u64,
    expected_sha256: String,
    cancel: Arc<crate::ffi::DownloadCancel>,
    listener: Option<Arc<dyn crate::ffi::DownloadListener>>,
) -> Result<bool> {
    let origin = local_origin(&origin)?;
    let host = reqwest::Url::parse(&origin).map(|u| host_key(&u)).unwrap_or_default();
    let trust = Trust { start: format!("{origin}/{DOWNLOAD_PART}"), hosts: vec![host], https_only: false };
    let rt = runtime()?;
    rt.block_on(download_with(&trust, &url, &dest_path, expected_size, &expected_sha256, &cancel, listener.as_deref()))
}

async fn download_with(
    trust: &Trust,
    url: &str,
    dest_path: &str,
    expected_size: u64,
    expected_sha256: &str,
    cancel: &crate::ffi::DownloadCancel,
    listener: Option<&dyn crate::ffi::DownloadListener>,
) -> Result<bool> {
    let part = format!("{dest_path}.part");
    let result = fetch(trust, url, &part, expected_size, expected_sha256, cancel, listener).await;
    match result {
        Ok(true) => {
            std::fs::rename(&part, dest_path).map(|()| true).map_err(|_| Error::store("The update could not be saved."))
        }
        other => {
            let _ = std::fs::remove_file(&part);
            other
        }
    }
}

async fn fetch(
    trust: &Trust,
    url: &str,
    part: &str,
    expected_size: u64,
    expected_sha256: &str,
    cancel: &crate::ffi::DownloadCancel,
    listener: Option<&dyn crate::ffi::DownloadListener>,
) -> Result<bool> {
    if !url.starts_with(&trust.start) {
        return Err(Error::invalid("The update is not at the project's release address."));
    }
    if expected_size > MAX_APK {
        return Err(Error::invalid("The update is larger than expected."));
    }
    if cancel.is_cancelled() {
        return Ok(false);
    }
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .referer(false)
        .no_proxy()
        .connect_timeout(LIMIT)
        .user_agent("kweb-mail")
        .build()
        .map_err(|_| Error::temporary("The update could not start."))?;
    let offline = || Error::temporary("The update could not be downloaded. Check the connection.");
    let mut next = reqwest::Url::parse(url).map_err(|_| Error::invalid("The update address is not valid."))?;
    let mut response = None;
    for hop in 0..=MAX_REDIRECTS {
        let sent = tokio::select! {
            sent = client.get(next.clone()).send() => sent.map_err(|_| offline())?,
            () = cancel.cancelled() => return Ok(false),
        };
        let status = sent.status();
        if status.is_redirection() {
            if hop == MAX_REDIRECTS {
                return Err(Error::temporary("The update address sent too many redirects."));
            }
            let target = sent
                .headers()
                .get(reqwest::header::LOCATION)
                .and_then(|v| v.to_str().ok())
                .and_then(|v| next.join(v).ok())
                .ok_or_else(|| Error::temporary("The update address sent an unreadable redirect."))?;
            let scheme_ok = target.scheme() == "https" || (!trust.https_only && target.scheme() == "http");
            if !scheme_ok || !trust.hosts.contains(&host_key(&target)) || !target.username().is_empty() {
                return Err(Error::invalid("The update was sent on to an address that is not trusted."));
            }
            next = target;
            continue;
        }
        if !status.is_success() {
            return Err(Error::temporary(format!("The update download answered with HTTP {}.", status.as_u16())));
        }
        response = Some(sent);
        break;
    }
    let mut response = response.ok_or_else(|| Error::temporary("The update could not be downloaded."))?;
    let total = response.content_length();
    if total.is_some_and(|t| t > MAX_APK) {
        return Err(Error::invalid("The update is larger than expected."));
    }
    if expected_size != 0 && total.is_some_and(|t| t != expected_size) {
        return Err(Error::invalid("The update is not the size the release says. It was not kept."));
    }
    let shown_total = total.or(Some(expected_size).filter(|n| *n != 0));
    let mut file = std::fs::File::create(part).map_err(|_| Error::store("The update could not be saved."))?;
    let mut hasher = Sha256::new();
    let mut done: u64 = 0;
    let mut last = std::time::Instant::now();
    if let Some(l) = listener {
        l.progress(0, shown_total);
    }
    loop {
        let chunk = tokio::select! {
            chunk = tokio::time::timeout(IDLE, response.chunk()) => match chunk {
                Ok(c) => c.map_err(|_| Error::temporary("The update download broke off."))?,
                Err(_) => return Err(Error::temporary("The update download stopped.")),
            },
            () = cancel.cancelled() => return Ok(false),
        };
        let Some(chunk) = chunk else { break };
        done += chunk.len() as u64;
        if done > MAX_APK || (expected_size != 0 && done > expected_size) {
            return Err(Error::invalid("The update is larger than the release says. It was not kept."));
        }
        hasher.update(&chunk);
        file.write_all(&chunk).map_err(|_| Error::store("The update could not be saved."))?;
        if last.elapsed() >= Duration::from_millis(150) {
            if let Some(l) = listener {
                l.progress(done, shown_total);
            }
            last = std::time::Instant::now();
        }
    }
    file.flush().map_err(|_| Error::store("The update could not be saved."))?;
    drop(file);
    if (expected_size != 0 && done != expected_size) || total.is_some_and(|t| t != done) {
        return Err(Error::temporary("The update download was cut short."));
    }
    if !expected_sha256.is_empty() {
        let got: String = hasher.finalize().iter().map(|b| format!("{b:02x}")).collect();
        if !got.eq_ignore_ascii_case(expected_sha256) {
            return Err(Error::invalid("The update does not match the checksum of the release. It was not kept."));
        }
    }
    if let Some(l) = listener {
        l.progress(done, shown_total);
    }
    Ok(true)
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
        let none = UpdateInfo::none();
        assert_eq!(read_release(&json!({"tag_name":"v9.0.0","draft":true}), "0.1.0"), none);
        assert_eq!(read_release(&json!({"tag_name":"v9.0.0","prerelease":true}), "0.1.0"), none);
        assert_eq!(read_release(&json!({"tag_name":"latest-build"}), "0.1.0"), none);
        assert_eq!(read_release(&json!({}), "0.1.0"), none);
    }
}
