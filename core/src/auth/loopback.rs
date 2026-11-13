//! A one-shot listener on 127.0.0.1 for hosts that use a loopback redirect (RFC 8252 7.3).

use std::time::Duration;

use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};
use url::Url;

use crate::{Error, Result};

const REDIRECT_PATH: &str = "/callback";

/// How long one loopback connection may take to send its request line before it is dropped.
const READ_TIMEOUT: Duration = if cfg!(test) { Duration::from_millis(200) } else { Duration::from_secs(5) };

/// Binds 127.0.0.1 on a free port. Returns the listener and the redirect URI to give to
/// [`super::begin_sign_in`].
pub async fn bind() -> Result<(TcpListener, String)> {
    let io = |e: std::io::Error| Error::temporary(format!("local listener: {e}"));
    let listener = TcpListener::bind("127.0.0.1:0").await.map_err(io)?;
    let port = listener.local_addr().map_err(io)?.port();
    Ok((listener, format!("http://127.0.0.1:{port}{REDIRECT_PATH}")))
}

/// Reads up to the end of the request head. `None` for a connection that sends nothing in time
/// (browsers open spare "preconnect" sockets that never carry a request) or breaks.
async fn read_head(stream: &mut TcpStream) -> Option<String> {
    let read = async {
        let mut buf = vec![0u8; 8192];
        let mut len = 0;
        while len < buf.len() {
            let n = stream.read(&mut buf[len..]).await.ok()?;
            if n == 0 {
                break;
            }
            len += n;
            if buf[..len].windows(4).any(|w| w == b"\r\n\r\n") {
                break;
            }
        }
        (len > 0).then(|| String::from_utf8_lossy(&buf[..len]).into_owned())
    };
    tokio::time::timeout(READ_TIMEOUT, read).await.ok().flatten()
}

/// Waits for the browser to come back to the listener. Returns the full callback URL (with
/// `code`, or `error`) for [`super::complete_sign_in`]. `who` names the app on the page the
/// browser shows afterwards. Connections are served one at a time, each with a short read
/// deadline; anything that is not a reply with our `state` is answered and ignored, so a stray
/// request can't end the wait. The caller bounds the wait, e.g. with [`super::SIGN_IN_TIMEOUT`].
pub async fn wait_for_callback(listener: TcpListener, state: &str, who: &str) -> Result<String> {
    loop {
        let (mut stream, _) = listener.accept().await.map_err(|e| Error::temporary(format!("local listener: {e}")))?;
        let Some(head) = read_head(&mut stream).await else { continue };
        let target = head.lines().next().and_then(|l| l.split_whitespace().nth(1)).unwrap_or("/");
        let url = match Url::parse(&format!("http://127.0.0.1{target}")) {
            Ok(u) if u.path() == REDIRECT_PATH => u,
            _ => {
                let _ =
                    stream.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await;
                continue; // favicon and the like
            }
        };
        let param = |k: &str| url.query_pairs().find(|(n, _)| n == k).map(|(_, v)| v.into_owned());
        if param("state").as_deref() != Some(state) {
            // Not this sign-in's reply (an old tab, another program): keep waiting for ours.
            let _ =
                stream.write_all(b"HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await;
            continue;
        }
        let (title, text) = if param("error").is_some() {
            ("Not signed in", format!("{who} was not signed in. You can close this tab."))
        } else if param("code").is_none() {
            ("Sign-in failed", format!("{who} could not sign in. You can close this tab and try again."))
        } else {
            ("Signed in", format!("{who} is signed in. You can close this tab."))
        };
        let page = format!(
            "<!doctype html><meta charset=utf-8><title>{title}</title><body style=\"font:16px system-ui;margin:15vh auto;max-width:28em;padding:0 1em\"><h1>{title}</h1><p>{text}</p>"
        );
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{page}",
            page.len()
        );
        let _ = stream.write_all(response.as_bytes()).await;
        let _ = stream.shutdown().await;
        return Ok(url.to_string());
    }
}
