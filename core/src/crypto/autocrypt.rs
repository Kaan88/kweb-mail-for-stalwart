//! Autocrypt (level 1): the `Autocrypt` header of a mail carries its sender's OpenPGP public key.
//!
//! A key learned this way was claimed by whoever wrote the mail and nobody checked it: it is kept with its
//! source, used to encrypt with a warning, and never a reason for a green badge (trust on first use).

use base64::{engine::general_purpose::STANDARD, Engine};

/// The most a header's key may be (decoded). A real key is a few kilobytes.
pub const MAX_KEYDATA: usize = 64 * 1024;

/// What one `Autocrypt` header says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Header {
    pub addr: String,
    /// The OpenPGP key packets, as sent.
    pub keydata: Vec<u8>,
    pub prefer_encrypt_mutual: bool,
}

/// Reads one header value. `None` when it is not a usable level 1 header: no address or key, an attribute that
/// must be understood and is not (any name not starting with `_`), or a key that is far too large.
pub fn parse(value: &str) -> Option<Header> {
    let (mut addr, mut keydata, mut mutual) = (None, None, false);
    for attribute in value.split(';') {
        let attribute = attribute.trim();
        if attribute.is_empty() {
            continue;
        }
        let (name, data) = attribute.split_once('=')?;
        let name = name.trim().to_ascii_lowercase();
        match name.as_str() {
            "addr" => addr = Some(data.trim().to_ascii_lowercase()),
            "keydata" => {
                let compact: String = data.chars().filter(|c| !c.is_whitespace()).collect();
                if compact.len() > MAX_KEYDATA * 4 / 3 + 8 {
                    return None;
                }
                keydata = STANDARD.decode(compact.as_bytes()).ok();
            }
            "prefer-encrypt" => mutual = data.trim().eq_ignore_ascii_case("mutual"),
            other if other.starts_with('_') => {}
            _ => return None,
        }
    }
    let (addr, keydata) = (addr?, keydata?);
    (!addr.is_empty() && !keydata.is_empty() && keydata.len() <= MAX_KEYDATA).then_some(Header {
        addr,
        keydata,
        prefer_encrypt_mutual: mutual,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_header_is_read_with_its_folding_and_unknown_critical_attributes_refuse_it() {
        let h = parse(
            "addr=Bob@Example.TEST; prefer-encrypt=mutual; keydata=
 AQID
 BAUG",
        )
        .unwrap();
        assert_eq!(h.addr, "bob@example.test");
        assert!(h.prefer_encrypt_mutual);
        assert_eq!(h.keydata, [1, 2, 3, 4, 5, 6]);
        assert!(parse("addr=a@b.test; _note=x; keydata=AQID").is_some(), "non-critical attributes are ignored");
        assert!(parse("addr=a@b.test; type=2; keydata=AQID").is_none(), "an attribute it does not know is critical");
        assert!(parse("addr=a@b.test").is_none());
        assert!(parse("keydata=AQID").is_none());
        assert!(parse("addr=a@b.test; keydata=!!!").is_none());
        let huge = format!("addr=a@b.test; keydata={}", "A".repeat(200_000));
        assert!(parse(&huge).is_none());
    }
}
