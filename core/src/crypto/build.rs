//! M9 stage 3: writing a signed or encrypted message (OpenPGP as PGP/MIME, S/MIME as CMS).
//!
//! Pure: bytes and keys in, the finished RFC 5322 message out. Nothing here touches the store or
//! the network, and nothing secret is ever put into an error. The message is built exactly once
//! per send (the caller stores the bytes), so a retry uploads the same bytes and never builds a
//! second, different message.
//!
//! What is protected is the text and the files (one MIME entity). The headers, the subject
//! included, stay in the clear; the screens say so.

use std::fmt::Write as _;

use base64::{engine::general_purpose::STANDARD, Engine};
use pgp::{
    composed::{ArmorOptions, DetachedSignature, MessageBuilder, SignedPublicKey, SignedPublicSubKey, SignedSecretKey},
    crypto::{hash::HashAlgorithm, sym::SymmetricKeyAlgorithm},
    types::{KeyDetails, Password, SigningKey},
};
use rsa::rand_core::{CryptoRng, Error as RngError, RngCore};
use sha2::{Digest, Sha256};

use super::openpgp;

// ---- randomness ---------------------------------------------------------------------------------

/// The system's random numbers, for the pure-Rust crypto crates (they want `rand_core` 0.6).
pub struct SysRng;

impl RngCore for SysRng {
    fn next_u32(&mut self) -> u32 {
        let mut b = [0u8; 4];
        self.fill_bytes(&mut b);
        u32::from_le_bytes(b)
    }
    fn next_u64(&mut self) -> u64 {
        let mut b = [0u8; 8];
        self.fill_bytes(&mut b);
        u64::from_le_bytes(b)
    }
    fn fill_bytes(&mut self, dest: &mut [u8]) {
        getrandom::fill(dest).expect("the system has no random numbers");
    }
    fn try_fill_bytes(&mut self, dest: &mut [u8]) -> Result<(), RngError> {
        getrandom::fill(dest).map_err(|_| RngError::from(std::num::NonZeroU32::new(RngError::CUSTOM_START).unwrap()))
    }
}

impl CryptoRng for SysRng {}

fn random_hex(bytes: usize) -> String {
    let mut buf = vec![0u8; bytes];
    SysRng.fill_bytes(&mut buf);
    buf.iter().map(|b| format!("{b:02x}")).collect()
}

// ---- what goes in and what can go wrong ---------------------------------------------------------

/// Why a message could not be protected. Plain words only: never any of the content or a key.
#[derive(Debug, Clone, PartialEq)]
pub enum BuildError {
    /// The signing key is locked and no passphrase was given.
    NeedPassphrase,
    WrongPassphrase,
    /// The key cannot sign (no signing key in it).
    NoSigningKey,
    /// This address has no key that can encrypt (none, expired, revoked or not made for encrypting).
    NoEncryptionKey(String),
    /// Something this program does not do, with the reason in words.
    Unsupported(String),
    Failed(String),
}

impl BuildError {
    pub fn text(&self) -> String {
        match self {
            BuildError::NeedPassphrase => "The key needs its passphrase.".into(),
            BuildError::WrongPassphrase => "That passphrase does not open the key.".into(),
            BuildError::NoSigningKey => "Your key cannot be used for signing.".into(),
            BuildError::NoEncryptionKey(who) => format!("There is no key to encrypt to for {who}."),
            BuildError::Unsupported(why) => why.clone(),
            BuildError::Failed(why) => why.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Addr {
    pub name: String,
    pub email: String,
}

/// The headers of the outer message. Bcc is deliberately not here: it goes in the envelope only.
#[derive(Debug, Clone)]
pub struct Headers {
    pub from: Addr,
    pub to: Vec<Addr>,
    pub cc: Vec<Addr>,
    pub subject: String,
    /// Unix seconds.
    pub date: i64,
    /// Without angle brackets.
    pub message_id: String,
    pub in_reply_to: Vec<String>,
    pub references: Vec<String>,
}

pub struct File {
    pub name: String,
    pub mime_type: String,
    pub bytes: Vec<u8>,
}

/// Who signs, and with what.
pub enum Signer<'a> {
    Pgp {
        key: &'a SignedSecretKey,
        passphrase: &'a str,
    },
    /// The certificate (DER) and the PKCS#8 RSA key (DER).
    Smime {
        cert: &'a [u8],
        pkcs8: &'a [u8],
    },
}

/// Who can read it.
pub enum Encrypt<'a> {
    /// `hide`: the packets do not name the recipients' keys (used when a Bcc is among them).
    Pgp { keys: Vec<&'a SignedPublicKey>, hide: bool },
    /// Certificates (DER). S/MIME names every recipient, so it cannot hide any.
    Smime { certs: Vec<&'a [u8]> },
}

pub struct Protect<'a> {
    pub sign: Option<Signer<'a>>,
    pub encrypt: Option<Encrypt<'a>>,
    /// `(address, the public key bytes)` for the `Autocrypt` header of an OpenPGP message.
    pub autocrypt: Option<(String, Vec<u8>)>,
    /// Unix seconds: for the signing time and for judging whether a recipient key is still valid.
    pub now: i64,
}

// ---- MIME pieces --------------------------------------------------------------------------------

/// A MIME entity: header lines (each ending in CRLF) and the body.
struct Entity {
    headers: String,
    body: Vec<u8>,
}

impl Entity {
    fn bytes(&self) -> Vec<u8> {
        let mut out = self.headers.clone().into_bytes();
        out.extend_from_slice(b"\r\n");
        out.extend_from_slice(&self.body);
        out
    }
}

fn boundary() -> String {
    format!("kweb-{}", random_hex(12))
}

fn is_plain_ascii(s: &str) -> bool {
    s.bytes().all(|b| (0x20..0x7f).contains(&b))
}

/// Text with line ends made CRLF, quoted-printable, every line at most 76 columns, ending in CRLF.
pub fn quoted_printable(text: &str) -> String {
    let normalised = text.replace("\r\n", "\n").replace('\r', "\n");
    let mut out = String::new();
    let body = normalised.strip_suffix('\n').unwrap_or(&normalised);
    for line in body.split('\n') {
        let bytes = line.as_bytes();
        let mut width = 0usize;
        for (i, &b) in bytes.iter().enumerate() {
            let last = i + 1 == bytes.len();
            let literal = (b == b' ' || b == b'\t') && !last || (0x21..=0x7e).contains(&b) && b != b'=';
            let piece = if literal { (b as char).to_string() } else { format!("={b:02X}") };
            // Keep room for the soft break "=" within 76 columns.
            if width + piece.len() > 75 && !(last && width + piece.len() <= 76) {
                out.push_str("=\r\n");
                width = 0;
            }
            out.push_str(&piece);
            width += piece.len();
        }
        out.push_str("\r\n");
    }
    out
}

fn base64_lines(bytes: &[u8]) -> String {
    let text = STANDARD.encode(bytes);
    let mut out = String::new();
    for chunk in text.as_bytes().chunks(76) {
        out.push_str(std::str::from_utf8(chunk).unwrap_or_default());
        out.push_str("\r\n");
    }
    out
}

/// A header value for text that may not be ASCII: RFC 2047 encoded words, folded.
fn encode_text(text: &str) -> String {
    let clean: String = text.chars().filter(|c| !c.is_control()).collect();
    if is_plain_ascii(&clean) {
        return clean;
    }
    let mut words: Vec<String> = Vec::new();
    let mut chunk = String::new();
    for c in clean.chars() {
        if chunk.len() + c.len_utf8() > 42 {
            words.push(std::mem::take(&mut chunk));
        }
        chunk.push(c);
    }
    if !chunk.is_empty() {
        words.push(chunk);
    }
    words.iter().map(|w| format!("=?UTF-8?B?{}?=", STANDARD.encode(w.as_bytes()))).collect::<Vec<_>>().join("\r\n ")
}

/// `Name: value`, folded at spaces so no line passes 78 columns (encoded words are already folded).
fn header_line(name: &str, value: &str) -> String {
    let mut out = String::new();
    let mut line = format!("{name}:");
    for word in value.split(' ') {
        if line.len() + 1 + word.len() > 76 && line.len() > name.len() + 1 {
            out.push_str(&line);
            out.push_str("\r\n");
            line = String::new();
        }
        line.push(' ');
        line.push_str(word);
    }
    out.push_str(&line);
    out.push_str("\r\n");
    out
}

fn address_text(a: &Addr) -> String {
    let email = a.email.trim();
    let name = a.name.trim();
    if name.is_empty() {
        return email.to_string();
    }
    let shown = if is_plain_ascii(name) {
        if name.chars().any(|c| "\"(),.:;<>@[]\\".contains(c)) {
            format!("\"{}\"", name.replace('\\', "\\\\").replace('"', "\\\""))
        } else {
            name.to_string()
        }
    } else {
        encode_text(name)
    };
    format!("{shown} <{email}>")
}

fn address_list(list: &[Addr]) -> String {
    list.iter().map(address_text).collect::<Vec<_>>().join(", ")
}

/// A parameter such as `filename`: plain when ASCII, RFC 2231 when not.
fn param(name: &str, value: &str) -> String {
    if is_plain_ascii(value) {
        format!("{name}=\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
    } else {
        let mut encoded = String::new();
        for b in value.bytes() {
            if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
                encoded.push(b as char);
            } else {
                let _ = write!(encoded, "%{b:02X}");
            }
        }
        format!("{name}*=UTF-8''{encoded}")
    }
}

fn text_entity(text: &str) -> Entity {
    Entity {
        headers: "Content-Type: text/plain; charset=utf-8\r\nContent-Transfer-Encoding: quoted-printable\r\n".into(),
        body: quoted_printable(text).into_bytes(),
    }
}

/// The text and the files as one entity: the text alone, or `multipart/mixed`.
fn content_entity(text: &str, files: &[File]) -> Entity {
    if files.is_empty() {
        return text_entity(text);
    }
    let b = boundary();
    let mut body: Vec<u8> = Vec::new();
    let first = text_entity(text);
    body.extend_from_slice(format!("--{b}\r\n").as_bytes());
    body.extend_from_slice(&first.bytes());
    for file in files {
        let kind = if file.mime_type.trim().is_empty() || !file.mime_type.contains('/') {
            "application/octet-stream"
        } else {
            file.mime_type.as_str()
        };
        body.extend_from_slice(format!("--{b}\r\n").as_bytes());
        body.extend_from_slice(
            format!(
                "Content-Type: {kind}; {}\r\nContent-Transfer-Encoding: base64\r\nContent-Disposition: attachment; {}\r\n\r\n",
                param("name", &file.name),
                param("filename", &file.name)
            )
            .as_bytes(),
        );
        body.extend_from_slice(base64_lines(&file.bytes).as_bytes());
    }
    body.extend_from_slice(format!("--{b}--\r\n").as_bytes());
    Entity { headers: format!("Content-Type: multipart/mixed; boundary=\"{b}\"\r\n"), body }
}

/// A `multipart/signed` entity: the content exactly as signed, then the signature part.
fn signed_entity(protocol: &str, micalg: &str, signed: &[u8], signature_part: &str) -> Entity {
    let b = boundary();
    let mut body: Vec<u8> = Vec::new();
    body.extend_from_slice(format!("--{b}\r\n").as_bytes());
    body.extend_from_slice(signed);
    body.extend_from_slice(format!("\r\n--{b}\r\n{signature_part}\r\n--{b}--\r\n").as_bytes());
    Entity {
        headers: format!(
            "Content-Type: multipart/signed; protocol=\"{protocol}\"; micalg={micalg}; boundary=\"{b}\"\r\n"
        ),
        body,
    }
}

// ---- OpenPGP ------------------------------------------------------------------------------------

enum PgpSigning<'a> {
    Primary(&'a pgp::packet::SecretKey),
    Sub(&'a pgp::packet::SecretSubkey),
}

/// Which part of the key signs: the primary key when its latest self-signature allows it (or says
/// nothing about it), else a signing subkey.
fn pgp_signing(key: &SignedSecretKey) -> Option<PgpSigning<'_>> {
    let latest = key
        .details
        .users
        .iter()
        .flat_map(|u| u.signatures.iter())
        .chain(key.details.direct_signatures.iter())
        .filter(|s| s.created().is_some())
        .max_by_key(|s| s.created().map(|t| t.as_secs()));
    let primary_ok = latest.is_none_or(|s| {
        let flags = s.key_flags();
        flags.sign() || !(flags.certify() || flags.encrypt_comms() || flags.encrypt_storage())
    });
    if primary_ok {
        return Some(PgpSigning::Primary(&key.primary_key));
    }
    key.secret_subkeys
        .iter()
        .find(|sub| sub.signatures.iter().any(|s| s.key_flags().sign()))
        .map(|sub| PgpSigning::Sub(&sub.key))
}

fn pgp_password(key: &SignedSecretKey, passphrase: &str) -> Result<Password, BuildError> {
    if !openpgp::is_locked(key) {
        return Ok(Password::empty());
    }
    if passphrase.is_empty() {
        return Err(BuildError::NeedPassphrase);
    }
    if !openpgp::passphrase_opens(key, passphrase) {
        return Err(BuildError::WrongPassphrase);
    }
    Ok(Password::from(passphrase))
}

fn pgp_sign_detached(key: &SignedSecretKey, passphrase: &str, data: &[u8]) -> Result<String, BuildError> {
    let password = pgp_password(key, passphrase)?;
    let which = pgp_signing(key).ok_or(BuildError::NoSigningKey)?;
    let failed = |_| BuildError::Failed("The signature could not be made.".into());
    let signature = match which {
        PgpSigning::Primary(k) => {
            DetachedSignature::sign_binary_data(SysRng, k, &password, HashAlgorithm::Sha256, data).map_err(failed)?
        }
        PgpSigning::Sub(k) => {
            DetachedSignature::sign_binary_data(SysRng, k, &password, HashAlgorithm::Sha256, data).map_err(failed)?
        }
    };
    signature.to_armored_string(ArmorOptions::default()).map_err(failed)
}

/// What a public key offers for encrypting: a subkey, or the primary key itself.
enum PgpEncryptTo<'a> {
    Primary(&'a SignedPublicKey),
    Sub(&'a SignedPublicSubKey),
}

/// The newest encryption-capable subkey that is neither expired nor revoked; failing that, the primary
/// key when it is marked for encrypting. A key that is revoked as a whole or has expired gives nothing.
fn pgp_encryption_target(key: &SignedPublicKey, now: i64) -> Option<PgpEncryptTo<'_>> {
    let info = openpgp::info(key);
    if info.revoked || info.expires.is_some_and(|at| at < now) {
        return None;
    }
    let mut subs: Vec<(&SignedPublicSubKey, i64)> = Vec::new();
    for sub in &key.public_subkeys {
        let Some(binding) =
            sub.signatures.iter().filter(|s| s.created().is_some()).max_by_key(|s| s.created().map(|t| t.as_secs()))
        else {
            continue;
        };
        let flags = binding.key_flags();
        if !(flags.encrypt_comms() || flags.encrypt_storage()) {
            continue;
        }
        let created = i64::from(sub.key.created_at().as_secs());
        if binding.key_expiration_time().is_some_and(|d| created + i64::from(d.as_secs()) < now) {
            continue;
        }
        subs.push((sub, created));
    }
    subs.sort_by_key(|(_, created)| std::cmp::Reverse(*created));
    if let Some((sub, _)) = subs.first() {
        return Some(PgpEncryptTo::Sub(sub));
    }
    let latest = key
        .details
        .users
        .iter()
        .flat_map(|u| u.signatures.iter())
        .chain(key.details.direct_signatures.iter())
        .filter(|s| s.created().is_some())
        .max_by_key(|s| s.created().map(|t| t.as_secs()));
    let flags = latest.map(|s| s.key_flags())?;
    (flags.encrypt_comms() || flags.encrypt_storage()).then_some(PgpEncryptTo::Primary(key))
}

/// Whether `key` can be encrypted to at `now`.
pub fn pgp_can_encrypt(key: &SignedPublicKey, now: i64) -> bool {
    pgp_encryption_target(key, now).is_some()
}

/// Encrypts `plaintext` for the keys; when `sign` is given it is signed inside the encryption
/// (sign-then-encrypt, in one OpenPGP message with a one-pass signature).
fn pgp_encrypt(
    plaintext: &[u8],
    keys: &[&SignedPublicKey],
    hide: bool,
    sign: Option<(&SignedSecretKey, &str)>,
    now: i64,
) -> Result<String, BuildError> {
    let failed = |_| BuildError::Failed("The message could not be encrypted.".into());
    let mut rng = SysRng;
    let mut builder =
        MessageBuilder::from_bytes("", plaintext.to_vec()).seipd_v1(&mut rng, SymmetricKeyAlgorithm::AES256);
    for key in keys {
        let target = pgp_encryption_target(key, now).ok_or_else(|| {
            let who = openpgp::info(key).emails.first().cloned().unwrap_or_default();
            BuildError::NoEncryptionKey(who)
        })?;
        match (target, hide) {
            (PgpEncryptTo::Sub(k), false) => builder.encrypt_to_key(&mut rng, k).map_err(failed)?,
            (PgpEncryptTo::Sub(k), true) => builder.encrypt_to_key_anonymous(&mut rng, k).map_err(failed)?,
            (PgpEncryptTo::Primary(k), false) => builder.encrypt_to_key(&mut rng, k).map_err(failed)?,
            (PgpEncryptTo::Primary(k), true) => builder.encrypt_to_key_anonymous(&mut rng, k).map_err(failed)?,
        };
    }
    if let Some((key, passphrase)) = sign {
        let password = pgp_password(key, passphrase)?;
        let signer: &dyn SigningKey = match pgp_signing(key).ok_or(BuildError::NoSigningKey)? {
            PgpSigning::Primary(k) => k,
            PgpSigning::Sub(k) => k,
        };
        builder.sign(signer, password, HashAlgorithm::Sha256);
    }
    builder.to_armored_string(&mut rng, ArmorOptions::default()).map_err(failed)
}

fn pgp_encrypted_entity(armored: &str) -> Entity {
    let b = boundary();
    let body = format!(
        "--{b}\r\nContent-Type: application/pgp-encrypted\r\nContent-Description: PGP/MIME version identification\r\n\r\nVersion: 1\r\n\r\n--{b}\r\nContent-Type: application/octet-stream; name=\"encrypted.asc\"\r\nContent-Description: OpenPGP encrypted message\r\nContent-Disposition: inline; filename=\"encrypted.asc\"\r\n\r\n{}\r\n--{b}--\r\n",
        armored.replace("\r\n", "\n").replace('\n', "\r\n").trim_end_matches("\r\n")
    );
    Entity {
        headers: format!(
            "Content-Type: multipart/encrypted; protocol=\"application/pgp-encrypted\"; boundary=\"{b}\"\r\n"
        ),
        body: body.into_bytes(),
    }
}

// ---- S/MIME -------------------------------------------------------------------------------------

mod cmsbuild {
    use cms::{
        cert::{CertificateChoices, IssuerAndSerialNumber},
        content_info::{CmsVersion, ContentInfo},
        enveloped_data::{
            EncryptedContentInfo, EnvelopedData, KeyTransRecipientInfo, RecipientIdentifier, RecipientInfo,
            RecipientInfos,
        },
        signed_data::{
            CertificateSet, EncapsulatedContentInfo, SignedAttributes, SignedData, SignerIdentifier, SignerInfo,
            SignerInfos,
        },
    };
    use der::{
        asn1::{ObjectIdentifier, OctetString, SetOfVec, UtcTime},
        Any, Decode, Encode,
    };
    use rsa::{pkcs8::DecodePrivateKey, Pkcs1v15Encrypt, Pkcs1v15Sign, RsaPrivateKey, RsaPublicKey};
    use sha2::{Digest, Sha256};
    use x509_cert::{attr::Attribute, spki::AlgorithmIdentifierOwned, Certificate};

    use super::{BuildError, SysRng};

    const OID_SIGNED_DATA: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.2.840.113549.1.7.2");
    const OID_ENVELOPED_DATA: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.2.840.113549.1.7.3");
    const OID_DATA: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.2.840.113549.1.7.1");
    const OID_CONTENT_TYPE: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.2.840.113549.1.9.3");
    const OID_MESSAGE_DIGEST: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.2.840.113549.1.9.4");
    const OID_SIGNING_TIME: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.2.840.113549.1.9.5");
    const OID_SHA256: ObjectIdentifier = ObjectIdentifier::new_unwrap("2.16.840.1.101.3.4.2.1");
    const OID_RSA: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.2.840.113549.1.1.1");
    const OID_AES256_CBC: ObjectIdentifier = ObjectIdentifier::new_unwrap("2.16.840.1.101.3.4.1.42");

    fn failed<T>(_: T) -> BuildError {
        BuildError::Failed("The S/MIME data could not be made.".into())
    }

    fn rsa_algorithm() -> Result<AlgorithmIdentifierOwned, BuildError> {
        Ok(AlgorithmIdentifierOwned { oid: OID_RSA, parameters: Some(Any::null()) })
    }

    fn issuer_and_serial(cert: &Certificate) -> IssuerAndSerialNumber {
        IssuerAndSerialNumber {
            issuer: cert.tbs_certificate.issuer.clone(),
            serial_number: cert.tbs_certificate.serial_number.clone(),
        }
    }

    /// A detached CMS signature (DER) over `content`, RSA PKCS#1 v1.5 with SHA-256, the signer's
    /// certificate inside.
    pub fn sign_detached(cert_der: &[u8], pkcs8: &[u8], content: &[u8], now: i64) -> Result<Vec<u8>, BuildError> {
        let cert = Certificate::from_der(cert_der).map_err(failed)?;
        let key = RsaPrivateKey::from_pkcs8_der(pkcs8)
            .map_err(|_| BuildError::Unsupported("Only an RSA key can sign S/MIME mail here.".into()))?;
        let digest = Sha256::digest(content);
        let attribute = |oid: ObjectIdentifier, value: Any| -> Result<Attribute, BuildError> {
            Ok(Attribute { oid, values: SetOfVec::try_from(vec![value]).map_err(failed)? })
        };
        let time = UtcTime::from_unix_duration(std::time::Duration::from_secs(now.max(0) as u64)).map_err(failed)?;
        let attrs: SignedAttributes = SetOfVec::try_from(vec![
            attribute(OID_CONTENT_TYPE, Any::encode_from(&OID_DATA).map_err(failed)?)?,
            attribute(OID_SIGNING_TIME, Any::encode_from(&time).map_err(failed)?)?,
            attribute(
                OID_MESSAGE_DIGEST,
                Any::encode_from(&OctetString::new(digest.to_vec()).map_err(failed)?).map_err(failed)?,
            )?,
        ])
        .map_err(failed)?;
        let to_sign = attrs.to_der().map_err(failed)?;
        let signature = key
            .sign(Pkcs1v15Sign::new::<Sha256>(), &Sha256::digest(&to_sign))
            .map_err(|_| BuildError::Failed("The signature could not be made.".into()))?;
        let digest_alg = AlgorithmIdentifierOwned { oid: OID_SHA256, parameters: None };
        let signer = SignerInfo {
            version: CmsVersion::V1,
            sid: SignerIdentifier::IssuerAndSerialNumber(issuer_and_serial(&cert)),
            digest_alg: digest_alg.clone(),
            signed_attrs: Some(attrs),
            signature_algorithm: rsa_algorithm()?,
            signature: OctetString::new(signature).map_err(failed)?,
            unsigned_attrs: None,
        };
        let data = SignedData {
            version: CmsVersion::V1,
            digest_algorithms: SetOfVec::try_from(vec![digest_alg]).map_err(failed)?,
            encap_content_info: EncapsulatedContentInfo { econtent_type: OID_DATA, econtent: None },
            certificates: Some(CertificateSet::try_from(vec![CertificateChoices::Certificate(cert)]).map_err(failed)?),
            crls: None,
            signer_infos: SignerInfos(SetOfVec::try_from(vec![signer]).map_err(failed)?),
        };
        let info = ContentInfo { content_type: OID_SIGNED_DATA, content: Any::encode_from(&data).map_err(failed)? };
        info.to_der().map_err(failed)
    }

    /// Enveloped data (DER) for the recipients' certificates: a fresh AES-256 key and IV, the key
    /// wrapped for each recipient with RSA PKCS#1 v1.5, the content AES-256-CBC.
    pub fn envelope(content: &[u8], certs: &[&[u8]]) -> Result<Vec<u8>, BuildError> {
        use cbc::cipher::{block_padding::Pkcs7, BlockEncryptMut, KeyIvInit};
        use rsa::rand_core::RngCore;
        let mut cek = zeroize::Zeroizing::new([0u8; 32]);
        let mut iv = [0u8; 16];
        SysRng.fill_bytes(cek.as_mut());
        SysRng.fill_bytes(&mut iv);
        let cipher_text = cbc::Encryptor::<aes::Aes256>::new_from_slices(cek.as_ref(), &iv)
            .map_err(failed)?
            .encrypt_padded_vec_mut::<Pkcs7>(content);
        let mut recipients: Vec<RecipientInfo> = Vec::new();
        for der in certs {
            let cert = Certificate::from_der(der).map_err(failed)?;
            let spki = cert.tbs_certificate.subject_public_key_info.to_der().map_err(failed)?;
            let public = <RsaPublicKey as rsa::pkcs8::DecodePublicKey>::from_public_key_der(&spki)
                .map_err(|_| BuildError::Unsupported("Only RSA certificates can be encrypted to here.".into()))?;
            let wrapped = public
                .encrypt(&mut SysRng, Pkcs1v15Encrypt, cek.as_ref())
                .map_err(|_| BuildError::Failed("The message could not be encrypted.".into()))?;
            recipients.push(RecipientInfo::Ktri(KeyTransRecipientInfo {
                version: CmsVersion::V0,
                rid: RecipientIdentifier::IssuerAndSerialNumber(issuer_and_serial(&cert)),
                key_enc_alg: rsa_algorithm()?,
                enc_key: OctetString::new(wrapped).map_err(failed)?,
            }));
        }
        let data = EnvelopedData {
            version: CmsVersion::V0,
            originator_info: None,
            recip_infos: RecipientInfos(SetOfVec::try_from(recipients).map_err(failed)?),
            encrypted_content: EncryptedContentInfo {
                content_type: OID_DATA,
                content_enc_alg: AlgorithmIdentifierOwned {
                    oid: OID_AES256_CBC,
                    parameters: Some(
                        Any::encode_from(&OctetString::new(iv.to_vec()).map_err(failed)?).map_err(failed)?,
                    ),
                },
                encrypted_content: Some(OctetString::new(cipher_text).map_err(failed)?),
            },
            unprotected_attrs: None,
        };
        let info = ContentInfo { content_type: OID_ENVELOPED_DATA, content: Any::encode_from(&data).map_err(failed)? };
        info.to_der().map_err(failed)
    }
}

pub use cmsbuild::{envelope as smime_envelope, sign_detached as smime_sign_detached};

fn smime_signed_entity(signed: &Entity, cert: &[u8], pkcs8: &[u8], now: i64) -> Result<Entity, BuildError> {
    let bytes = signed.bytes();
    let signature = smime_sign_detached(cert, pkcs8, &bytes, now)?;
    let part = format!(
        "Content-Type: application/pkcs7-signature; name=\"smime.p7s\"\r\nContent-Transfer-Encoding: base64\r\nContent-Disposition: attachment; filename=\"smime.p7s\"\r\n\r\n{}",
        base64_lines(&signature)
    );
    Ok(signed_entity("application/pkcs7-signature", "sha-256", &bytes, &part))
}

fn smime_enveloped_entity(content: &Entity, certs: &[&[u8]]) -> Result<Entity, BuildError> {
    let der = smime_envelope(&content.bytes(), certs)?;
    Ok(Entity {
        headers: "Content-Type: application/pkcs7-mime; smime-type=enveloped-data; name=\"smime.p7m\"\r\nContent-Transfer-Encoding: base64\r\nContent-Disposition: attachment; filename=\"smime.p7m\"\r\n".into(),
        body: base64_lines(&der).into_bytes(),
    })
}

// ---- the whole message --------------------------------------------------------------------------

fn date_text(at: i64) -> String {
    chrono::DateTime::from_timestamp(at, 0)
        .map(|d| d.format("%a, %d %b %Y %H:%M:%S +0000").to_string())
        .unwrap_or_else(|| "Thu, 01 Jan 1970 00:00:00 +0000".into())
}

/// A fresh Message-ID (no brackets) for a sender address.
pub fn new_message_id(sender: &str) -> String {
    let domain = sender.rsplit_once('@').map_or("localhost", |(_, d)| d.trim());
    format!("{}@{}", random_hex(16), if domain.is_empty() { "localhost" } else { domain })
}

/// Writes the finished message: the outer headers, then the protected entity. Nothing is left out
/// of protection but the headers.
pub fn build(headers: &Headers, text: &str, files: &[File], protect: &Protect<'_>) -> Result<Vec<u8>, BuildError> {
    if protect.sign.is_none() && protect.encrypt.is_none() {
        return Err(BuildError::Failed("Nothing to protect.".into()));
    }
    let inner = content_entity(text, files);
    let entity = match (&protect.sign, &protect.encrypt) {
        (Some(Signer::Pgp { key, passphrase }), None) => {
            let bytes = inner.bytes();
            let armored = pgp_sign_detached(key, passphrase, &bytes)?;
            let part = format!(
                "Content-Type: application/pgp-signature; name=\"signature.asc\"\r\nContent-Description: OpenPGP digital signature\r\nContent-Disposition: attachment; filename=\"signature.asc\"\r\n\r\n{}",
                armored.replace("\r\n", "\n").replace('\n', "\r\n").trim_end_matches("\r\n").to_string() + "\r\n"
            );
            signed_entity("application/pgp-signature", "pgp-sha256", &bytes, &part)
        }
        (sign, Some(Encrypt::Pgp { keys, hide })) => {
            let signing = match sign {
                Some(Signer::Pgp { key, passphrase }) => Some((*key, *passphrase)),
                Some(Signer::Smime { .. }) => {
                    return Err(BuildError::Failed("OpenPGP encryption cannot be signed with an S/MIME key.".into()))
                }
                None => None,
            };
            let armored = pgp_encrypt(&inner.bytes(), keys, *hide, signing, protect.now)?;
            pgp_encrypted_entity(&armored)
        }
        (Some(Signer::Smime { cert, pkcs8 }), None) => smime_signed_entity(&inner, cert, pkcs8, protect.now)?,
        (sign, Some(Encrypt::Smime { certs })) => {
            let content = match sign {
                Some(Signer::Smime { cert, pkcs8 }) => smime_signed_entity(&inner, cert, pkcs8, protect.now)?,
                Some(Signer::Pgp { .. }) => {
                    return Err(BuildError::Failed("S/MIME encryption cannot be signed with an OpenPGP key.".into()))
                }
                None => inner,
            };
            smime_enveloped_entity(&content, certs)?
        }
        (None, None) => unreachable!("checked above"),
    };
    let mut out = String::new();
    out.push_str(&header_line("From", &address_text(&headers.from)));
    if !headers.to.is_empty() {
        out.push_str(&header_line("To", &address_list(&headers.to)));
    }
    if !headers.cc.is_empty() {
        out.push_str(&header_line("Cc", &address_list(&headers.cc)));
    }
    out.push_str(&header_line("Subject", &encode_text(&headers.subject)));
    out.push_str(&header_line("Date", &date_text(headers.date)));
    out.push_str(&format!("Message-ID: <{}>\r\n", headers.message_id));
    if !headers.in_reply_to.is_empty() {
        let list: Vec<String> = headers.in_reply_to.iter().map(|i| format!("<{i}>")).collect();
        out.push_str(&header_line("In-Reply-To", &list.join(" ")));
    }
    if !headers.references.is_empty() {
        let list: Vec<String> = headers.references.iter().map(|i| format!("<{i}>")).collect();
        out.push_str(&header_line("References", &list.join(" ")));
    }
    out.push_str("MIME-Version: 1.0\r\n");
    if let Some((addr, keydata)) = &protect.autocrypt {
        let encoded = STANDARD.encode(keydata);
        let folded: Vec<&str> =
            encoded.as_bytes().chunks(72).map(|c| std::str::from_utf8(c).unwrap_or_default()).collect();
        out.push_str(&format!("Autocrypt: addr={addr}; keydata=\r\n {}\r\n", folded.join("\r\n ")));
    }
    out.push_str(&entity.headers);
    out.push_str("\r\n");
    let mut bytes = out.into_bytes();
    bytes.extend_from_slice(&entity.body);
    Ok(bytes)
}

/// A short fingerprint of the finished message, for tests that a retry carries the same bytes.
pub fn digest_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quoted_printable_is_canonical_and_short_lined() {
        let q = quoted_printable("Héllo = world \nsecond line\n");
        assert_eq!(q, "H=C3=A9llo =3D world=20\r\nsecond line\r\n");
        let long = quoted_printable(&"a".repeat(200));
        assert!(long.split("\r\n").all(|l| l.len() <= 76), "{long}");
        assert!(long.contains("=\r\n"));
    }

    #[test]
    fn headers_with_non_ascii_text_are_encoded() {
        assert_eq!(encode_text("Plain subject"), "Plain subject");
        let e = encode_text("Grüße aus Köln, ein langer Betreff mit vielen Wörtern");
        assert!(e.starts_with("=?UTF-8?B?"), "{e}");
        assert!(e.lines().all(|l| l.len() <= 76), "{e}");
        assert_eq!(
            address_text(&Addr { name: "Doe, Jane".into(), email: "j@x.org".into() }),
            "\"Doe, Jane\" <j@x.org>"
        );
        assert_eq!(address_text(&Addr { name: String::new(), email: "j@x.org".into() }), "j@x.org");
    }

    #[test]
    fn file_names_that_are_not_ascii_use_the_extended_form() {
        assert_eq!(param("filename", "a.txt"), "filename=\"a.txt\"");
        assert_eq!(param("filename", "ü.txt"), "filename*=UTF-8''%C3%BC.txt");
    }
}
