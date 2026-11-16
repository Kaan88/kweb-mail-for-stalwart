//! The update download over a real local HTTP exchange: where it may start, where a redirect may
//! lead, the size cap, the size and the digest check, cancel and the part file. The real address
//! rules are the same code with GitHub's names; here the allowed host is the local test origin.

use std::{
    path::Path,
    sync::{Arc, Mutex},
};

use kweb_core::{
    ffi::{DownloadCancel, DownloadListener},
    update, Error,
};
use sha2::{Digest, Sha256};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

/// Serves any number of requests. `.../ok` answers `body`; `.../hop` redirects to `redirect`
/// (`{origin}` in it is replaced by this server's own origin); `.../lie` promises ten bytes more
/// than it sends.
async fn serve(body: Vec<u8>, redirect: String) -> (String, Arc<Mutex<Vec<String>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let seen = Arc::new(Mutex::new(Vec::new()));
    let kept = seen.clone();
    let own = origin.clone();
    tokio::spawn(async move {
        loop {
            let Ok((mut socket, _)) = listener.accept().await else { return };
            let body = body.clone();
            let redirect = redirect.replace("{origin}", &own);
            let kept = kept.clone();
            tokio::spawn(async move {
                let mut buffer = vec![0u8; 8192];
                let n = socket.read(&mut buffer).await.unwrap_or(0);
                let request = String::from_utf8_lossy(&buffer[..n]).to_string();
                let path = request.split_whitespace().nth(1).unwrap_or("").to_string();
                kept.lock().unwrap().push(request);
                let (head, send_body) = if path.ends_with("/hop") {
                    (
                        format!("HTTP/1.1 302 Found\r\nlocation: {redirect}\r\ncontent-length: 0\r\nconnection: close\r\n\r\n"),
                        false,
                    )
                } else if path.ends_with("/ok") {
                    (format!("HTTP/1.1 200 OK\r\ncontent-length: {}\r\nconnection: close\r\n\r\n", body.len()), true)
                } else if path.ends_with("/lie") {
                    (
                        format!("HTTP/1.1 200 OK\r\ncontent-length: {}\r\nconnection: close\r\n\r\n", body.len() + 10),
                        true,
                    )
                } else {
                    ("HTTP/1.1 404 Not Found\r\ncontent-length: 0\r\nconnection: close\r\n\r\n".to_string(), false)
                };
                let _ = socket.write_all(head.as_bytes()).await;
                if send_body {
                    let _ = socket.write_all(&body).await;
                }
            });
        }
    });
    (origin, seen)
}

fn hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

struct Count(Mutex<(u64, Option<u64>)>);
impl DownloadListener for Count {
    fn progress(&self, done: u64, total: Option<u64>) {
        *self.0.lock().unwrap() = (done, total);
    }
}

fn run(
    origin: &str,
    path: &str,
    dest: &Path,
    size: u64,
    sha: &str,
    listener: Option<Arc<dyn DownloadListener>>,
) -> Result<bool, Error> {
    let origin = origin.to_string();
    let url = format!("{origin}/releases/download/{path}");
    let dest = dest.to_string_lossy().to_string();
    let sha = sha.to_string();
    // The call blocks and builds its own runtime, so it runs off the test's.
    std::thread::spawn(move || {
        update::update_download_local(origin, url, dest, size, sha, DownloadCancel::new(), listener)
    })
    .join()
    .unwrap()
}

fn rt() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap()
}

#[test]
fn a_file_with_the_right_size_and_digest_is_kept_and_progress_is_told() {
    let rt = rt();
    let body: Vec<u8> = (0..200_000u32).map(|i| (i % 251) as u8).collect();
    let (origin, seen) = rt.block_on(serve(body.clone(), String::new()));
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("a.apk");
    let count = Arc::new(Count(Mutex::new((0, None))));
    let done = run(&origin, "ok", &dest, body.len() as u64, &hex(&body), Some(count.clone())).unwrap();
    assert!(done);
    assert_eq!(std::fs::read(&dest).unwrap(), body);
    assert!(!dir.path().join("a.apk.part").exists());
    assert_eq!(*count.0.lock().unwrap(), (body.len() as u64, Some(body.len() as u64)));
    let request = seen.lock().unwrap()[0].to_lowercase();
    assert!(request.contains("user-agent: kweb-mail\r\n"), "{request}");
    for forbidden in ["authorization", "cookie", "referer"] {
        assert!(!request.contains(&format!("\r\n{forbidden}")), "{request}");
    }
}

#[test]
fn a_digest_or_size_mismatch_is_refused_and_leaves_nothing() {
    let rt = rt();
    let body = vec![7u8; 5000];
    let (origin, _) = rt.block_on(serve(body.clone(), String::new()));
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("a.apk");
    let wrong = hex(b"something else");
    assert!(run(&origin, "ok", &dest, 5000, &wrong, None).is_err());
    assert!(!dest.exists() && !dir.path().join("a.apk.part").exists());
    // The release says another size than the server sends.
    assert!(run(&origin, "ok", &dest, 4999, "", None).is_err());
    assert!(!dest.exists());
    // The server's length header is a lie: the body is short.
    assert!(run(&origin, "lie", &dest, 0, "", None).is_err());
    assert!(!dest.exists() && !dir.path().join("a.apk.part").exists());
    // With the right size and no digest it is accepted.
    assert!(run(&origin, "ok", &dest, 5000, "", None).unwrap());
}

#[test]
fn a_redirect_to_an_allowed_host_is_followed_and_one_to_another_host_is_refused() {
    let rt = rt();
    let body = vec![2u8; 3000];
    // Redirects back to its own origin (allowed), where `/ok` answers.
    let (own, _) = rt.block_on(serve(body.clone(), "{origin}/releases/download/ok".to_string()));
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("a.apk");
    assert!(run(&own, "hop", &dest, 3000, &hex(&body), None).unwrap());
    // Redirects to a second local server: another port is another host, so it is not followed.
    let (other, other_seen) = rt.block_on(serve(body.clone(), String::new()));
    let (hopper, _) = rt.block_on(serve(body.clone(), format!("{other}/releases/download/ok")));
    let dest2 = dir.path().join("b.apk");
    assert!(run(&hopper, "hop", &dest2, 0, "", None).is_err());
    assert!(!dest2.exists() && !dir.path().join("b.apk.part").exists());
    assert!(other_seen.lock().unwrap().is_empty(), "the foreign host was asked");
}

#[test]
fn an_address_outside_the_release_and_a_too_large_file_are_refused_before_any_request() {
    let rt = rt();
    let (origin, seen) = rt.block_on(serve(vec![0u8; 10], String::new()));
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("a.apk");
    let outside = format!("{origin}/other/file");
    let (o, d) = (origin.clone(), dest.to_string_lossy().to_string());
    let r = std::thread::spawn(move || {
        update::update_download_local(o, outside, d, 0, String::new(), DownloadCancel::new(), None)
    })
    .join()
    .unwrap();
    assert!(r.is_err());
    // Over the cap.
    assert!(run(&origin, "ok", &dest, update::MAX_APK + 1, "", None).is_err());
    // The real entry point refuses a local address.
    let (o, d) = (origin.clone(), dest.to_string_lossy().to_string());
    let r = std::thread::spawn(move || {
        update::update_download(format!("{o}/releases/download/ok"), d, 0, String::new(), DownloadCancel::new(), None)
    })
    .join()
    .unwrap();
    assert!(r.is_err());
    // The test entry points refuse a non-local origin.
    assert!(update::update_check_local("https://example.com".into(), "0.1.0".into()).is_err());
    assert!(seen.lock().unwrap().is_empty(), "a request was made");
}

#[test]
fn a_cancelled_download_ends_without_a_file() {
    let rt = rt();
    let (origin, _) = rt.block_on(serve(vec![0u8; 100], String::new()));
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("a.apk");
    let cancel = DownloadCancel::new();
    cancel.cancel();
    let (o, d) = (origin.clone(), dest.to_string_lossy().to_string());
    let url = format!("{origin}/releases/download/ok");
    let r = std::thread::spawn(move || update::update_download_local(o, url, d, 100, String::new(), cancel, None))
        .join()
        .unwrap();
    assert!(!r.unwrap());
    assert!(!dest.exists() && !dir.path().join("a.apk.part").exists());
}

#[test]
fn the_release_answer_carries_the_apk_with_size_and_digest() {
    let base = "https://github.com/Kaan88/kweb-mail-for-stalwart/releases/download/v0.2.0";
    let json = serde_json::json!({"tag_name":"v0.2.0","html_url":"https://github.com/Kaan88/kweb-mail-for-stalwart/releases/tag/v0.2.0",
      "assets":[
        {"name":"notes.txt","browser_download_url":format!("{base}/notes.txt"),"size":3},
        {"name":"evil.apk","browser_download_url":"https://evil.example/releases/download/v0.2.0/evil.apk","size":3},
        {"name":"kweb-mail-0.2.0.apk","browser_download_url":format!("{base}/kweb-mail-0.2.0.apk"),"size":46878957,
         "digest":"sha256:8C89F94404B24F097E0CF42BE61FA12DAE6E07ACBF4F1CB3B7546185D4DA75DA"}]});
    let info = update::read_release(&json, "0.1.0");
    assert_eq!(info.apk_name, "kweb-mail-0.2.0.apk");
    assert_eq!(info.apk_size, 46878957);
    assert_eq!(info.apk_sha256, "8c89f94404b24f097e0cf42be61fa12dae6e07acbf4f1cb3b7546185d4da75da");
    // A malformed digest: empty.
    let mut j2 = json.clone();
    j2["assets"][2]["digest"] = serde_json::json!("md5:abc");
    assert!(update::read_release(&j2, "0.1.0").apk_sha256.is_empty());
    // No APK under the project's own downloads: none.
    let none = serde_json::json!({"tag_name":"v0.2.0","assets":[json["assets"][1].clone()]});
    assert!(update::read_release(&none, "0.1.0").apk_url.is_empty());
}

/// Needs the network: the published v0.2.0 file through GitHub's real redirect. Run by hand with
/// `cargo test -p kweb-core --test main real_release -- --ignored`.
#[test]
#[ignore]
fn real_release_file_downloads_through_githubs_redirect_and_matches_its_digest() {
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("real.apk");
    let url = "https://github.com/Kaan88/kweb-mail-for-stalwart/releases/download/v0.2.0/kweb-mail-0.2.0.apk";
    let info = update::update_check("0.1.0".into()).unwrap();
    assert_eq!(info.apk_url, url);
    let done = update::update_download(
        url.into(),
        dest.to_string_lossy().to_string(),
        info.apk_size,
        info.apk_sha256,
        DownloadCancel::new(),
        None,
    )
    .unwrap();
    assert!(done && dest.metadata().unwrap().len() == info.apk_size);
}
