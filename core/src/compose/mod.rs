//! Writing, saving and sending messages.
//!
//! [`rules`] holds the pure rules; this file builds the drafts a reply, a forward or an opened
//! server draft start from, and finds recipients to suggest; [`flush`] is the part of a pass
//! that brings the server up to date with the drafts and sends what is due.
//!
//! A draft lives in the cache first. The server's copy in Drafts is a backup that a pass keeps
//! current, so nothing typed is lost offline, and a message is sent only from a copy the
//! server holds (see `flush`).

mod flush;
pub mod later;
pub mod rules;

use std::collections::HashSet;

pub use flush::{flush, refresh_directory, Flushed};

use crate::{
    jmap::HeaderIds,
    model::{Address, Attachment, Draft, Identity, MessageBody, MessageHeader},
    store::{Store, Threading},
    Error, Result,
};
use rules::{address_key, match_rank};

/// How many of the newest cached messages are looked at for people to suggest.
const MAIL_SCAN: u32 = 5000;

/// A fresh draft id: random, hex, never reused.
pub fn new_id() -> Result<String> {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).map_err(|e| Error::temporary(format!("random numbers: {e}")))?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

/// The text of a message to quote: the plain version, else the HTML rendered as text.
pub(crate) fn source_text(body: &MessageBody) -> String {
    match (&body.text, &body.html) {
        (Some(text), _) if !text.trim().is_empty() => text.clone(),
        (_, Some(html)) => rules::html_to_text(html),
        (text, None) => text.clone().unwrap_or_default(),
    }
}

fn sender(header: &MessageHeader) -> Address {
    Address { name: header.from_name.clone(), email: header.from_email.clone() }
}

/// The files of a message that are files, not pictures its HTML shows.
fn files(body: &MessageBody) -> Vec<Attachment> {
    body.attachments.iter().filter(|a| !a.is_inline).cloned().collect()
}

fn empty_draft(id: String, identity: Option<&Identity>, now: i64) -> Draft {
    Draft {
        id,
        identity_id: identity.map(|i| i.id.clone()).unwrap_or_default(),
        to: Vec::new(),
        cc: Vec::new(),
        bcc: Vec::new(),
        subject: String::new(),
        body: String::new(),
        attachments: Vec::new(),
        source_message_id: None,
        is_forward: false,
        server_id: None,
        send_error: None,
        updated_at: now,
    }
}

fn signature(identity: Option<&Identity>) -> &str {
    identity.map_or("", |i| i.signature.as_str())
}

/// An empty message from the default identity.
pub fn blank(identities: &[Identity], id: String, now: i64) -> Draft {
    let identity = identities.first();
    Draft { body: rules::with_signature("", signature(identity)), ..empty_draft(id, identity, now) }
}

/// An answer to `source`: to its sender, or to everyone on it.
pub fn answer(
    all: bool,
    source: (&MessageHeader, &MessageBody),
    identities: &[Identity],
    id: String,
    now: i64,
) -> Draft {
    let (header, body) = source;
    let recipients = [body.to.as_slice(), body.cc.as_slice()].concat();
    let identity = rules::pick_identity(identities, &recipients);
    let own = rules::own_keys(identities);
    let to = rules::reply_recipients(all, &body.reply_to, &sender(header), &body.to, &body.cc, &own);
    let quoted = rules::quoted_reply(&sender(header), &source_text(body));
    Draft {
        to: to.to,
        cc: to.cc,
        subject: rules::reply_subject(&header.subject),
        body: rules::reply_with_signature(&quoted, signature(identity)),
        source_message_id: Some(header.id.clone()),
        ..empty_draft(id, identity, now)
    }
}

/// `source` passed on, with its files.
pub fn forward(source: (&MessageHeader, &MessageBody), identities: &[Identity], id: String, now: i64) -> Draft {
    let (header, body) = source;
    let identity = rules::pick_identity(identities, &body.to);
    let text = rules::forwarded_text(
        &sender(header),
        body.sent_at.unwrap_or(header.received_at),
        &header.subject,
        &body.to,
        &source_text(body),
    );
    Draft {
        subject: rules::forward_subject(&header.subject),
        body: rules::with_signature(&text, signature(identity)),
        attachments: files(body),
        source_message_id: Some(header.id.clone()),
        is_forward: true,
        ..empty_draft(id, identity, now)
    }
}

/// A draft the server holds, as a local draft that remembers its server copy. It is sent as
/// the identity whose address it is from.
pub fn opened(source: (&MessageHeader, &MessageBody), identities: &[Identity], id: String, now: i64) -> Draft {
    let (header, body) = source;
    let identity = rules::pick_identity(identities, &[sender(header)]);
    Draft {
        to: body.to.clone(),
        cc: body.cc.clone(),
        bcc: body.bcc.clone(),
        subject: header.subject.clone(),
        body: source_text(body),
        attachments: files(body),
        server_id: Some(header.id.clone()),
        ..empty_draft(id, identity, now)
    }
}

/// What an answer to a message carries so that mail programs thread it: it is In-Reply-To the
/// message and References what that message referred to, then the message itself.
pub fn answer_threading(source: &HeaderIds) -> Threading {
    let Some(id) = &source.message_id else { return Threading::default() };
    // A message that names only what it answers (no References) still has a thread behind it:
    // that one id stands in for the list (RFC 5322, 3.6.4).
    let mut references = match (source.references.is_empty(), source.in_reply_to.as_slice()) {
        (true, [parent]) => vec![parent.clone()],
        _ => source.references.clone(),
    };
    if !references.contains(id) {
        references.push(id.clone());
    }
    Threading { in_reply_to: vec![id.clone()], references }
}

/// People that fit what the user typed, best first: the address book, then the people in the
/// cached mail (newest first), each address once. A match at the start of a name or an address
/// comes before one on a later word of a name.
pub fn suggest(store: &Store, account_id: &str, text: &str, limit: u32) -> Result<Vec<Address>> {
    let needle = text.trim().to_lowercase();
    if needle.is_empty() || limit == 0 {
        return Ok(Vec::new());
    }
    let (book, people) = (store.contacts_list(account_id)?, store.people_in_mail(account_id, MAIL_SCAN)?);
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for group in [book, people] {
        let mut hits: Vec<(u8, Address)> =
            group.into_iter().filter_map(|a| match_rank(&needle, &a.name, &a.email).map(|rank| (rank, a))).collect();
        // A stable sort: within one rank the order of the source stays.
        hits.sort_by_key(|(rank, _)| *rank);
        for (_, person) in hits {
            if seen.insert(address_key(&person.email)) {
                out.push(person);
                if out.len() >= limit as usize {
                    return Ok(out);
                }
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn addr(name: &str, email: &str) -> Address {
        Address { name: name.into(), email: email.into() }
    }

    fn identity(id: &str, email: &str, signature: &str) -> Identity {
        Identity { id: id.into(), name: "Me".into(), email: email.into(), signature: signature.into() }
    }

    fn header() -> MessageHeader {
        MessageHeader {
            id: "m1".into(),
            thread_id: "t1".into(),
            mailbox_ids: vec!["inbox".into()],
            from_name: "Ann".into(),
            from_email: "ann@x.org".into(),
            to: Vec::new(),
            subject: "Plans".into(),
            preview: String::new(),
            received_at: 1_767_323_045,
            is_unread: false,
            is_flagged: false,
            has_attachment: false,
            size: 1,
        }
    }

    fn file(blob: &str, inline: bool) -> Attachment {
        Attachment {
            blob_id: blob.into(),
            name: format!("{blob}.pdf"),
            mime_type: "application/pdf".into(),
            size: 4,
            content_id: None,
            is_inline: inline,
        }
    }

    fn body() -> MessageBody {
        MessageBody {
            message_id: "m1".into(),
            blob_id: "b".into(),
            to: vec![addr("Me", "me@x.org"), addr("Bo", "bo@x.org")],
            cc: vec![addr("Cy", "cy@x.org")],
            bcc: Vec::new(),
            reply_to: Vec::new(),
            sent_at: Some(1_767_323_000),
            html: None,
            text: Some("Hello\nthere".into()),
            attachments: vec![file("f1", false), file("pic", true)],
            is_truncated: false,
        }
    }

    fn ids() -> Vec<Identity> {
        vec![identity("1", "other@x.org", ""), identity("2", "me@x.org", "Me\nAcme")]
    }

    #[test]
    fn ids_are_random_hex() {
        let (a, b) = (new_id().unwrap(), new_id().unwrap());
        assert_eq!(a.len(), 32);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(a, b);
    }

    #[test]
    fn a_blank_draft_has_the_default_identity_and_its_signature() {
        let d = blank(&ids(), "d".into(), 7);
        assert_eq!((d.identity_id.as_str(), d.updated_at, d.body.as_str()), ("1", 7, ""));
        let d = blank(&[identity("9", "a@x.org", "Sig")], "d".into(), 7);
        assert_eq!(d.body, "\n\n-- \nSig");
        // No identity yet: the draft can be written, not sent.
        assert_eq!(blank(&[], "d".into(), 7).identity_id, "");
    }

    #[test]
    fn a_reply_is_addressed_quoted_and_signed() {
        let d = answer(false, (&header(), &body()), &ids(), "d".into(), 5);
        assert_eq!(d.identity_id, "2", "sent to me@x.org");
        assert_eq!(d.to, vec![addr("Ann", "ann@x.org")]);
        assert!(d.cc.is_empty());
        assert_eq!(d.subject, "Re: Plans");
        assert_eq!(d.body, "\n\n-- \nMe\nAcme\n\nAnn <ann@x.org> wrote:\n> Hello\n> there");
        assert_eq!((d.source_message_id.as_deref(), d.is_forward, d.server_id), (Some("m1"), false, None));
        assert!(d.attachments.is_empty());
    }

    #[test]
    fn a_reply_to_all_keeps_the_others_without_me() {
        let d = answer(true, (&header(), &body()), &ids(), "d".into(), 5);
        assert_eq!(d.to, vec![addr("Ann", "ann@x.org"), addr("Bo", "bo@x.org")]);
        assert_eq!(d.cc, vec![addr("Cy", "cy@x.org")]);
        let mut b = body();
        b.reply_to = vec![addr("List", "list@x.org")];
        let d = answer(false, (&header(), &b), &ids(), "d".into(), 5);
        assert_eq!(d.to, vec![addr("List", "list@x.org")]);
    }

    #[test]
    fn an_html_only_message_is_quoted_as_text() {
        let mut b = body();
        b.text = None;
        b.html = Some("<p>Hi<br>you</p>".into());
        let d = answer(false, (&header(), &b), &[], "d".into(), 5);
        assert_eq!(d.body, "\n\nAnn <ann@x.org> wrote:\n> Hi\n> you");
        assert_eq!(d.identity_id, "");
    }

    #[test]
    fn a_forward_has_the_header_block_and_the_files_but_not_the_pictures() {
        let d = forward((&header(), &body()), &ids(), "d".into(), 5);
        assert_eq!(d.subject, "Fwd: Plans");
        assert!(d.to.is_empty() && d.cc.is_empty());
        assert!(d.is_forward);
        assert_eq!(d.source_message_id.as_deref(), Some("m1"));
        assert_eq!(d.attachments, vec![file("f1", false)]);
        assert!(d.body.starts_with("\n\n---------- Forwarded message ----------\nFrom: Ann <ann@x.org>\n"));
        let header_block =
            "Date: 2026-01-02T03:03:20Z\nSubject: Plans\nTo: Me <me@x.org>, Bo <bo@x.org>\n\nHello\nthere";
        assert!(d.body.contains(header_block), "{}", d.body);
        assert!(d.body.ends_with("\n\n-- \nMe\nAcme"));
    }

    #[test]
    fn a_server_draft_becomes_a_local_one_that_knows_its_copy() {
        let mut h = header();
        h.from_email = "ME@x.org".into();
        h.subject = "Draft".into();
        let mut b = body();
        b.bcc = vec![addr("", "hid@x.org")];
        let d = opened((&h, &b), &ids(), "d".into(), 5);
        assert_eq!(d.identity_id, "2");
        assert_eq!(
            (d.subject.as_str(), d.body.as_str(), d.server_id.as_deref()),
            ("Draft", "Hello\nthere", Some("m1"))
        );
        assert_eq!((d.to.len(), d.cc.len(), d.bcc.len()), (2, 1, 1));
        assert_eq!(d.attachments, vec![file("f1", false)]);
        assert_eq!((d.source_message_id, d.is_forward), (None, false));
    }

    #[test]
    fn an_answer_is_in_reply_to_the_message_and_references_its_history() {
        let source = HeaderIds {
            message_id: Some("m2@x".into()),
            in_reply_to: vec!["m1@x".into()],
            references: vec!["m0@x".into(), "m1@x".into()],
        };
        let t = answer_threading(&source);
        assert_eq!(t.in_reply_to, vec!["m2@x".to_string()]);
        assert_eq!(t.references, vec!["m0@x".to_string(), "m1@x".to_string(), "m2@x".to_string()]);
        let first = HeaderIds { message_id: Some("a@x".into()), ..HeaderIds::default() };
        assert_eq!(answer_threading(&first).references, vec!["a@x".to_string()]);
        assert_eq!(answer_threading(&HeaderIds::default()), Threading::default());
    }
}
