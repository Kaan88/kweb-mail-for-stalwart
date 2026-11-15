//! A small MIME reader that keeps the exact byte ranges of every part.
//!
//! A signature covers bytes, not text: the first part of a `multipart/signed` message has to be
//! checked as it was transmitted (only the line ends are made CRLF). So this reader never
//! rebuilds anything; it finds where each part starts and ends in the raw message and hands out
//! those ranges. Decoding a part's text (charsets, HTML and the like) is left to `mail-parser`.
//!
//! The reader is bounded (depth, number of parts) because the message is somebody else's.

use std::{borrow::Cow, collections::HashMap, ops::Range};

use base64::{engine::general_purpose::STANDARD, Engine};

/// How deep parts may nest, and how many parts a message may have, before the reader gives up.
const MAX_DEPTH: usize = 8;
const MAX_PARTS: usize = 300;

/// A parsed `Content-Type`: the lowercase media type and the parameters (names lowercase).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ContentType {
    pub media: String,
    pub params: HashMap<String, String>,
}

impl ContentType {
    pub fn parse(value: &str) -> ContentType {
        let mut pieces = split_params(value).into_iter();
        let media = pieces.next().unwrap_or_default().trim().to_ascii_lowercase();
        let mut params = HashMap::new();
        for piece in pieces {
            if let Some((name, value)) = piece.split_once('=') {
                let value = value.trim();
                let value = match value.strip_prefix('"') {
                    Some(quoted) => unquote(quoted),
                    None => value.to_string(),
                };
                params.entry(name.trim().to_ascii_lowercase()).or_insert(value);
            }
        }
        ContentType { media, params }
    }

    pub fn param(&self, name: &str) -> Option<&str> {
        self.params.get(name).map(String::as_str)
    }

    pub fn is(&self, media: &str) -> bool {
        self.media == media
    }
}

/// Splits at `;` outside quotes.
fn split_params(value: &str) -> Vec<String> {
    let (mut out, mut current, mut quoted, mut escaped) = (Vec::new(), String::new(), false, false);
    for c in value.chars() {
        match c {
            _ if escaped => {
                current.push(c);
                escaped = false;
            }
            '\\' if quoted => {
                current.push(c);
                escaped = true;
            }
            '"' => {
                quoted = !quoted;
                current.push(c);
            }
            ';' if !quoted => out.push(std::mem::take(&mut current)),
            _ => current.push(c),
        }
    }
    out.push(current);
    out
}

/// The text of a quoted string whose opening quote is already gone, up to the closing one.
fn unquote(rest: &str) -> String {
    let (mut out, mut escaped) = (String::new(), false);
    for c in rest.chars() {
        match c {
            _ if escaped => {
                out.push(c);
                escaped = false;
            }
            '\\' => escaped = true,
            '"' => break,
            _ => out.push(c),
        }
    }
    out
}

/// One part of a message, with where it is in the raw bytes.
#[derive(Debug, Clone)]
pub struct Node {
    /// The whole part: its headers, the blank line and its body, as transmitted.
    pub whole: Range<usize>,
    /// Just the body.
    pub body: Range<usize>,
    /// Header names in lowercase, values unfolded and trimmed (not decoded).
    pub headers: Vec<(String, String)>,
    pub ctype: ContentType,
    pub children: Vec<Node>,
}

impl Node {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(n, _)| n == name).map(|(_, v)| v.as_str())
    }

    /// The `Content-Transfer-Encoding`, lowercase; `7bit` when there is none.
    pub fn encoding(&self) -> String {
        self.header("content-transfer-encoding").map(|v| v.trim().to_ascii_lowercase()).unwrap_or_else(|| "7bit".into())
    }

    /// The body bytes with the transfer encoding (base64, quoted-printable) removed.
    pub fn decoded_body(&self, raw: &[u8]) -> Vec<u8> {
        decode_transfer(&raw[self.body.clone()], &self.encoding())
    }

    /// Every node of this subtree, this one first, parents before children.
    pub fn walk<'a>(&'a self, out: &mut Vec<&'a Node>) {
        out.push(self);
        for child in &self.children {
            child.walk(out);
        }
    }
}

/// Parses the message (or a part taken out of it: `raw` is then that part alone). `None` when
/// it does not look like a message at all (no header lines).
pub fn parse(raw: &[u8]) -> Option<Node> {
    let mut parts = 0;
    parse_entity(raw, 0..raw.len(), 0, &mut parts)
}

fn parse_entity(raw: &[u8], span: Range<usize>, depth: usize, parts: &mut usize) -> Option<Node> {
    *parts += 1;
    if *parts > MAX_PARTS {
        return None;
    }
    let bytes = &raw[span.clone()];
    let (header_len, body_off) = split_header(bytes);
    let headers = parse_headers(&bytes[..header_len]);
    let ctype = headers
        .iter()
        .find(|(n, _)| n == "content-type")
        .map(|(_, v)| ContentType::parse(v))
        .filter(|c| c.media.contains('/'))
        .unwrap_or_else(|| ContentType { media: "text/plain".into(), params: HashMap::new() });
    let body = span.start + body_off..span.end;
    let mut node = Node { whole: span, body: body.clone(), headers, ctype, children: Vec::new() };
    if node.ctype.media.starts_with("multipart/") && depth < MAX_DEPTH {
        if let Some(boundary) = node.ctype.param("boundary").filter(|b| !b.is_empty()) {
            for range in split_multipart(raw, body, boundary) {
                node.children.push(parse_entity(raw, range, depth + 1, parts)?);
            }
        }
    }
    Some(node)
}

/// Where the headers end and the body starts: `(length of the header block, offset of the body)`.
/// A part that starts with a blank line has no headers.
fn split_header(bytes: &[u8]) -> (usize, usize) {
    if bytes.starts_with(b"\r\n") {
        return (0, 2);
    }
    if bytes.starts_with(b"\n") {
        return (0, 1);
    }
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\n' {
            let next = i + 1;
            if bytes[next..].starts_with(b"\r\n") {
                return (i + 1, next + 2);
            }
            if bytes[next..].starts_with(b"\n") {
                return (i + 1, next + 1);
            }
        }
        i += 1;
    }
    (bytes.len(), bytes.len())
}

fn parse_headers(block: &[u8]) -> Vec<(String, String)> {
    let text = String::from_utf8_lossy(block);
    let mut out: Vec<(String, String)> = Vec::new();
    for line in text.split('\n') {
        let line = line.strip_suffix('\r').unwrap_or(line);
        if line.starts_with([' ', '\t']) {
            if let Some((_, value)) = out.last_mut() {
                value.push(' ');
                value.push_str(line.trim());
            }
        } else if let Some((name, value)) = line.split_once(':') {
            out.push((name.trim().to_ascii_lowercase(), value.trim().to_string()));
        }
    }
    out
}

/// The ranges of the parts between the boundary lines of a multipart body. A part starts after
/// the line ending that ends its delimiter line and stops before the line ending that belongs to
/// the next delimiter, which is exactly the byte range a signature over it covers.
fn split_multipart(raw: &[u8], body: Range<usize>, boundary: &str) -> Vec<Range<usize>> {
    let delimiter = [b"--", boundary.as_bytes()].concat();
    // (start of the delimiter line, offset after its line ending, is the closing delimiter)
    let mut delimiters: Vec<(usize, usize, bool)> = Vec::new();
    let mut line_start = body.start;
    while line_start < body.end {
        let line_end = raw[line_start..body.end].iter().position(|b| *b == b'\n').map_or(body.end, |p| line_start + p);
        let next = (line_end + 1).min(body.end);
        let line = &raw[line_start..line_end];
        if line.starts_with(&delimiter) {
            let after = &line[delimiter.len()..];
            let closing = after.starts_with(b"--");
            let tail = if closing { &after[2..] } else { after };
            if tail.iter().all(|b| matches!(b, b' ' | b'\t' | b'\r')) {
                delimiters.push((line_start, next, closing));
                if closing {
                    break;
                }
            }
        }
        line_start = next;
    }
    let mut ranges = Vec::new();
    for pair in delimiters.windows(2) {
        let (start, mut end) = (pair[0].1, pair[1].0);
        if pair[0].2 {
            break;
        }
        // The line ending before the next delimiter belongs to the delimiter.
        if end > start && raw[end - 1] == b'\n' {
            end -= 1;
            if end > start && raw[end - 1] == b'\r' {
                end -= 1;
            }
        }
        ranges.push(start..end);
    }
    ranges
}

/// The bytes with every line ending made CRLF (what signing programs hash for mail).
pub fn canonical_crlf(bytes: &[u8]) -> Cow<'_, [u8]> {
    let needs = bytes.iter().enumerate().any(|(i, b)| *b == b'\n' && (i == 0 || bytes[i - 1] != b'\r'));
    if !needs {
        return Cow::Borrowed(bytes);
    }
    let mut out = Vec::with_capacity(bytes.len() + 16);
    for (i, b) in bytes.iter().enumerate() {
        if *b == b'\n' && (i == 0 || bytes[i - 1] != b'\r') {
            out.push(b'\r');
        }
        out.push(*b);
    }
    Cow::Owned(out)
}

/// Removes a transfer encoding. Anything not base64 or quoted-printable is returned as is.
pub fn decode_transfer(body: &[u8], encoding: &str) -> Vec<u8> {
    match encoding {
        "base64" => {
            let clean: Vec<u8> = body.iter().copied().filter(|b| !b.is_ascii_whitespace() && *b != b'=').collect();
            base64::engine::general_purpose::STANDARD_NO_PAD.decode(clean).unwrap_or_default()
        }
        "quoted-printable" => decode_qp(body),
        _ => body.to_vec(),
    }
}

fn decode_qp(body: &[u8]) -> Vec<u8> {
    let hex = |b: u8| (b as char).to_digit(16).map(|d| d as u8);
    let mut out = Vec::with_capacity(body.len());
    let mut i = 0;
    while i < body.len() {
        if body[i] == b'=' {
            // A soft line break.
            if body[i + 1..].starts_with(b"\r\n") {
                i += 3;
                continue;
            }
            if body[i + 1..].starts_with(b"\n") {
                i += 2;
                continue;
            }
            if let (Some(hi), Some(lo)) = (body.get(i + 1).and_then(|b| hex(*b)), body.get(i + 2).and_then(|b| hex(*b)))
            {
                out.push(hi * 16 + lo);
                i += 3;
                continue;
            }
        }
        out.push(body[i]);
        i += 1;
    }
    out
}

/// The bytes of a base64 body (PEM-like) for the standard alphabet, with padding fixed.
pub fn base64_lenient(text: &[u8]) -> Option<Vec<u8>> {
    let clean: Vec<u8> = text.iter().copied().filter(|b| !b.is_ascii_whitespace()).collect();
    STANDARD.decode(&clean).ok().or_else(|| {
        let trimmed: Vec<u8> = clean.iter().copied().filter(|b| *b != b'=').collect();
        base64::engine::general_purpose::STANDARD_NO_PAD.decode(trimmed).ok()
    })
}

/// The media types of a message's parts, as the server's structure gives them: enough to tell
/// signed and encrypted mail from other mail without fetching it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TypeTree {
    pub media: String,
    pub children: Vec<TypeTree>,
}

// ---- what kind of cryptographic mail it is ----------------------------------------------------

/// The protocols a signed or encrypted message can use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Protocol {
    Pgp,
    Smime,
}

/// A signed or encrypted entity found in a message.
#[derive(Debug, Clone, PartialEq)]
pub enum Found {
    /// `multipart/signed`: the signed part is `signed`, the signature is `signature`
    /// (indexes into the node's children, which are the two parts).
    Signed { protocol: Protocol },
    /// `multipart/encrypted` (OpenPGP, RFC 3156).
    PgpEncrypted,
    /// `application/pkcs7-mime` (or `.p7m`): signed or enveloped data, told apart by `smime-type`
    /// or, when that is missing, by the data itself.
    Pkcs7 { smime_type: Option<String> },
}

/// Whether a media type is S/MIME opaque data.
pub fn is_pkcs7_mime(media: &str) -> bool {
    matches!(media, "application/pkcs7-mime" | "application/x-pkcs7-mime")
}

pub fn is_pkcs7_signature(media: &str) -> bool {
    matches!(media, "application/pkcs7-signature" | "application/x-pkcs7-signature")
}

/// What a node is, when it is a signed or encrypted one.
pub fn classify(node: &Node) -> Option<Found> {
    let media = node.ctype.media.as_str();
    match media {
        "multipart/signed" if node.children.len() == 2 => {
            let proto = node.ctype.param("protocol").map(str::to_ascii_lowercase).unwrap_or_default();
            let second = node.children[1].ctype.media.as_str();
            if proto == "application/pgp-signature" || second == "application/pgp-signature" {
                Some(Found::Signed { protocol: Protocol::Pgp })
            } else if is_pkcs7_signature(&proto) || is_pkcs7_signature(second) {
                Some(Found::Signed { protocol: Protocol::Smime })
            } else {
                None
            }
        }
        "multipart/encrypted" if node.children.len() == 2 => {
            let proto = node.ctype.param("protocol").map(str::to_ascii_lowercase).unwrap_or_default();
            (proto == "application/pgp-encrypted" || node.children[0].ctype.is("application/pgp-encrypted"))
                .then_some(Found::PgpEncrypted)
        }
        m if is_pkcs7_mime(m) => {
            Some(Found::Pkcs7 { smime_type: node.ctype.param("smime-type").map(str::to_ascii_lowercase) })
        }
        _ => None,
    }
}

/// The first signed or encrypted entity of the message, looking through plain wrappers
/// (`multipart/mixed` and the like) only, and whether it is the whole message. Entities that
/// sit inside another signed or encrypted one are not looked at here: what is inside is
/// handled when the outer one has been opened.
pub fn find(root: &Node) -> Option<(&Node, Found, bool)> {
    let mut all = Vec::new();
    root.walk(&mut all);
    for node in all {
        if let Some(found) = classify(node) {
            let whole = std::ptr::eq(node, root);
            return Some((node, found, whole));
        }
    }
    None
}

// ---- armored blocks in text ----------------------------------------------------------------

/// A block of OpenPGP armor found in a text, as byte offsets into it.
#[derive(Debug, Clone, PartialEq)]
pub struct ArmorBlock {
    pub kind: ArmorKind,
    pub range: Range<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArmorKind {
    Signed,
    Message,
    PublicKey,
}

/// The first armored OpenPGP block of the text that starts at the beginning of a line.
pub fn find_armor(text: &str) -> Option<ArmorBlock> {
    const BLOCKS: [(&str, &str, ArmorKind); 3] = [
        ("-----BEGIN PGP SIGNED MESSAGE-----", "-----END PGP SIGNATURE-----", ArmorKind::Signed),
        ("-----BEGIN PGP MESSAGE-----", "-----END PGP MESSAGE-----", ArmorKind::Message),
        ("-----BEGIN PGP PUBLIC KEY BLOCK-----", "-----END PGP PUBLIC KEY BLOCK-----", ArmorKind::PublicKey),
    ];
    let mut best: Option<ArmorBlock> = None;
    for (begin, end, kind) in BLOCKS {
        let mut from = 0;
        while let Some(at) = text[from..].find(begin).map(|p| p + from) {
            let at_line_start = at == 0 || text.as_bytes()[at - 1] == b'\n';
            if at_line_start {
                if let Some(stop) = text[at..].find(end) {
                    let range = at..at + stop + end.len();
                    if best.as_ref().is_none_or(|b| range.start < b.range.start) {
                        best = Some(ArmorBlock { kind, range });
                    }
                    break;
                }
            }
            from = at + begin.len();
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;

    fn signed_message() -> Vec<u8> {
        b"From: a@x.test\r\nContent-Type: multipart/signed; boundary=\"B\"; protocol=\"application/pgp-signature\"; micalg=pgp-sha256\r\n\r\npreamble\r\n--B\r\nContent-Type: text/plain\r\n\r\nhello \r\nworld\r\n--B\r\nContent-Type: application/pgp-signature\r\n\r\nSIG\r\n--B--\r\nepilogue\r\n".to_vec()
    }

    #[test]
    fn the_first_part_of_a_signed_message_is_found_to_the_byte() {
        let raw = signed_message();
        let root = parse(&raw).unwrap();
        assert_eq!(root.ctype.media, "multipart/signed");
        assert_eq!(root.children.len(), 2);
        // The line ending before the delimiter is not part of the signed bytes; the trailing space is.
        assert_eq!(&raw[root.children[0].whole.clone()], b"Content-Type: text/plain\r\n\r\nhello \r\nworld");
        assert_eq!(&raw[root.children[1].body.clone()], b"SIG");
        assert_eq!(classify(&root), Some(Found::Signed { protocol: Protocol::Pgp }));
    }

    #[test]
    fn bare_line_feeds_are_read_and_can_be_made_crlf() {
        let raw = b"Content-Type: multipart/signed; boundary=B; protocol=\"application/pkcs7-signature\"\n\n--B\nContent-Type: text/plain\n\nhi\n--B\nContent-Type: application/pkcs7-signature\n\nAAAA\n--B--\n";
        let root = parse(raw).unwrap();
        let first = &raw[root.children[0].whole.clone()];
        assert_eq!(first, b"Content-Type: text/plain\n\nhi");
        assert_eq!(canonical_crlf(first).as_ref(), b"Content-Type: text/plain\r\n\r\nhi");
        assert_eq!(classify(&root), Some(Found::Signed { protocol: Protocol::Smime }));
    }

    #[test]
    fn a_delimiter_inside_a_line_is_not_a_delimiter() {
        let raw = b"Content-Type: multipart/mixed; boundary=B\r\n\r\n--B\r\n\r\nsee --B here\r\nand x--B\r\n--B--\r\n";
        let root = parse(raw).unwrap();
        assert_eq!(root.children.len(), 1);
        assert_eq!(&raw[root.children[0].body.clone()], b"see --B here\r\nand x--B");
    }

    #[test]
    fn nesting_and_part_counts_are_bounded() {
        let mut raw = String::new();
        for i in 0..40 {
            raw.push_str(&format!("Content-Type: multipart/mixed; boundary=b{i}\r\n\r\n--b{i}\r\n"));
        }
        raw.push_str("\r\nx");
        assert!(parse(raw.as_bytes()).is_some());
        let mut many = String::from("Content-Type: multipart/mixed; boundary=B\r\n\r\n");
        for _ in 0..400 {
            many.push_str("--B\r\n\r\nx\r\n");
        }
        many.push_str("--B--\r\n");
        assert!(parse(many.as_bytes()).is_none());
    }

    #[test]
    fn content_type_parameters_are_read_quoted_or_not() {
        let c = ContentType::parse("Multipart/Signed; Boundary=\"a;b\\\"c\"; protocol=application/pgp-signature");
        assert_eq!(c.media, "multipart/signed");
        assert_eq!(c.param("boundary"), Some("a;b\"c"));
        assert_eq!(c.param("protocol"), Some("application/pgp-signature"));
    }

    #[test]
    fn transfer_encodings_are_removed() {
        assert_eq!(decode_transfer(b"aGVs\r\nbG8=", "base64"), b"hello");
        assert_eq!(decode_transfer(b"a=3Db=\r\nc", "quoted-printable"), b"a=bc");
        assert_eq!(base64_lenient(b"aGVsbG8").unwrap(), b"hello");
    }

    #[test]
    fn armor_is_found_only_at_the_start_of_a_line() {
        let text = "hi\n> -----BEGIN PGP MESSAGE-----\nx\n-----END PGP MESSAGE-----\n\n-----BEGIN PGP SIGNED MESSAGE-----\nHash: SHA256\n\nt\n-----BEGIN PGP SIGNATURE-----\n\nzz\n-----END PGP SIGNATURE-----\n";
        let block = find_armor(text).unwrap();
        assert_eq!(block.kind, ArmorKind::Signed);
        assert!(text[block.range.clone()].ends_with("SIGNATURE-----"));
        assert_eq!(find_armor("nothing here"), None);
    }

    #[test]
    fn a_pgp_mime_encrypted_message_and_a_p7m_are_recognised() {
        let raw = b"Content-Type: multipart/encrypted; protocol=\"application/pgp-encrypted\"; boundary=E\r\n\r\n--E\r\nContent-Type: application/pgp-encrypted\r\n\r\nVersion: 1\r\n--E\r\nContent-Type: application/octet-stream\r\n\r\nDATA\r\n--E--\r\n";
        let root = parse(raw).unwrap();
        assert_eq!(classify(&root), Some(Found::PgpEncrypted));
        let p7 =
            parse(b"Content-Type: application/pkcs7-mime; smime-type=enveloped-data; name=smime.p7m\r\n\r\nAAAA\r\n")
                .unwrap();
        assert_eq!(classify(&p7), Some(Found::Pkcs7 { smime_type: Some("enveloped-data".into()) }));
        let (_, _, whole) = find(&p7).unwrap();
        assert!(whole);
    }
}
