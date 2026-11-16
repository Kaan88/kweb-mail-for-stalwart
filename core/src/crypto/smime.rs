//! S/MIME (CMS, RFC 5652) with RustCrypto: certificates and signature checks.
//!
//! What is checked here is the signature: that the signed bytes are the ones that were signed
//! and that the certificate's key made the signature. What is NOT checked is whether anybody
//! vouches for the certificate (no chain to an authority, no revocation): that is stage 5 of the
//! plan, and the callers say so in the words they show.

use cms::{
    cert::CertificateChoices,
    content_info::ContentInfo,
    signed_data::{SignedData, SignerIdentifier, SignerInfo},
};
use der::{asn1::ObjectIdentifier, Decode, Encode, Tagged};
use sha2::{Digest, Sha256, Sha384, Sha512};
use x509_cert::{
    ext::pkix::{name::GeneralName, SubjectAltName, SubjectKeyIdentifier},
    Certificate,
};

const OID_SIGNED_DATA: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.2.840.113549.1.7.2");
const OID_ENVELOPED_DATA: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.2.840.113549.1.7.3");
const OID_DATA: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.2.840.113549.1.7.1");
const OID_CONTENT_TYPE: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.2.840.113549.1.9.3");
const OID_MESSAGE_DIGEST: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.2.840.113549.1.9.4");
const OID_SIGNING_TIME: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.2.840.113549.1.9.5");
const OID_EMAIL: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.2.840.113549.1.9.1");
const OID_COMMON_NAME: ObjectIdentifier = ObjectIdentifier::new_unwrap("2.5.4.3");
const OID_SAN: ObjectIdentifier = ObjectIdentifier::new_unwrap("2.5.29.17");
const OID_SKI: ObjectIdentifier = ObjectIdentifier::new_unwrap("2.5.29.14");

const OID_SHA1: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.3.14.3.2.26");
const OID_SHA256: ObjectIdentifier = ObjectIdentifier::new_unwrap("2.16.840.1.101.3.4.2.1");
const OID_SHA384: ObjectIdentifier = ObjectIdentifier::new_unwrap("2.16.840.1.101.3.4.2.2");
const OID_SHA512: ObjectIdentifier = ObjectIdentifier::new_unwrap("2.16.840.1.101.3.4.2.3");

const OID_RSA: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.2.840.113549.1.1.1");
const OID_RSA_PSS: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.2.840.113549.1.1.10");
const OID_EC_KEY: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.2.840.10045.2.1");
const OID_P256: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.2.840.10045.3.1.7");
const OID_P384: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.3.132.0.34");

/// What a certificate says about itself, for the keyring and for the status line.
#[derive(Debug, Clone, PartialEq)]
pub struct CertInfo {
    /// SHA-256 of the DER certificate, upper-case hex.
    pub fingerprint: String,
    /// The common name, or the whole subject when there is none.
    pub subject: String,
    /// The mail addresses it names (subject alternative names and the subject's e-mail attribute), lower case.
    pub emails: Vec<String>,
    pub not_before: i64,
    pub not_after: i64,
}

pub fn hex_upper(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02X}")).collect()
}

/// The certificates in `data`: PEM blocks (any number) or one DER certificate.
pub fn parse_certificates(data: &[u8]) -> Vec<Vec<u8>> {
    let text = String::from_utf8_lossy(data);
    let mut out = Vec::new();
    if text.contains("-----BEGIN CERTIFICATE-----") {
        let mut rest: &str = &text;
        while let Some(start) = rest.find("-----BEGIN CERTIFICATE-----") {
            let Some(len) = rest[start..].find("-----END CERTIFICATE-----") else { break };
            let end = start + len + "-----END CERTIFICATE-----".len();
            let block: String =
                rest[start..end].lines().filter(|l| !l.starts_with("-----")).collect::<Vec<_>>().join("");
            if let Some(der) = crate::crypto::mime::base64_lenient(block.as_bytes()) {
                if Certificate::from_der(&der).is_ok() {
                    out.push(der);
                }
            }
            rest = &rest[end..];
        }
    } else if Certificate::from_der(data).is_ok() {
        out.push(data.to_vec());
    }
    out
}

fn any_text(any: &der::Any) -> Option<String> {
    use der::Tag;
    match any.tag() {
        Tag::Utf8String | Tag::PrintableString | Tag::Ia5String | Tag::TeletexString => {
            Some(String::from_utf8_lossy(any.value()).into_owned())
        }
        _ => None,
    }
}

pub fn cert_info(der: &[u8]) -> Result<CertInfo, String> {
    let cert = Certificate::from_der(der).map_err(|_| "not a certificate".to_string())?;
    let tbs = &cert.tbs_certificate;
    let (mut common, mut emails) = (None, Vec::new());
    for rdn in tbs.subject.0.iter() {
        for atv in rdn.0.iter() {
            if atv.oid == OID_COMMON_NAME {
                common = common.or_else(|| any_text(&atv.value));
            } else if atv.oid == OID_EMAIL {
                emails.extend(any_text(&atv.value).map(|e| e.to_ascii_lowercase()));
            }
        }
    }
    for ext in tbs.extensions.iter().flatten().filter(|e| e.extn_id == OID_SAN) {
        if let Ok(SubjectAltName(names)) = SubjectAltName::from_der(ext.extn_value.as_bytes()) {
            for name in names {
                if let GeneralName::Rfc822Name(mail) = name {
                    emails.push(mail.as_str().to_ascii_lowercase());
                }
            }
        }
    }
    emails.sort();
    emails.dedup();
    let seconds = |t: &x509_cert::time::Time| t.to_unix_duration().as_secs() as i64;
    Ok(CertInfo {
        fingerprint: hex_upper(&Sha256::digest(der)),
        subject: common.unwrap_or_else(|| tbs.subject.to_string()),
        emails,
        not_before: seconds(&tbs.validity.not_before),
        not_after: seconds(&tbs.validity.not_after),
    })
}

fn subject_key_id(cert: &Certificate) -> Option<Vec<u8>> {
    let ext = cert.tbs_certificate.extensions.iter().flatten().find(|e| e.extn_id == OID_SKI)?;
    SubjectKeyIdentifier::from_der(ext.extn_value.as_bytes()).ok().map(|s| s.0.as_bytes().to_vec())
}

fn names_signer(cert: &Certificate, sid: &SignerIdentifier) -> bool {
    match sid {
        SignerIdentifier::IssuerAndSerialNumber(iss) => {
            cert.tbs_certificate.serial_number == iss.serial_number
                && cert.tbs_certificate.issuer.to_der().ok() == iss.issuer.to_der().ok()
        }
        SignerIdentifier::SubjectKeyIdentifier(ski) => subject_key_id(cert).is_some_and(|k| k == ski.0.as_bytes()),
    }
}

/// How an S/MIME signature stands.
#[derive(Debug, Clone, PartialEq)]
pub enum Verdict {
    /// The signature is right for the content and made by the certificate's key.
    Valid {
        cert: CertInfo,
        cert_der: Vec<u8>,
        /// The certificate came from the keyring (the user imported it), not only from the message.
        from_keyring: bool,
        signed_at: Option<i64>,
    },
    /// A certificate was found for the signer but the signature does not fit.
    Invalid,
    /// No certificate for the signer came with the message or is in the keyring.
    NoCert,
    /// A way of signing this program does not do. The text says which, in words.
    Unsupported(String),
    Unreadable,
}

/// A checked signature and, for opaque signed data, the signed content.
#[derive(Debug, Clone, PartialEq)]
pub struct Signed {
    pub verdict: Verdict,
    /// The content inside opaque signed data (it is only meaningful when the verdict is `Valid`).
    pub content: Option<Vec<u8>>,
    /// The certificates that came with the message (DER), for offering them to the user.
    pub embedded: Vec<Vec<u8>>,
}

/// Whether the DER data is CMS signed data (`Some(true)`), enveloped data (`Some(false)`), or neither.
pub fn is_signed_data(der: &[u8]) -> Option<bool> {
    let info = ContentInfo::from_der(der).ok()?;
    match info.content_type {
        t if t == OID_SIGNED_DATA => Some(true),
        t if t == OID_ENVELOPED_DATA => Some(false),
        _ => None,
    }
}

/// Checks the signed data `der`. `detached` is the signed content of a `multipart/signed`
/// message (already in CRLF form); `None` for opaque signed data, whose content is inside.
/// `keyring` holds the DER certificates the user has imported.
pub fn verify(der: &[u8], detached: Option<&[u8]>, keyring: &[Vec<u8>], now: i64) -> Signed {
    let unreadable = || Signed { verdict: Verdict::Unreadable, content: None, embedded: Vec::new() };
    let Ok(info) = ContentInfo::from_der(der) else { return unreadable() };
    if info.content_type != OID_SIGNED_DATA {
        return unreadable();
    }
    let Ok(data) = info.content.decode_as::<SignedData>() else { return unreadable() };
    let mut embedded: Vec<Vec<u8>> = Vec::new();
    if let Some(set) = &data.certificates {
        for choice in set.0.iter() {
            if let CertificateChoices::Certificate(cert) = choice {
                if let Ok(bytes) = cert.to_der() {
                    embedded.push(bytes);
                }
            }
        }
    }
    let content: Option<Vec<u8>> = match (&data.encap_content_info.econtent, detached) {
        (_, Some(bytes)) => Some(bytes.to_vec()),
        (Some(any), None) => any.decode_as::<der::asn1::OctetString>().ok().map(|o| o.as_bytes().to_vec()),
        (None, None) => None,
    };
    let done = |verdict: Verdict, content: Option<Vec<u8>>| Signed { verdict, content, embedded: embedded.clone() };
    let Some(bytes) = content else { return done(Verdict::Unreadable, None) };
    // Every signer must check out; the first one that does not decides the verdict.
    let mut last = None;
    for signer in data.signer_infos.0.iter() {
        let verdict = verify_signer(signer, &data, &bytes, &embedded, keyring, now);
        let good = matches!(verdict, Verdict::Valid { .. });
        last = Some(verdict);
        if !good {
            break;
        }
    }
    match last {
        Some(verdict @ Verdict::Valid { .. }) => done(verdict, Some(bytes)),
        Some(verdict) => done(verdict, None),
        None => done(Verdict::Unreadable, None),
    }
}

/// Why a check could not be made at all (as opposed to failing).
enum Cannot {
    Unreadable,
    Unsupported(String),
}

impl From<Cannot> for Verdict {
    fn from(c: Cannot) -> Verdict {
        match c {
            Cannot::Unreadable => Verdict::Unreadable,
            Cannot::Unsupported(why) => Verdict::Unsupported(why),
        }
    }
}

fn digest(alg: &ObjectIdentifier, data: &[u8]) -> Result<Vec<u8>, Cannot> {
    Ok(match *alg {
        OID_SHA256 => Sha256::digest(data).to_vec(),
        OID_SHA384 => Sha384::digest(data).to_vec(),
        OID_SHA512 => Sha512::digest(data).to_vec(),
        OID_SHA1 => return Err(Cannot::Unsupported("the signature uses SHA-1, which is too weak to trust".into())),
        _ => return Err(Cannot::Unsupported("the signature uses a hash this app does not know".into())),
    })
}

fn verify_signer(
    signer: &SignerInfo,
    data: &SignedData,
    content: &[u8],
    embedded: &[Vec<u8>],
    keyring: &[Vec<u8>],
    now: i64,
) -> Verdict {
    // The certificate: the user's own copy first (what they trust), then the one in the message.
    let find = |list: &[Vec<u8>]| -> Option<(Vec<u8>, Certificate)> {
        list.iter().find_map(|der| {
            let cert = Certificate::from_der(der).ok()?;
            names_signer(&cert, &signer.sid).then(|| (der.clone(), cert))
        })
    };
    let (from_keyring, (cert_der, cert)) = match find(keyring) {
        Some(found) => (true, found),
        None => match find(embedded) {
            Some(found) => (false, found),
            None => return Verdict::NoCert,
        },
    };
    let _ = data;
    let hash = match digest(&signer.digest_alg.oid, content) {
        Ok(h) => h,
        Err(cannot) => return cannot.into(),
    };
    let mut signed_at = None;
    let to_check: Vec<u8> = match &signer.signed_attrs {
        Some(attrs) => {
            let (mut digest_attr, mut type_ok) = (None, false);
            for attr in attrs.iter() {
                let Some(value) = attr.values.iter().next() else { continue };
                if attr.oid == OID_MESSAGE_DIGEST {
                    digest_attr = value.decode_as::<der::asn1::OctetString>().ok().map(|o| o.as_bytes().to_vec());
                } else if attr.oid == OID_CONTENT_TYPE {
                    type_ok = value.decode_as::<ObjectIdentifier>().is_ok_and(|t| t == OID_DATA);
                } else if attr.oid == OID_SIGNING_TIME {
                    signed_at = signing_time(value);
                }
            }
            if digest_attr.as_deref() != Some(hash.as_slice()) || !type_ok {
                return Verdict::Invalid;
            }
            match attrs.to_der() {
                Ok(bytes) => bytes,
                Err(_) => return Verdict::Unreadable,
            }
        }
        None => content.to_vec(),
    };
    let signature = signer.signature.as_bytes();
    match check_signature(&cert, signer, &to_check, signature) {
        Ok(true) => {}
        Ok(false) => return Verdict::Invalid,
        Err(cannot) => return cannot.into(),
    }
    let Ok(info) = cert_info(&cert_der) else { return Verdict::Unreadable };
    let _ = now;
    Verdict::Valid { cert: info, cert_der, from_keyring, signed_at }
}

fn signing_time(value: &der::Any) -> Option<i64> {
    use der::Tag;
    match value.tag() {
        Tag::UtcTime => value.decode_as::<der::asn1::UtcTime>().ok().map(|t| t.to_unix_duration().as_secs() as i64),
        Tag::GeneralizedTime => {
            value.decode_as::<der::asn1::GeneralizedTime>().ok().map(|t| t.to_unix_duration().as_secs() as i64)
        }
        _ => None,
    }
}

/// `Ok(false)`: the signature does not match. `Err`: this program cannot tell.
fn check_signature(cert: &Certificate, signer: &SignerInfo, data: &[u8], signature: &[u8]) -> Result<bool, Cannot> {
    use p256::ecdsa::signature::hazmat::PrehashVerifier;
    let spki = &cert.tbs_certificate.subject_public_key_info;
    let sig_alg = signer.signature_algorithm.oid;
    if sig_alg == OID_RSA_PSS {
        return Err(Cannot::Unsupported("the signature uses RSA-PSS, which this app does not check yet".into()));
    }
    let prehash = digest(&signer.digest_alg.oid, data)?;
    if spki.algorithm.oid == OID_RSA {
        use rsa::{pkcs8::DecodePublicKey, Pkcs1v15Sign, RsaPublicKey};
        let der = spki.to_der().map_err(|_| Cannot::Unreadable)?;
        let key = RsaPublicKey::from_public_key_der(&der).map_err(|_| Cannot::Unreadable)?;
        let scheme = match signer.digest_alg.oid {
            OID_SHA256 => Pkcs1v15Sign::new::<Sha256>(),
            OID_SHA384 => Pkcs1v15Sign::new::<Sha384>(),
            _ => Pkcs1v15Sign::new::<Sha512>(),
        };
        return Ok(key.verify(scheme, &prehash, signature).is_ok());
    }
    if spki.algorithm.oid == OID_EC_KEY {
        let curve = spki.algorithm.parameters.as_ref().and_then(|p| p.decode_as::<ObjectIdentifier>().ok());
        let point = spki.subject_public_key.raw_bytes();
        return match curve {
            Some(OID_P256) => {
                let key = p256::ecdsa::VerifyingKey::from_sec1_bytes(point).map_err(|_| Cannot::Unreadable)?;
                let sig = p256::ecdsa::Signature::from_der(signature).map_err(|_| Cannot::Unreadable)?;
                Ok(key.verify_prehash(&prehash, &sig).is_ok())
            }
            Some(OID_P384) => {
                let key = p384::ecdsa::VerifyingKey::from_sec1_bytes(point).map_err(|_| Cannot::Unreadable)?;
                let sig = p384::ecdsa::Signature::from_der(signature).map_err(|_| Cannot::Unreadable)?;
                Ok(key.verify_prehash(&prehash, &sig).is_ok())
            }
            _ => Err(Cannot::Unsupported("the certificate uses an elliptic curve this app does not know".into())),
        };
    }
    Err(Cannot::Unsupported("the certificate uses a kind of key this app does not know".into()))
}

/// Whether `at` (a Unix time) lies within the certificate's validity.
pub fn valid_at(cert: &CertInfo, at: i64) -> bool {
    cert.not_before <= at && at <= cert.not_after
}
