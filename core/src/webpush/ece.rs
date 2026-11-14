//! RFC 8291 (Web Push message encryption) on top of RFC 8188 (`aes128gcm`), the receiving side
//! only: one record, as every push the server sends is.
//!
//! Anything that is not exactly that (another content encoding, a second record, a record
//! cut short, a body for other keys) is refused with `None`. Nothing here panics on input from
//! the network, and nothing says why a body was refused.

use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes128Gcm, Nonce,
};
use hkdf::Hkdf;
use p256::{
    ecdh::diffie_hellman,
    elliptic_curve::sec1::{FromEncodedPoint, ToEncodedPoint},
    EncodedPoint, PublicKey, SecretKey,
};
use sha2::Sha256;

/// The most a record may hold (RFC 8188 `rs`): the server's `maxPushSize` and Google's limit.
pub const MAX_RECORD: usize = 4096;
/// Salt (16) + record size (4) + key id length (1).
const HEADER: usize = 21;
/// The length of the uncompressed P-256 point the key id holds.
const POINT: usize = 65;
const TAG: usize = 16;

/// The public key (65 bytes, uncompressed) that belongs to a private key.
pub fn public_point(secret: &SecretKey) -> Vec<u8> {
    secret.public_key().to_encoded_point(false).as_bytes().to_vec()
}

/// The two keys derived from the shared secret.
fn keys(
    secret: &SecretKey,
    as_public: &PublicKey,
    ua_public: &[u8],
    as_point: &[u8],
    auth: &[u8],
    salt: &[u8],
) -> Option<([u8; 16], [u8; 12])> {
    let shared = diffie_hellman(secret.to_nonzero_scalar(), as_public.as_affine());
    let mut info = Vec::with_capacity(14 + 2 * POINT);
    info.extend_from_slice(b"WebPush: info\0");
    info.extend_from_slice(ua_public);
    info.extend_from_slice(as_point);
    let mut ikm = [0u8; 32];
    Hkdf::<Sha256>::new(Some(auth), shared.raw_secret_bytes()).expand(&info, &mut ikm).ok()?;
    let hk = Hkdf::<Sha256>::new(Some(salt), &ikm);
    let mut cek = [0u8; 16];
    let mut nonce = [0u8; 12];
    hk.expand(b"Content-Encoding: aes128gcm\0", &mut cek).ok()?;
    hk.expand(b"Content-Encoding: nonce\0", &mut nonce).ok()?;
    Some((cek, nonce))
}

/// Decrypts a push body for the key pair of this subscription (`secret`, `auth`). `None` for
/// anything that is not one well-formed `aes128gcm` record made for these keys.
pub fn decrypt(body: &[u8], secret: &SecretKey, auth: &[u8]) -> Option<Vec<u8>> {
    if body.len() < HEADER {
        return None;
    }
    let salt = &body[..16];
    let record_size = u32::from_be_bytes([body[16], body[17], body[18], body[19]]) as usize;
    let id_len = body[20] as usize;
    // The key id is the server's public key, an uncompressed point; nothing else is used.
    if id_len != POINT || body.len() < HEADER + id_len + TAG + 1 {
        return None;
    }
    let as_point = &body[HEADER..HEADER + id_len];
    let cipher_text = &body[HEADER + id_len..];
    // One record: it fits in `rs`, and `rs` is within the limits (RFC 8188: at least 18).
    if !(TAG + 2..=MAX_RECORD).contains(&record_size) || cipher_text.len() > record_size {
        return None;
    }
    let as_public = PublicKey::from_encoded_point(&EncodedPoint::from_bytes(as_point).ok()?).into_option()?;
    let ua_public = public_point(secret);
    let (cek, nonce) = keys(secret, &as_public, &ua_public, as_point, auth, salt)?;
    let cipher = Aes128Gcm::new_from_slice(&cek).ok()?;
    let mut plain = cipher.decrypt(&Nonce::from(nonce), cipher_text).ok()?;
    // Padding: zeros, then the delimiter. 0x02 marks the last record; 0x01 would mean that
    // more records follow, which this does not take.
    let end = plain.iter().rposition(|b| *b != 0)?;
    if plain[end] != 2 {
        return None;
    }
    plain.truncate(end);
    Some(plain)
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};

    fn b64(text: &str) -> Vec<u8> {
        URL_SAFE_NO_PAD.decode(text).unwrap()
    }

    // RFC 8291, section 5 and appendix A.
    const UA_PRIVATE: &str = "q1dXpw3UpT5VOmu_cf_v6ih07Aems3njxI-JWgLcM94";
    const AUTH: &str = "BTBZMqHH6r4Tts7J_aSIgg";
    const AS_PRIVATE: &str = "yfWPiYE-n46HLnH0KqZOF1fJJU3MYrct3AELtAQ-oRw";
    const SALT: &str = "DGv6ra1nlYgDCS1FRnbzlw";
    const PLAINTEXT: &[u8] = b"When I grow up, I want to be a watermelon";
    const BODY: &str = "DGv6ra1nlYgDCS1FRnbzlwAAEABBBP4z9KsN6nGRTbVYI_c7VJSPQTBtkgcy27mlmlMoZIIgDll6e3vCYLocInmYWAmS6TlzAC8wEqKK6PBru3jl7A_yl95bQpu6cVPTpK4Mqgkf1CXztLVBSt2Ks3oZwbuwXPXLWyouBWLVWGNWQexSgSxsj_Qulcy4a-fN";

    fn ua() -> SecretKey {
        SecretKey::from_slice(&b64(UA_PRIVATE)).unwrap()
    }

    /// Encrypts the way a push server does (one record, padded with zeros to `pad_to` bytes of
    /// plaintext), for tests of the odd cases.
    fn encrypt(plain: &[u8], pad_to: usize, ua_pub: &[u8], auth: &[u8], as_secret: &SecretKey, salt: &[u8]) -> Vec<u8> {
        let mut data = plain.to_vec();
        data.push(2);
        data.resize(pad_to.max(data.len()), 0);
        encrypt_raw(&data, ua_pub, auth, as_secret, salt)
    }

    #[test]
    fn the_rfc_8291_example_decrypts() {
        let plain = decrypt(&b64(BODY), &ua(), &b64(AUTH)).expect("the RFC's own example");
        assert_eq!(plain, PLAINTEXT);
    }

    #[test]
    fn our_encryption_reproduces_the_rfc_body() {
        let ua = ua();
        let as_secret = SecretKey::from_slice(&b64(AS_PRIVATE)).unwrap();
        let body = encrypt(PLAINTEXT, 0, &public_point(&ua), &b64(AUTH), &as_secret, &b64(SALT));
        assert_eq!(body, b64(BODY));
    }

    fn body_for(plain: &[u8], pad_to: usize) -> (Vec<u8>, SecretKey, Vec<u8>) {
        let ua = SecretKey::from_slice(&[7u8; 32]).unwrap();
        let as_secret = SecretKey::from_slice(&[9u8; 32]).unwrap();
        let auth = vec![3u8; 16];
        (encrypt(plain, pad_to, &public_point(&ua), &auth, &as_secret, &[5u8; 16]), ua, auth)
    }

    #[test]
    fn padding_is_taken_off() {
        let (body, ua, auth) = body_for(b"{\"a\":1}", 200);
        assert_eq!(decrypt(&body, &ua, &auth).unwrap(), b"{\"a\":1}");
        let (body, ua, auth) = body_for(b"", 0);
        assert_eq!(decrypt(&body, &ua, &auth).unwrap(), b"");
    }

    #[test]
    fn a_body_for_other_keys_is_refused() {
        let (body, _, auth) = body_for(b"x", 0);
        let other = SecretKey::from_slice(&[8u8; 32]).unwrap();
        assert!(decrypt(&body, &other, &auth).is_none());
        let (body, ua, _) = body_for(b"x", 0);
        assert!(decrypt(&body, &ua, &[4u8; 16]).is_none());
    }

    #[test]
    fn a_damaged_body_is_refused_at_every_length_and_position() {
        let (body, ua, auth) = body_for(b"hello there", 64);
        assert!(decrypt(&body, &ua, &auth).is_some());
        for cut in 0..body.len() {
            assert!(decrypt(&body[..cut], &ua, &auth).is_none(), "cut at {cut}");
        }
        for at in 0..body.len() {
            let mut bent = body.clone();
            bent[at] ^= 0x01;
            // The salt, the key id and the ciphertext all matter; the record size may be
            // anything that still holds the record.
            if (16..20).contains(&at) {
                continue;
            }
            assert!(decrypt(&bent, &ua, &auth).is_none(), "bit flipped at {at}");
        }
    }

    #[test]
    fn a_wrong_header_is_refused() {
        let (body, ua, auth) = body_for(b"hello", 0);
        let with = |f: &dyn Fn(&mut Vec<u8>)| {
            let mut b = body.clone();
            f(&mut b);
            decrypt(&b, &ua, &auth)
        };
        assert!(with(&|_| {}).is_some());
        // A key id of another length.
        assert!(with(&|b| b[20] = 64).is_none());
        assert!(with(&|b| b[20] = 0).is_none());
        // A record size that is too small, too large, or smaller than the data (more records).
        assert!(with(&|b| b[16..20].copy_from_slice(&17u32.to_be_bytes())).is_none());
        assert!(with(&|b| b[16..20].copy_from_slice(&4097u32.to_be_bytes())).is_none());
        assert!(with(&|b| b[16..20].copy_from_slice(&u32::MAX.to_be_bytes())).is_none());
        let n = body.len() - HEADER - POINT;
        assert!(with(&|b| b[16..20].copy_from_slice(&((n - 1) as u32).to_be_bytes())).is_none());
        // The key id is not a point on the curve.
        assert!(with(&|b| b[HEADER..HEADER + POINT].fill(0)).is_none());
        // Trailing bytes after the record are a second record, or garbage.
        assert!(with(&|b| b.push(0)).is_none());
    }

    #[test]
    fn a_record_that_is_not_the_last_is_refused() {
        // Delimiter 1 (more records follow) and a missing delimiter.
        let ua = SecretKey::from_slice(&[7u8; 32]).unwrap();
        let as_secret = SecretKey::from_slice(&[9u8; 32]).unwrap();
        let (auth, salt) = (vec![3u8; 16], [5u8; 16]);
        let good = encrypt(b"abc", 0, &public_point(&ua), &auth, &as_secret, &salt);
        assert!(decrypt(&good, &ua, &auth).is_some());
        // Re-seal "abc" + 0x01, and "abc" + zeros only.
        for tail in [vec![1u8], vec![0u8, 0u8]] {
            let mut sealed_plain = b"abc".to_vec();
            sealed_plain.extend(&tail);
            let body = encrypt_raw(&sealed_plain, &public_point(&ua), &auth, &as_secret, &salt);
            assert!(decrypt(&body, &ua, &auth).is_none(), "{tail:?}");
        }
    }

    /// Like `encrypt`, but the bytes are sealed as they are (no delimiter added).
    fn encrypt_raw(data: &[u8], ua_pub: &[u8], auth: &[u8], as_secret: &SecretKey, salt: &[u8]) -> Vec<u8> {
        let as_point = public_point(as_secret);
        let ua_public = PublicKey::from_sec1_bytes(ua_pub).unwrap();
        let shared = diffie_hellman(as_secret.to_nonzero_scalar(), ua_public.as_affine());
        let mut info = b"WebPush: info\0".to_vec();
        info.extend_from_slice(ua_pub);
        info.extend_from_slice(&as_point);
        let mut ikm = [0u8; 32];
        Hkdf::<Sha256>::new(Some(auth), shared.raw_secret_bytes()).expand(&info, &mut ikm).unwrap();
        let hk = Hkdf::<Sha256>::new(Some(salt), &ikm);
        let (mut cek, mut nonce) = ([0u8; 16], [0u8; 12]);
        hk.expand(b"Content-Encoding: aes128gcm\0", &mut cek).unwrap();
        hk.expand(b"Content-Encoding: nonce\0", &mut nonce).unwrap();
        let sealed = Aes128Gcm::new_from_slice(&cek).unwrap().encrypt(&Nonce::from(nonce), data).unwrap();
        let mut out = salt.to_vec();
        out.extend_from_slice(&4096u32.to_be_bytes());
        out.push(65);
        out.extend_from_slice(&as_point);
        out.extend_from_slice(&sealed);
        out
    }

    #[test]
    fn nothing_panics_on_junk() {
        let (_, ua, auth) = body_for(b"x", 0);
        for len in [0usize, 1, 20, 21, 22, 86, 87, 102, 103, 104, 500, 5000] {
            for fill in [0u8, 1, 0xff, 0x41] {
                let mut junk = vec![fill; len];
                if len > 20 {
                    junk[20] = 65;
                    junk[16..20].copy_from_slice(&4096u32.to_be_bytes());
                }
                assert!(decrypt(&junk, &ua, &auth).is_none());
            }
        }
    }
}
