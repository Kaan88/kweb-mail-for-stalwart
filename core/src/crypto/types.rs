//! What the host sees of the encryption feature: plain records and enums, no secrets.

/// Which kind of signed or encrypted mail a message is, judged from the type structure alone
/// (no raw bytes needed). `None` for nearly all mail.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, uniffi::Enum)]
pub enum CryptoKind {
    None,
    /// `multipart/signed` with an OpenPGP signature.
    PgpSigned,
    /// `multipart/encrypted` (OpenPGP).
    PgpEncrypted,
    /// Armored OpenPGP text in the body (clear-signed or encrypted).
    PgpInline,
    /// `multipart/signed` with an S/MIME signature.
    SmimeSigned,
    /// `application/pkcs7-mime`: signed or encrypted S/MIME data inside one part.
    SmimeOpaque,
}

/// How the signature of a message stands. Only `Verified` is good news.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum SignatureState {
    NotSigned,
    /// A valid signature from a key or certificate the user has imported, and the signer's address
    /// is the From address.
    Verified,
    /// As `Verified`, but the key is not the one first seen for this address: a warning.
    KeyChanged,
    /// S/MIME: a valid signature from the certificate that came with the message, whose address is
    /// the From address. Nobody vouches for the certificate (it is not checked against an authority).
    CertificateNotChecked,
    /// The signature is valid, but the signer's address is not the From address.
    OtherAddress,
    /// The signing key or certificate is not known, so nothing can be said.
    NoKey,
    /// The signature does not fit the message: it was changed, or the signature is false.
    Invalid,
    /// The signature is valid, but the key or certificate had expired when it was used.
    KeyExpired,
    /// The signature is valid, but the key had been revoked.
    KeyRevoked,
    /// A way of signing this app cannot check (the notes say which).
    Unsupported,
    /// The signature data could not be read.
    Unreadable,
}

/// What became of the encryption of a message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum EncryptionState {
    NotEncrypted,
    /// Opened: the content shown is the decrypted one.
    Decrypted,
    /// Encrypted, and none of the user's keys is one it was made for.
    NeedKey,
    /// A key that fits needs its passphrase.
    NeedPassphrase,
    /// The passphrase given did not open the key.
    WrongPassphrase,
    /// The message has no protection against change (or it failed): not opened, nothing shown.
    NoIntegrity,
    /// A way of encrypting this app cannot open.
    Unsupported,
    /// It could not be opened (damaged data).
    Failed,
}

/// Something worth telling about a message beyond the two states, as a code the host words.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum CryptoNote {
    /// Only a part of the message is signed or encrypted; the rest is not covered.
    OnlyPartCovered,
    /// Remote content stays off for this message whatever the user's setting is.
    RemoteContentBlocked,
    /// Text around the signed block was left out because the signature does not cover it.
    UnsignedTextHidden,
    /// The signature carries an expiry that had passed.
    SignatureExpired,
    /// The message was signed inside the encryption, and that signature is the one shown.
    SignedInsideEncryption,
    /// Encrypted mail cannot be searched on the server.
    NotSearchable,
    /// The signer's address was checked against the first From address only.
    SmimeNoRevocationCheck,
}

/// Where a key or certificate came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum KeyOrigin {
    File,
    Pasted,
    /// Imported from a message the user chose to take it from.
    Mail,
    /// The user's own key (has a private part on this phone).
    Own,
}

impl KeyOrigin {
    pub fn code(self) -> &'static str {
        match self {
            KeyOrigin::File => "file",
            KeyOrigin::Pasted => "paste",
            KeyOrigin::Mail => "mail",
            KeyOrigin::Own => "own",
        }
    }

    pub fn from_code(code: &str) -> KeyOrigin {
        match code {
            "paste" => KeyOrigin::Pasted,
            "mail" => KeyOrigin::Mail,
            "own" => KeyOrigin::Own,
            _ => KeyOrigin::File,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum KeyKind {
    Pgp,
    Smime,
}

impl KeyKind {
    pub fn code(self) -> &'static str {
        match self {
            KeyKind::Pgp => "pgp",
            KeyKind::Smime => "x509",
        }
    }
}

/// One entry of the keyring: somebody's public key or certificate, or the user's own.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct KeyEntry {
    pub id: String,
    pub kind: KeyKind,
    /// A name to show: the user id or the certificate's common name.
    pub label: String,
    pub emails: Vec<String>,
    /// Upper-case hex, as the other tools show it.
    pub fingerprint: String,
    pub origin: KeyOrigin,
    /// The person or message it came from, when it came from mail (the sender's address).
    pub origin_detail: String,
    pub added_at: i64,
    /// A private part is kept on this phone: the key is the user's own.
    pub has_secret: bool,
    /// The private part is protected by a passphrase the user has to enter.
    pub needs_passphrase: bool,
    /// The passphrase is kept on this phone (the user chose "keep unlocked").
    pub unlocked: bool,
    /// When it stops being valid, if it does (Unix seconds).
    pub expires_at: Option<i64>,
}

/// A key found in a message that the user may add to the keyring.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct KeyOffer {
    pub id: String,
    pub kind: KeyKind,
    pub label: String,
    pub emails: Vec<String>,
    pub fingerprint: String,
    /// It is in the keyring already.
    pub already_known: bool,
}

/// Who signed a message, as far as the signature says.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct Signer {
    pub label: String,
    pub emails: Vec<String>,
    pub fingerprint: String,
    pub kind: KeyKind,
    /// The key or certificate is in the user's keyring (not only in the message).
    pub in_keyring: bool,
    pub signed_at: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct CryptoAttachment {
    /// What `crypto_save_attachment` takes.
    pub index: u32,
    pub name: String,
    pub mime_type: String,
    pub size: u64,
    pub content_id: Option<String>,
    pub is_inline: bool,
}

/// The content of a message when the core, not the server, produced it: decrypted or opaque
/// signed mail. It is shown on its own, never merged with anything else.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct CryptoContent {
    pub html: Option<String>,
    pub text: Option<String>,
    pub attachments: Vec<CryptoAttachment>,
}

/// Everything the thread screen needs about the signature and encryption of one message.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct CryptoView {
    pub kind: CryptoKind,
    pub signature: SignatureState,
    pub signer: Option<Signer>,
    /// The From address the signer was compared with.
    pub from: String,
    pub encryption: EncryptionState,
    /// The status covers the whole message (not just one part of it).
    pub covers_whole_message: bool,
    /// Present when the message has to be shown from this (decrypted or opaque): the server's
    /// own version of the body is empty or not to be trusted for it.
    pub content: Option<CryptoContent>,
    pub offers: Vec<KeyOffer>,
    pub notes: Vec<CryptoNote>,
    /// Remote content must not load for this message, whatever the setting says.
    pub remote_content_blocked: bool,
    /// For `NeedPassphrase` and `WrongPassphrase`: the key to ask about.
    pub passphrase_for: Option<String>,
    /// For `Unsupported`, `Unreadable` and the like: a short plain reason (never any of the content).
    pub detail: Option<String>,
}

impl CryptoView {
    pub fn plain(kind: CryptoKind) -> CryptoView {
        CryptoView {
            kind,
            signature: SignatureState::NotSigned,
            signer: None,
            from: String::new(),
            encryption: EncryptionState::NotEncrypted,
            covers_whole_message: true,
            content: None,
            offers: Vec::new(),
            notes: Vec::new(),
            remote_content_blocked: false,
            passphrase_for: None,
            detail: None,
        }
    }
}
