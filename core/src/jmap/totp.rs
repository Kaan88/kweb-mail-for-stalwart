//! Two-step codes made on this device: the secret, the `otpauth://` address an authenticator app
//! reads, and the check of a code (RFC 6238 over RFC 4226: HMAC-SHA1, 6 digits, 30 seconds).
//!
//! The secret is only ever in the address and in what the host shows the user. Nothing here
//! logs it, and nothing here keeps it.

use sha1::{Digest, Sha1};
use url::Url;

use crate::{Error, Result};

const ALPHABET: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
const SECRET_BYTES: usize = 20;
const DIGITS: u32 = 6;
const PERIOD_SECS: u64 = 30;
const BLOCK: usize = 64;

/// RFC 4648 base32, upper case, no padding.
pub(super) fn base32_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(5) * 8);
    let (mut buffer, mut bits) = (0u32, 0u32);
    for &byte in bytes {
        buffer = (buffer << 8) | u32::from(byte);
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            out.push(ALPHABET[((buffer >> bits) & 31) as usize] as char);
        }
        buffer &= (1 << bits) - 1;
    }
    if bits > 0 {
        out.push(ALPHABET[((buffer << (5 - bits)) & 31) as usize] as char);
    }
    out
}

/// The bytes of a base32 text. Spaces, hyphens, padding and the case of the letters do not
/// matter; anything else that is not base32 makes it `None`.
pub(super) fn base32_decode(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len() * 5 / 8);
    let (mut buffer, mut bits) = (0u32, 0u32);
    for c in text.chars().filter(|c| !matches!(c, ' ' | '-' | '=')) {
        let value = ALPHABET.iter().position(|a| *a as char == c.to_ascii_uppercase())? as u32;
        buffer = (buffer << 5) | value;
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            out.push((buffer >> bits) as u8);
            buffer &= (1 << bits) - 1;
        }
    }
    Some(out)
}

/// HMAC-SHA1 (RFC 2104).
fn hmac_sha1(key: &[u8], message: &[u8]) -> [u8; 20] {
    let mut block = [0u8; BLOCK];
    if key.len() > BLOCK {
        block[..20].copy_from_slice(&Sha1::digest(key));
    } else {
        block[..key.len()].copy_from_slice(key);
    }
    let pad = |byte: u8| block.map(|b| b ^ byte);
    let mut inner = Sha1::new();
    inner.update(pad(0x36));
    inner.update(message);
    let mut outer = Sha1::new();
    outer.update(pad(0x5c));
    outer.update(inner.finalize());
    outer.finalize().into()
}

/// The code of one counter value (RFC 4226 section 5.3), `digits` long.
pub(super) fn hotp(key: &[u8], counter: u64, digits: u32) -> u32 {
    let mac = hmac_sha1(key, &counter.to_be_bytes());
    let offset = (mac[19] & 0x0f) as usize;
    let truncated = u32::from_be_bytes([mac[offset] & 0x7f, mac[offset + 1], mac[offset + 2], mac[offset + 3]]);
    truncated % 10u32.pow(digits)
}

/// Whether `code` is the code of `key` at `unix_secs`, of the 30 second step before it, or of
/// the one after (a clock a little off, or a code typed just as the step ends).
pub(super) fn code_matches(key: &[u8], code: &str, unix_secs: u64) -> bool {
    let typed: String = code.chars().filter(|c| !c.is_whitespace()).collect();
    if typed.len() != DIGITS as usize || !typed.bytes().all(|b| b.is_ascii_digit()) {
        return false;
    }
    let step = unix_secs / PERIOD_SECS;
    let mut found = false;
    // Every step is checked, whatever the first answer: the time taken does not tell which.
    for counter in [step.wrapping_sub(1), step, step + 1] {
        let expected = format!("{:0width$}", hotp(key, counter, DIGITS), width = DIGITS as usize);
        found |= constant_time_eq(expected.as_bytes(), typed.as_bytes());
    }
    found
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// A new secret and the address for it. `issuer` is the server's host, `label` the account's
/// address. The secret is shown in groups of four.
pub(super) fn new_setup(issuer: &str, label: &str) -> Result<(String, String)> {
    let mut bytes = [0u8; SECRET_BYTES];
    getrandom::fill(&mut bytes).map_err(|e| Error::temporary(format!("random numbers: {e}")))?;
    let secret = base32_encode(&bytes);
    let mut url = Url::parse("otpauth://totp/").map_err(|e| Error::invalid(e.to_string()))?;
    url.path_segments_mut().map_err(|()| Error::invalid("the account's address cannot be a label"))?.pop().push(label);
    url.query_pairs_mut().append_pair("secret", &secret).append_pair("issuer", issuer);
    let groups: Vec<&str> = secret.as_bytes().chunks(4).map(|c| std::str::from_utf8(c).unwrap_or_default()).collect();
    Ok((url.to_string(), groups.join(" ")))
}

/// The key inside an `otpauth://totp/` address. An address that is not one, has no secret, or
/// asks for anything but the usual 6 digits every 30 seconds with SHA-1 (what this app can
/// check, and what authenticator apps use unless told otherwise) is `Error::Invalid`.
pub(super) fn key_of(otpauth_url: &str) -> Result<Vec<u8>> {
    let bad = || Error::invalid("That is not a two-step setup this app made.");
    let url = Url::parse(otpauth_url.trim()).map_err(|_| bad())?;
    if url.scheme() != "otpauth" || url.host_str() != Some("totp") {
        return Err(bad());
    }
    let mut key = None;
    for (name, value) in url.query_pairs() {
        match (name.as_ref(), value.as_ref()) {
            ("secret", v) => key = base32_decode(v),
            ("algorithm", v) if !v.eq_ignore_ascii_case("SHA1") => return Err(bad()),
            ("digits", v) if v != "6" => return Err(bad()),
            ("period", v) if v != "30" => return Err(bad()),
            _ => {}
        }
    }
    key.filter(|k| !k.is_empty()).ok_or_else(bad)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// RFC 6238 appendix B: the SHA-1 key "12345678901234567890", 8 digits.
    #[test]
    fn the_codes_of_rfc_6238_come_out() {
        let key = b"12345678901234567890";
        for (time, expected) in [
            (59u64, 94_287_082u32),
            (1_111_111_109, 7_081_804),
            (1_111_111_111, 14_050_471),
            (1_234_567_890, 89_005_924),
            (2_000_000_000, 69_279_037),
            (20_000_000_000, 65_353_130),
        ] {
            assert_eq!(hotp(key, time / PERIOD_SECS, 8), expected, "at {time}");
        }
    }

    /// RFC 4226 appendix D: the first ten counters of the same key, 6 digits.
    #[test]
    fn the_codes_of_rfc_4226_come_out() {
        let expected = [755224, 287082, 359152, 969429, 338314, 254676, 287922, 162583, 399871, 520489];
        for (counter, want) in expected.into_iter().enumerate() {
            assert_eq!(hotp(b"12345678901234567890", counter as u64, 6), want);
        }
    }

    #[test]
    fn hmac_works_for_keys_longer_than_a_block() {
        // RFC 2202 test case 6: an 80-byte key of 0xaa.
        let mac = hmac_sha1(&[0xaa; 80], b"Test Using Larger Than Block-Size Key - Hash Key First");
        let hex: String = mac.iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(hex, "aa4ae5e15272d00e95705637ce8a3b55ed402112");
    }

    #[test]
    fn a_code_is_taken_one_step_early_or_late_and_no_more() {
        let key = b"12345678901234567890";
        let at = 1_111_111_111u64; // step 37037037
        let code = |step: u64| format!("{:06}", hotp(key, step, 6));
        let now = at / PERIOD_SECS;
        assert!(code_matches(key, &code(now), at));
        assert!(code_matches(key, &code(now - 1), at));
        assert!(code_matches(key, &code(now + 1), at));
        assert!(!code_matches(key, &code(now - 2), at));
        assert!(!code_matches(key, &code(now + 2), at));
        // Spaces are what an authenticator app shows; other shapes are no code.
        let spaced = format!("{} {}", &code(now)[..3], &code(now)[3..]);
        assert!(code_matches(key, &spaced, at));
        assert!(!code_matches(key, "12345", at));
        assert!(!code_matches(key, "1234567", at));
        assert!(!code_matches(key, "12a456", at));
        assert!(!code_matches(key, "", at));
    }

    #[test]
    fn base32_round_trips_and_matches_rfc_4648() {
        for (plain, coded) in [("", ""), ("f", "MY"), ("fo", "MZXQ"), ("foo", "MZXW6"), ("foobar", "MZXW6YTBOI")] {
            assert_eq!(base32_encode(plain.as_bytes()), coded);
            assert_eq!(base32_decode(coded).unwrap(), plain.as_bytes());
        }
        assert_eq!(base32_decode("mzxw 6ytb-oi==").unwrap(), b"foobar");
        assert!(base32_decode("MZ1").is_none());
        let bytes: Vec<u8> = (0..=255).collect();
        assert_eq!(base32_decode(&base32_encode(&bytes)).unwrap(), bytes);
    }

    #[test]
    fn a_new_setup_is_an_address_with_a_key_of_twenty_bytes() {
        let (url, shown) = new_setup("mail.example.org", "me@example.org").unwrap();
        assert!(url.starts_with("otpauth://totp/me@example.org?secret="), "{url}");
        assert!(url.ends_with("&issuer=mail.example.org"), "{url}");
        let key = key_of(&url).unwrap();
        assert_eq!(key.len(), 20);
        // The same secret for typing, in groups of four.
        assert!(shown.split(' ').all(|g| g.len() == 4));
        assert_eq!(base32_decode(&shown).unwrap(), key);
        // Two setups never share a secret.
        assert_ne!(new_setup("h", "a@b").unwrap().1, shown);
    }

    #[test]
    fn a_label_with_odd_characters_is_escaped() {
        let (url, _) = new_setup("host", "a b/c?d@e.org").unwrap();
        let parsed = Url::parse(&url).unwrap();
        assert_eq!(parsed.query_pairs().count(), 2);
        assert_eq!(parsed.path_segments().unwrap().count(), 1);
    }

    #[test]
    fn only_the_usual_settings_are_taken() {
        let ok = "otpauth://totp/x?secret=JBSWY3DPEHPK3PXP&issuer=h";
        assert_eq!(key_of(ok).unwrap().len(), 10);
        assert!(key_of("otpauth://totp/x?secret=JBSWY3DPEHPK3PXP&algorithm=sha1&digits=6&period=30").is_ok());
        for bad in [
            "https://example.org/?secret=JBSWY3DPEHPK3PXP",
            "otpauth://hotp/x?secret=JBSWY3DPEHPK3PXP",
            "otpauth://totp/x",
            "otpauth://totp/x?secret=",
            "otpauth://totp/x?secret=!!!",
            "otpauth://totp/x?secret=JBSWY3DPEHPK3PXP&digits=8",
            "otpauth://totp/x?secret=JBSWY3DPEHPK3PXP&period=60",
            "otpauth://totp/x?secret=JBSWY3DPEHPK3PXP&algorithm=SHA256",
            "not a url",
        ] {
            assert!(matches!(key_of(bad), Err(Error::Invalid { .. })), "{bad}");
        }
    }
}
