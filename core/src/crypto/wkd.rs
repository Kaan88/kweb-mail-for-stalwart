//! Web Key Directory (draft-koch-openpgp-webkey-service): looking up a recipient's OpenPGP key at their mail
//! provider's own web server.
//!
//! Only ever started by the user for one address ("Look for a key"). It is a lookup the recipient's provider
//! sees (their server is asked for a file named after the address). Rules kept here: HTTPS only; no redirect is
//! followed (not even to the same host); no credential, cookie or header of the account is sent; a small answer
//! limit and a short time; addresses that are not plain internet names are refused before anything is sent.

use std::future::Future;

use pgp::composed::SignedPublicKey;
use sha1::{Digest, Sha1};

use super::openpgp;

/// The most a directory may answer with. A key is a few kilobytes.
pub const MAX_ANSWER: usize = 128 * 1024;

const ZBASE32: &[u8; 32] = b"ybndrfg8ejkmcpqxot1uwisza345h769";

/// An address that may be looked up: its local part (as written) and its domain (lower case).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub local: String,
    pub domain: String,
}

impl Target {
    /// `None` for anything but a plain ASCII address at a name with a dot (no IP number, no `localhost`).
    pub fn parse(address: &str) -> Option<Target> {
        let address = address.trim();
        let (local, domain) = address.rsplit_once('@')?;
        let ok_local = !local.is_empty()
            && local.len() <= 64
            && local.bytes().all(|b| b.is_ascii_graphic() && b != b'"' && b != b'@');
        let domain = domain.to_ascii_lowercase();
        let labels: Vec<&str> = domain.split('.').collect();
        let ok_domain = domain.len() <= 253
            && labels.len() >= 2
            && labels.iter().all(|l| {
                !l.is_empty()
                    && l.len() <= 63
                    && !l.starts_with('-')
                    && !l.ends_with('-')
                    && l.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
            })
            // A name whose last label is all digits is an IP number, not a domain.
            && !labels.last().is_some_and(|l| l.bytes().all(|b| b.is_ascii_digit()));
        (ok_local && ok_domain).then(|| Target { local: local.to_string(), domain })
    }

    pub fn address(&self) -> String {
        format!("{}@{}", self.local.to_ascii_lowercase(), self.domain)
    }
}

/// The z-base-32 SHA-1 of the lower-case local part: the file name in the directory.
pub fn hash(local: &str) -> String {
    let digest = Sha1::digest(local.to_ascii_lowercase().as_bytes());
    let mut out = String::with_capacity(32);
    let (mut buffer, mut bits) = (0u32, 0u32);
    for byte in digest {
        buffer = (buffer << 8) | u32::from(byte);
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            out.push(ZBASE32[((buffer >> bits) & 31) as usize] as char);
        }
    }
    out
}

fn query(local: &str) -> String {
    let mut out = String::new();
    for b in local.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// The two places a key can be: the advanced method (a host of its own) first, then the direct one.
pub fn urls(target: &Target) -> [String; 2] {
    let (hash, local, domain) = (hash(&target.local), query(&target.local), &target.domain);
    [
        format!("https://openpgpkey.{domain}/.well-known/openpgpkey/{domain}/hu/{hash}?l={local}"),
        format!("https://{domain}/.well-known/openpgpkey/hu/{hash}?l={local}"),
    ]
}

/// The host of a URL made by [`urls`], for telling the user where was asked.
pub fn host_of(url: &str) -> &str {
    url.strip_prefix("https://").and_then(|rest| rest.split('/').next()).unwrap_or(url)
}

/// What a lookup found.
pub struct Found {
    /// The keys with a user id for the address that can encrypt, and the host that served them.
    pub keys: Vec<SignedPublicKey>,
    pub host: String,
    /// Every host that was asked, in order.
    pub asked: Vec<String>,
}

/// Why nothing was found, in words for the user.
pub fn nothing_text(target: &Target, asked: &[String]) -> String {
    format!(
        "No key for {} was found at {}. Their provider may not publish keys this way: ask them to send you their public key.",
        target.address(),
        asked.join(" or ")
    )
}

/// Looks `target` up. `fetch` gets a URL and answers the bytes, `Ok(None)` for "no such file", or an error for
/// anything else (it must not follow redirects). The advanced method goes first; when it gives no key the direct
/// one is asked. Only a key that names the address and can encrypt is returned.
pub async fn lookup<F, Fut>(target: &Target, fetch: F, now: i64) -> Found
where
    F: Fn(String) -> Fut,
    Fut: Future<Output = Result<Option<Vec<u8>>, String>>,
{
    let wanted = target.address();
    let mut asked = Vec::new();
    for url in urls(target) {
        asked.push(host_of(&url).to_string());
        let Ok(Some(bytes)) = fetch(url.clone()).await else { continue };
        let Ok(parsed) = openpgp::parse_public(&bytes) else { continue };
        let keys: Vec<SignedPublicKey> = parsed
            .into_iter()
            .filter(|k| openpgp::info(k).emails.iter().any(|e| e.eq_ignore_ascii_case(&wanted)))
            .filter(|k| super::build::pgp_can_encrypt(k, now))
            .collect();
        if !keys.is_empty() {
            return Found { keys, host: host_of(&url).to_string(), asked };
        }
    }
    Found { keys: Vec::new(), host: String::new(), asked }
}

/// One request. `https_only` is true outside the tests. A redirect, a wrong status or an answer over the limit is
/// an error; 404 and 410 are `Ok(None)`.
pub async fn fetch(url: &str, https_only: bool) -> Result<Option<Vec<u8>>, String> {
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .https_only(https_only)
        .timeout(std::time::Duration::from_secs(15))
        .connect_timeout(std::time::Duration::from_secs(8))
        .user_agent("kweb-mail")
        .build()
        .map_err(|_| "no connection could be made".to_string())?;
    let mut response = client.get(url).send().await.map_err(|_| "the server could not be reached".to_string())?;
    let status = response.status();
    if status == reqwest::StatusCode::NOT_FOUND || status == reqwest::StatusCode::GONE {
        return Ok(None);
    }
    if status.is_redirection() {
        return Err("the server sent the request somewhere else, which is not followed".into());
    }
    if !status.is_success() {
        return Err(format!("the server answered {}", status.as_u16()));
    }
    if response.content_length().is_some_and(|n| n > MAX_ANSWER as u64) {
        return Err("the answer is too large to be a key".into());
    }
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| "the answer was cut off".to_string())? {
        if body.len() + chunk.len() > MAX_ANSWER {
            return Err("the answer is too large to be a key".into());
        }
        body.extend_from_slice(&chunk);
    }
    Ok(Some(body))
}

#[cfg(test)]
mod tests {
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    };

    use super::*;

    fn fx(name: &str) -> Vec<u8> {
        std::fs::read(format!("{}/tests/fixtures/crypto/{name}", env!("CARGO_MANIFEST_DIR"))).unwrap()
    }

    #[test]
    fn the_file_name_is_the_one_the_specification_gives() {
        // The example of the specification: Joe.Doe@Example.ORG.
        assert_eq!(hash("Joe.Doe"), "iy9q119eutrkn8s1mk4r39qejnbu3n5q");
        let t = Target::parse("Joe.Doe@Example.ORG").unwrap();
        let [advanced, direct] = urls(&t);
        assert_eq!(
            advanced,
            "https://openpgpkey.example.org/.well-known/openpgpkey/example.org/hu/iy9q119eutrkn8s1mk4r39qejnbu3n5q?l=Joe.Doe"
        );
        assert_eq!(direct, "https://example.org/.well-known/openpgpkey/hu/iy9q119eutrkn8s1mk4r39qejnbu3n5q?l=Joe.Doe");
        assert_eq!(host_of(&advanced), "openpgpkey.example.org");
    }

    #[test]
    fn only_plain_internet_addresses_are_looked_up() {
        for bad in [
            "a@localhost",
            "a@127.0.0.1",
            "a@[::1]",
            "a@10.0.0.1",
            "a@b.c d",
            "@x.org",
            "a@x..org",
            "é@x.org",
            "a@-x.org",
            "plain",
        ] {
            assert!(Target::parse(bad).is_none(), "{bad}");
        }
        assert!(Target::parse("a+b@sub.example.org").is_some());
        assert_eq!(query("a+b"), "a%2Bb");
    }

    #[test]
    fn the_advanced_method_goes_first_then_the_direct_one_and_only_a_key_for_the_address_counts() {
        let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
        let t = Target::parse("bob@example.test").unwrap();
        let (bob, alice) = (fx("bob.pub.asc"), fx("alice.pub.asc"));
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64;
        let seen = std::sync::Mutex::new(Vec::new());
        // The advanced host has nothing; the direct one has Bob's key.
        let found = rt.block_on(lookup(
            &t,
            |url| {
                seen.lock().unwrap().push(url.clone());
                let bob = bob.clone();
                async move {
                    if url.starts_with("https://example.test/") {
                        Ok(Some(bob))
                    } else {
                        Ok(None)
                    }
                }
            },
            now,
        ));
        assert_eq!(found.keys.len(), 1);
        assert_eq!(found.host, "example.test");
        assert_eq!(found.asked, ["openpgpkey.example.test", "example.test"]);
        assert!(seen.lock().unwrap()[0].starts_with("https://openpgpkey.example.test/"));
        // A directory that serves somebody else's key is not believed.
        let wrong = rt.block_on(lookup(
            &t,
            |_| {
                let a = alice.clone();
                async move { Ok(Some(a)) }
            },
            now,
        ));
        assert!(wrong.keys.is_empty());
        let junk = rt.block_on(lookup(&t, |_| async { Ok(Some(b"<html>nope</html>".to_vec())) }, now));
        assert!(junk.keys.is_empty());
        let failing = rt.block_on(lookup(&t, |_| async { Err("down".to_string()) }, now));
        assert!(failing.keys.is_empty() && failing.asked.len() == 2);
        assert!(nothing_text(&t, &failing.asked).contains("openpgpkey.example.test or example.test"));
    }

    /// A server that answers one request with `response` and says what it was asked.
    async fn serve(response: Vec<u8>) -> (String, tokio::task::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/x", listener.local_addr().unwrap());
        let task = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buf = vec![0u8; 4096];
            let n = socket.read(&mut buf).await.unwrap();
            let _ = socket.write_all(&response).await;
            let _ = socket.shutdown().await;
            String::from_utf8_lossy(&buf[..n]).into_owned()
        });
        (url, task)
    }

    #[tokio::test]
    async fn a_redirect_is_not_followed_and_nothing_but_a_plain_request_is_sent() {
        let (url, asked) = serve(
            b"HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1:1/elsewhere\r\nContent-Length: 0\r\n\r\n".to_vec(),
        )
        .await;
        let result = fetch(&url, false).await;
        assert!(result.unwrap_err().contains("not followed"));
        let request = asked.await.unwrap().to_ascii_lowercase();
        assert!(request.starts_with("get /x "));
        assert!(!request.contains("authorization") && !request.contains("cookie"), "{request}");
    }

    #[tokio::test]
    async fn a_small_answer_is_taken_and_a_large_or_missing_one_is_not() {
        let (url, _) = serve(b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\nConnection: close\r\n\r\nhello".to_vec()).await;
        assert_eq!(fetch(&url, false).await.unwrap().unwrap(), b"hello");
        let (url, _) = serve(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n".to_vec()).await;
        assert_eq!(fetch(&url, false).await.unwrap(), None);
        let mut big =
            format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", MAX_ANSWER + 1).into_bytes();
        big.extend(vec![b'a'; MAX_ANSWER + 1]);
        let (url, _) = serve(big).await;
        assert!(fetch(&url, false).await.unwrap_err().contains("too large"));
        let (url, _) = serve(b"HTTP/1.1 500 Oops\r\nContent-Length: 0\r\n\r\n".to_vec()).await;
        assert!(fetch(&url, false).await.is_err());
    }

    #[tokio::test]
    async fn a_plain_http_address_is_refused_when_https_is_required() {
        assert!(fetch("http://127.0.0.1:9/x", true).await.is_err());
    }
}
