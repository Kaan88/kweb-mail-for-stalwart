//! M12 stage 6, the update check, over a real local HTTP exchange: what is sent (a generic agent
//! and nothing of the user) and how each kind of answer is told. The comparison and the reading of
//! the release are unit tests in `update.rs`.

use std::sync::{Arc, Mutex};

use kweb_core::{update, Error};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

/// Answers one request with `status` and `body`; returns the address and what the request looked like.
async fn serve(status: &'static str, body: &'static str) -> (String, Arc<Mutex<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/releases/latest", listener.local_addr().unwrap());
    let seen = Arc::new(Mutex::new(String::new()));
    let kept = seen.clone();
    tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut buffer = vec![0u8; 8192];
        let n = socket.read(&mut buffer).await.unwrap();
        *kept.lock().unwrap() = String::from_utf8_lossy(&buffer[..n]).to_string();
        let reply = format!(
            "HTTP/1.1 {status}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
            body.len()
        );
        socket.write_all(reply.as_bytes()).await.unwrap();
    });
    (url, seen)
}

fn rt() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap()
}

const RELEASE: &str = r#"{"tag_name":"v0.2.0","draft":false,"prerelease":false,
 "html_url":"https://github.com/Kaan88/kweb-mail-for-stalwart/releases/tag/v0.2.0"}"#;

#[test]
fn a_newer_release_is_found_and_the_request_carries_nothing_of_the_user() {
    rt().block_on(async {
        let (url, seen) = serve("200 OK", RELEASE).await;
        let info = update::check(&url, "0.1.0").await.unwrap();
        assert_eq!((info.latest.as_str(), info.newer), ("0.2.0", true));
        assert!(info.page_url.ends_with("releases/tag/v0.2.0"));
        let request = seen.lock().unwrap().to_lowercase();
        assert!(request.starts_with("get /releases/latest"), "{request}");
        assert!(request.contains("user-agent: kweb-mail\r\n"), "{request}");
        for forbidden in ["authorization", "cookie", "referer", "x-"] {
            assert!(!request.contains(&format!("\r\n{forbidden}")), "{forbidden} in {request}");
        }
    });
}

#[test]
fn the_same_version_is_not_newer() {
    rt().block_on(async {
        let (url, _) = serve("200 OK", RELEASE).await;
        let info = update::check(&url, "0.2.0").await.unwrap();
        assert!(!info.newer);
        assert_eq!(info.latest, "0.2.0");
    });
}

#[test]
fn no_published_release_is_an_answer_and_a_busy_page_is_told_plainly() {
    rt().block_on(async {
        let (url, _) = serve("404 Not Found", r#"{"message":"Not Found"}"#).await;
        let info = update::check(&url, "0.1.0").await.unwrap();
        assert!(info.latest.is_empty() && !info.newer && info.page_url.is_empty());

        let (url, _) = serve("403 Forbidden", r#"{"message":"API rate limit exceeded"}"#).await;
        match update::check(&url, "0.1.0").await {
            Err(Error::Temporary { msg }) => assert!(msg.contains("busy"), "{msg}"),
            other => panic!("expected a temporary error, got {other:?}"),
        }
        let (url, _) = serve("200 OK", "<html>not json</html>").await;
        assert!(update::check(&url, "0.1.0").await.is_err());
        // Nothing listens there.
        assert!(update::check("http://127.0.0.1:1/releases/latest", "0.1.0").await.is_err());
    });
}
