//! Leaving a mailing list: reading the List-Unsubscribe headers, and the one-click request.
//!
//! The headers come from the server as a list of URLs (`header:List-Unsubscribe:asURLs`) and the
//! text of List-Unsubscribe-Post. What a list can be left by is worked out by pure functions,
//! and so is the request; sending it is a thin layer on top, so the rules can be tested
//! without a server.
//!
//! The one-click request (RFC 8058) is the only request the core makes to an address a mail
//! gave it, so it is made as bare as possible: https only, a POST with the one fixed body, no
//! redirect followed, no cookie, none of the user's headers, a time limit, and only to a public
//! address (a mail must not make the phone ask something on the user's own network).

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::Duration;

use url::{Host, Url};

use crate::{assistant::settings::is_private, compose::rules::valid_address, model::Unsubscribe, Error, Result};

/// The List-Unsubscribe-Post value that says the list takes the one-click request.
const ONE_CLICK_POST: &str = "List-Unsubscribe=One-Click";
/// The body of the one-click request.
pub const ONE_CLICK_BODY: &str = ONE_CLICK_POST;
const REQUEST_LIMIT: Duration = Duration::from_secs(30);
const LOOKUP_LIMIT: Duration = Duration::from_secs(10);
/// Longest address of a list taken at all.
const URL_MAX: usize = 2048;
const ADDRESS_MAX: usize = 254;
const SUBJECT_MAX_CHARS: usize = 200;
const BODY_MAX_CHARS: usize = 1000;
const DEFAULT_SUBJECT: &str = "unsubscribe";

/// The message to send when a list is left by mail.
#[derive(Debug, Clone, PartialEq)]
pub struct MailRequest {
    pub to: String,
    pub subject: String,
    pub body: String,
}

/// What the headers of one message offer.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Options {
    /// The https address of the one-click request: there when the list says it takes one.
    pub one_click: Option<String>,
    /// The first https address.
    pub web: Option<String>,
    pub mail: Option<MailRequest>,
}

impl Options {
    /// For the host; `None` when there is nothing to offer.
    pub fn info(&self) -> Option<Unsubscribe> {
        if self.web.is_none() && self.mail.is_none() {
            return None;
        }
        Some(Unsubscribe {
            one_click: self.one_click.is_some(),
            by_mail: self.mail.is_some(),
            web_url: self.web.clone(),
        })
    }
}

/// Reads what `urls` (List-Unsubscribe as the server gives it, brackets taken off) and `post`
/// (List-Unsubscribe-Post) offer. `http` addresses and every other kind are ignored.
pub fn read(urls: &[String], post: Option<&str>) -> Options {
    let web = urls.iter().map(|u| u.trim()).find(|u| https_url(u).is_some()).map(str::to_string);
    let mail = urls.iter().find_map(|u| mailto(u.trim()));
    let one_click = web.clone().filter(|_| post.is_some_and(|p| p.trim() == ONE_CLICK_POST));
    Options { one_click, web, mail }
}

/// The address when it is one the core may talk to: https, a host, no name or password.
fn https_url(text: &str) -> Option<Url> {
    if text.len() > URL_MAX {
        return None;
    }
    let url = Url::parse(text).ok()?;
    let plain =
        url.scheme() == "https" && url.host().is_some() && url.username().is_empty() && url.password().is_none();
    plain.then_some(url)
}

/// The request by mail a `mailto:` address describes: the first address (a list that names more
/// gets one message), its subject and its text, cut to a sane length. Any other field (Cc, Bcc,
/// In-Reply-To) is ignored.
fn mailto(text: &str) -> Option<MailRequest> {
    let rest = text.get(..7).filter(|p| p.eq_ignore_ascii_case("mailto:")).map(|_| &text[7..])?;
    let (addresses, query) = rest.split_once('?').unwrap_or((rest, ""));
    let to = decode(addresses.split(',').next().unwrap_or_default()).trim().to_string();
    if to.len() > ADDRESS_MAX || !valid_address(&to) {
        return None;
    }
    let (mut subject, mut body) = (None, None);
    for field in query.split('&') {
        let (name, value) = field.split_once('=').unwrap_or((field, ""));
        match decode(name).to_ascii_lowercase().as_str() {
            "subject" if subject.is_none() => subject = Some(decode(value)),
            "body" if body.is_none() => body = Some(decode(value)),
            _ => {}
        }
    }
    let subject =
        subject.map(|s| one_line(&s)).filter(|s| !s.is_empty()).unwrap_or_else(|| DEFAULT_SUBJECT.to_string());
    let body = body.map(|b| lines(&b)).unwrap_or_default();
    Some(MailRequest {
        to,
        subject: subject.chars().take(SUBJECT_MAX_CHARS).collect(),
        body: body.chars().take(BODY_MAX_CHARS).collect(),
    })
}

/// Percent-decoding (RFC 3986): `+` stays a plus; bytes that are not UTF-8 become U+FFFD.
fn decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = |b: u8| (b as char).to_digit(16);
        match (bytes[i], bytes.get(i + 1).and_then(|b| hex(*b)), bytes.get(i + 2).and_then(|b| hex(*b))) {
            (b'%', Some(hi), Some(lo)) => {
                out.push((hi * 16 + lo) as u8);
                i += 3;
            }
            (b, _, _) => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// A subject is one line: line breaks and tabs become spaces, the ends are trimmed.
fn one_line(text: &str) -> String {
    text.chars().map(|c| if c.is_control() { ' ' } else { c }).collect::<String>().trim().to_string()
}

/// A text with `\n` line ends and no control characters besides them.
fn lines(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n").chars().filter(|c| *c == '\n' || !c.is_control()).collect()
}

// ---- who may be asked ----

/// Whether this address belongs to the public internet: not this device, not a private or
/// link-local network, not a range that is not for hosts.
pub fn ip_is_public(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(a) => v4_is_public(a),
        IpAddr::V6(a) => {
            let first = a.segments()[0];
            let mapped = a.to_ipv4_mapped().is_some_and(|v4| !v4_is_public(v4));
            !(a.is_loopback()
                || a.is_unspecified()
                || a.is_multicast()
                || first & 0xfe00 == 0xfc00
                || first & 0xffc0 == 0xfe80
                || mapped)
        }
    }
}

fn v4_is_public(a: Ipv4Addr) -> bool {
    let [first, second, ..] = a.octets();
    let shared = first == 100 && (64..128).contains(&second);
    !(a.is_loopback()
        || a.is_private()
        || a.is_link_local()
        || a.is_unspecified()
        || a.is_broadcast()
        || a.is_multicast()
        || first == 0
        || shared)
}

/// What the address itself says: a public IP, or a name that is not this device's or a private
/// network's (`localhost`, `.local`, a name without a dot). A name can still point somewhere
/// private; [`post_one_click`] checks what it resolves to.
pub fn host_is_public(url: &Url) -> bool {
    match url.host() {
        Some(Host::Domain(name)) => {
            let bare = name.trim_end_matches('.');
            bare.contains('.') && !is_private(&Host::Domain(name)) && !bare.to_ascii_lowercase().ends_with(".internal")
        }
        Some(Host::Ipv4(a)) => ip_is_public(IpAddr::V4(a)),
        Some(Host::Ipv6(a)) => ip_is_public(IpAddr::V6(a)),
        None => false,
    }
}

// ---- the request ----

/// The client of the one-click request. https only; no redirect is followed; no cookies; no
/// default header; a 30 second limit. `pinned` are the addresses the host was checked to have:
/// the request goes to them and to no other.
pub fn one_click_client(host: &str, pinned: &[SocketAddr]) -> reqwest::Result<reqwest::Client> {
    let mut builder = reqwest::Client::builder()
        .https_only(true)
        .redirect(reqwest::redirect::Policy::none())
        .referer(false)
        // A proxy named in the environment would resolve the host itself and ignore the pin.
        .no_proxy()
        .timeout(REQUEST_LIMIT);
    if !pinned.is_empty() {
        builder = builder.resolve_to_addrs(host, pinned);
    }
    builder.build()
}

/// The request: a POST of [`ONE_CLICK_BODY`] as a form, and nothing else.
pub fn one_click_request(client: &reqwest::Client, url: &Url) -> reqwest::Result<reqwest::Request> {
    client
        .post(url.clone())
        .header(reqwest::header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(ONE_CLICK_BODY)
        .build()
}

/// What [`post_one_click`] did.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Posted {
    /// The list answered with a success (any 2xx).
    Done,
    /// The address is on this device or a private network (or its name leads there): nothing
    /// was sent.
    NotAllowed,
}

/// Does the one-click request at `address`. A list that does not answer with a 2xx (a redirect
/// is no 2xx) or cannot be reached is `Error::Temporary`; its address is not repeated in the
/// error, it may carry the user's identity.
pub async fn post_one_click(address: &str) -> Result<Posted> {
    let url = https_url(address).ok_or_else(|| Error::invalid("The list's address is not usable."))?;
    if !host_is_public(&url) {
        return Ok(Posted::NotAllowed);
    }
    let (host, port) = (url.host_str().unwrap_or_default().to_string(), url.port_or_known_default().unwrap_or(443));
    let pinned: Vec<SocketAddr> = match url.host() {
        Some(Host::Domain(name)) => {
            let found = tokio::time::timeout(LOOKUP_LIMIT, tokio::net::lookup_host((name, port))).await;
            let found: Vec<SocketAddr> = match found {
                Ok(Ok(found)) => found.collect(),
                _ => return Err(Error::temporary(format!("The list's server ({host}) could not be found."))),
            };
            if found.is_empty() {
                return Err(Error::temporary(format!("The list's server ({host}) could not be found.")));
            }
            if found.iter().any(|a| !ip_is_public(a.ip())) {
                return Ok(Posted::NotAllowed);
            }
            found
        }
        _ => Vec::new(),
    };
    let unreachable =
        |why: &dyn std::fmt::Display| Error::temporary(format!("The list's server ({host}) did not answer: {why}"));
    let client = one_click_client(&host, &pinned).map_err(|e| unreachable(&e))?;
    let request = one_click_request(&client, &url).map_err(|e| unreachable(&e))?;
    let answer = client.execute(request).await.map_err(|e| unreachable(&e.without_url()))?;
    let status = answer.status();
    if status.is_success() {
        Ok(Posted::Done)
    } else {
        Err(Error::temporary(format!("The list's server ({host}) did not accept it: HTTP {}.", status.as_u16())))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn urls(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn one_click_needs_the_exact_post_header_and_an_https_address() {
        let both =
            urls(&["https://lists.example.org/u/abc?x=1&y=2", "mailto:unsub@lists.example.org?subject=unsubscribe"]);
        let o = read(&both, Some("List-Unsubscribe=One-Click"));
        let info = o.info().unwrap();
        assert!(info.one_click && info.by_mail);
        assert_eq!(info.web_url.as_deref(), Some("https://lists.example.org/u/abc?x=1&y=2"));
        assert_eq!(o.one_click.as_deref(), Some("https://lists.example.org/u/abc?x=1&y=2"));
        // The header with the folding space the server leaves in is the same header.
        assert!(read(&both, Some(" List-Unsubscribe=One-Click")).info().unwrap().one_click);
        for post in
            [None, Some(""), Some("list-unsubscribe=one-click"), Some("List-Unsubscribe=One-Click; x"), Some("Other")]
        {
            let info = read(&both, post).info().unwrap();
            assert!(!info.one_click, "{post:?}");
            assert!(info.by_mail && info.web_url.is_some());
        }
        // No https address, no one-click.
        let info = read(&urls(&["mailto:u@lists.example.org"]), Some("List-Unsubscribe=One-Click")).info().unwrap();
        assert!(!info.one_click && info.by_mail && info.web_url.is_none());
    }

    #[test]
    fn http_and_odd_addresses_are_ignored() {
        assert_eq!(read(&urls(&["http://lists.example.org/u"]), Some("List-Unsubscribe=One-Click")).info(), None);
        assert_eq!(read(&urls(&["ftp://x.org/u", "javascript:alert(1)", "not a url", ""]), None).info(), None);
        assert_eq!(read(&[], None).info(), None);
        // The first https address, whatever comes before it.
        let o = read(&urls(&["http://a.org/", "https://b.org/first", "https://c.org/second"]), None);
        assert_eq!(o.web.as_deref(), Some("https://b.org/first"));
        // A name and password in the address: not used.
        assert_eq!(read(&urls(&["https://user:pw@lists.example.org/u"]), None).info(), None);
        assert_eq!(read(&urls(&[&format!("https://x.org/{}", "a".repeat(3000))]), None).info(), None);
        assert!(read(&urls(&["HTTPS://Lists.Example.org/u"]), None).web.is_some());
    }

    #[test]
    fn a_mailto_gives_the_address_the_subject_and_the_body() {
        let o =
            read(&urls(&["mailto:leave@lists.example.org?subject=Unsub%20me&body=Please%0D%0Aremove%20me%21"]), None);
        let mail = o.mail.unwrap();
        assert_eq!(mail.to, "leave@lists.example.org");
        assert_eq!(mail.subject, "Unsub me");
        assert_eq!(mail.body, "Please\nremove me!");
        // Without them: a subject of its own and no text.
        let mail = read(&urls(&["MAILTO:u@lists.example.org"]), None).mail.unwrap();
        assert_eq!((mail.subject.as_str(), mail.body.as_str()), ("unsubscribe", ""));
        // Only the first address, nothing but subject and body of the rest, a plus stays a plus.
        let mail = read(&urls(&["mailto:a@x.org,b@y.org?cc=c@z.org&bcc=d@z.org&subject=a+b&SUBJECT=second"]), None)
            .mail
            .unwrap();
        assert_eq!((mail.to.as_str(), mail.subject.as_str()), ("a@x.org", "a+b"));
        assert_eq!(read(&urls(&["mailto:%75@x.org"]), None).mail.unwrap().to, "u@x.org");
    }

    #[test]
    fn a_mailto_is_cut_and_kept_to_one_line() {
        let long = "x".repeat(500);
        let mail =
            read(&urls(&[&format!("mailto:u@x.org?subject={long}&body={long}{long}{long}")]), None).mail.unwrap();
        assert_eq!(mail.subject.chars().count(), 200);
        assert_eq!(mail.body.chars().count(), 1000);
        let mail = read(&urls(&["mailto:u@x.org?subject=a%0D%0ABcc:%20evil@x.org%09b"]), None).mail.unwrap();
        assert_eq!(mail.subject, "a  Bcc: evil@x.org b");
        let mail = read(&urls(&["mailto:u@x.org?subject=%00%0A&body=a%00b"]), None).mail.unwrap();
        assert_eq!((mail.subject.as_str(), mail.body.as_str()), ("unsubscribe", "ab"));
        // Not an address: the next mailto is the one, or none.
        assert_eq!(read(&urls(&["mailto:nobody", "mailto:u@x.org"]), None).mail.unwrap().to, "u@x.org");
        assert_eq!(read(&urls(&["mailto:", "mailto:a b@x.org", "mailto:?subject=x"]), None).info(), None);
        assert_eq!(read(&urls(&[&format!("mailto:{}@x.org", "a".repeat(260))]), None).info(), None);
    }

    #[test]
    fn only_public_addresses_may_be_asked() {
        let public = |s: &str| host_is_public(&Url::parse(s).unwrap());
        assert!(public("https://lists.example.org/u"));
        assert!(public("https://8.8.8.8/u") && public("https://[2606:4700::1111]/u"));
        for no in [
            "https://localhost/u",
            "https://app.localhost/u",
            "https://127.0.0.1/u",
            "https://127.1.2.3:8443/u",
            "https://10.0.0.5/u",
            "https://172.16.0.1/u",
            "https://192.168.1.1/u",
            "https://169.254.169.254/u",
            "https://100.64.0.1/u",
            "https://0.0.0.0/u",
            "https://224.0.0.1/u",
            "https://[::1]/u",
            "https://[fe80::1]/u",
            "https://[fd00::1]/u",
            "https://[::ffff:127.0.0.1]/u",
            "https://[::ffff:10.0.0.1]/u",
            "https://printer.local/u",
            "https://intranet/u",
            "https://nas.internal/u",
        ] {
            assert!(!public(no), "{no}");
        }
        assert!(ip_is_public("1.1.1.1".parse().unwrap()) && !ip_is_public("192.168.0.2".parse().unwrap()));
    }

    #[test]
    fn the_request_is_a_bare_form_post() {
        let url = Url::parse("https://lists.example.org/u/abc?x=1").unwrap();
        let client = one_click_client("lists.example.org", &[]).unwrap();
        let request = one_click_request(&client, &url).unwrap();
        assert_eq!(request.method(), reqwest::Method::POST);
        assert_eq!(request.url().as_str(), "https://lists.example.org/u/abc?x=1");
        assert_eq!(request.body().and_then(|b| b.as_bytes()), Some(b"List-Unsubscribe=One-Click".as_slice()));
        let names: Vec<&str> = request.headers().keys().map(|k| k.as_str()).collect();
        assert_eq!(names, ["content-type"]);
        assert_eq!(request.headers()["content-type"], "application/x-www-form-urlencoded");
    }

    #[tokio::test]
    async fn a_private_address_is_not_asked() {
        for address in ["https://127.0.0.1:9/u", "https://localhost/u", "https://[::1]/u", "https://192.168.1.1/u"] {
            assert_eq!(post_one_click(address).await.unwrap(), Posted::NotAllowed, "{address}");
        }
        // Not https: not usable at all.
        assert!(matches!(post_one_click("http://lists.example.org/u").await, Err(Error::Invalid { .. })));
        assert!(matches!(post_one_click("mailto:a@b.org").await, Err(Error::Invalid { .. })));
    }
}
