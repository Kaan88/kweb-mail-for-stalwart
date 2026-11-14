//! Pure conversions from jmap-client objects to the core's model types, and the small
//! arithmetic around them. Kept apart from the network code so it can be tested directly.

use std::collections::{HashMap, HashSet};

use jmap_client::{
    email::{BodyProperty, Email, EmailAddress, EmailBodyPart, Property as EmailProperty},
    mailbox::{Mailbox as JmapMailbox, Property as MailboxProperty, Role},
    Get,
};

use super::Changes;
use crate::model::{Address, Attachment, Mailbox, MessageBody, MessageHeader};

const PREVIEW_CHARS: usize = 200;
/// Used when the server does not state `maxObjectsInGet` (RFC 8620 says at least 500, but
/// a smaller batch costs little).
const FALLBACK_BATCH: usize = 50;

/// Only what the message list and notifications show.
pub(super) fn header_properties() -> Vec<EmailProperty> {
    use EmailProperty::*;
    vec![Id, ThreadId, MailboxIds, Keywords, From, To, Subject, Preview, ReceivedAt, HasAttachment, Size]
}

/// What a message view needs besides the header.
pub(super) fn body_properties() -> Vec<EmailProperty> {
    use EmailProperty::*;
    vec![Id, BlobId, To, Cc, Bcc, ReplyTo, SentAt, TextBody, HtmlBody, Attachments, BodyValues]
}

/// The fields of each body part that `to_body` reads.
pub(super) fn body_part_properties() -> Vec<BodyProperty> {
    use BodyProperty::*;
    vec![PartId, BlobId, Size, Name, Type, Charset, Disposition, Cid]
}

fn addresses(list: Option<&[EmailAddress]>) -> Vec<Address> {
    let one =
        |a: &EmailAddress| Address { name: a.name().unwrap_or_default().to_string(), email: a.email().to_string() };
    list.unwrap_or_default().iter().map(one).collect()
}

pub(super) fn mailbox_properties() -> Vec<MailboxProperty> {
    use MailboxProperty::*;
    vec![Id, Name, ParentId, Role, SortOrder, TotalEmails, UnreadEmails]
}

/// A message without an id cannot be stored, so it is dropped.
pub(super) fn to_header(e: &Email<Get>) -> Option<MessageHeader> {
    let id = e.id()?.to_string();
    let keywords = e.keywords();
    let sender = e.from().and_then(|f| f.first());
    let mut mailbox_ids: Vec<String> = e.mailbox_ids().into_iter().map(String::from).collect();
    mailbox_ids.sort();
    Some(MessageHeader {
        id,
        thread_id: e.thread_id().unwrap_or_default().to_string(),
        mailbox_ids,
        from_name: sender.and_then(|a| a.name()).unwrap_or_default().to_string(),
        from_email: sender.map(|a| a.email().to_string()).unwrap_or_default(),
        to: addresses(e.to()),
        subject: e.subject().unwrap_or_default().to_string(),
        preview: e.preview().unwrap_or_default().chars().take(PREVIEW_CHARS).collect(),
        received_at: e.received_at().unwrap_or(0),
        is_unread: !keywords.contains(&"$seen"),
        is_flagged: keywords.contains(&"$flagged"),
        has_attachment: e.has_attachment(),
        size: e.size() as u64,
    })
}

/// The text of the parts whose type is `mime`, in order, joined with `separator`. `None` when
/// there is no such part (or none with a value). `truncated` is set when a value used was cut.
fn part_text(
    e: &Email<Get>,
    parts: &[EmailBodyPart],
    mime: &str,
    separator: &str,
    truncated: &mut bool,
) -> Option<String> {
    let values: Vec<_> = parts
        .iter()
        .filter(|p| p.content_type().is_some_and(|t| t.eq_ignore_ascii_case(mime)))
        .filter_map(|p| e.body_value(p.part_id()?))
        .collect();
    if values.is_empty() {
        return None;
    }
    *truncated |= values.iter().any(|v| v.is_truncated());
    Some(values.iter().map(|v| v.value()).collect::<Vec<_>>().join(separator))
}

/// The HTML version: the text/html parts of `htmlBody` in order. `None` when there is none.
/// In a message that is not a plain pair of alternatives (text, a file, then HTML, as some
/// mail programs send) the list also holds the text/plain parts that stand between; they are
/// shown where they stand, as escaped text, and not dropped.
fn html_text(e: &Email<Get>, parts: &[EmailBodyPart], truncated: &mut bool) -> Option<String> {
    let is = |p: &EmailBodyPart, mime: &str| p.content_type().is_some_and(|t| t.eq_ignore_ascii_case(mime));
    if !parts.iter().any(|p| is(p, "text/html")) {
        return None;
    }
    let mut out = String::new();
    for part in parts {
        let Some(value) = part.part_id().and_then(|id| e.body_value(id)) else { continue };
        if is(part, "text/html") {
            out.push_str(value.value());
        } else if is(part, "text/plain") {
            out.push_str("<pre style=\"white-space:pre-wrap;font:inherit\">");
            escape_html(value.value(), &mut out);
            out.push_str("</pre>");
        } else {
            continue;
        }
        *truncated |= value.is_truncated();
    }
    Some(out)
}

/// Plain text as HTML text. The result goes into a page that shows untrusted mail, so every
/// character that could start markup or end an attribute is replaced.
fn escape_html(text: &str, out: &mut String) {
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
}

fn is_picture(mime_type: &str) -> bool {
    mime_type.get(..6).is_some_and(|p| p.eq_ignore_ascii_case("image/"))
}

/// Whether the HTML refers to this Content-ID as `cid:<id>`: the whole id, not one that only
/// starts with it.
fn refers_to(html: &str, content_id: &str) -> bool {
    let needle = format!("cid:{content_id}");
    html.match_indices(&needle).any(|(at, _)| {
        html[at + needle.len()..]
            .chars()
            .next()
            .is_none_or(|c| matches!(c, '"' | '\'' | ')' | '>' | '?' | '#') || c.is_whitespace())
    })
}

fn is_text(part: &EmailBodyPart) -> bool {
    part.content_type().is_some_and(|t| t.get(..5).is_some_and(|p| p.eq_ignore_ascii_case("text/")))
}

/// The files of a message: the server's `attachments` list, plus the pictures and the like that
/// sit inside the text and HTML bodies. Each blob once, in that order.
fn files(e: &Email<Get>, html: Option<&str>) -> Vec<Attachment> {
    let listed = e.attachments().unwrap_or_default().iter();
    let inside = [e.text_body(), e.html_body()].into_iter().flatten().flatten().filter(|p| !is_text(p));
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for part in listed.chain(inside) {
        let Some(blob_id) = part.blob_id().filter(|b| seen.insert(b.to_string())) else { continue };
        let content_id = part.content_id().map(|c| c.trim_matches(['<', '>']).to_string()).filter(|c| !c.is_empty());
        let mime_type = part.content_type().unwrap_or("application/octet-stream").to_string();
        let is_inline =
            is_picture(&mime_type) && content_id.as_ref().is_some_and(|c| html.is_some_and(|h| refers_to(h, c)));
        out.push(Attachment {
            blob_id: blob_id.to_string(),
            name: part.name().unwrap_or_default().to_string(),
            mime_type,
            size: part.size() as u64,
            content_id,
            is_inline,
        });
    }
    out
}

/// A message fetched with the body properties. Without an id it cannot be stored: dropped.
pub(super) fn to_body(e: &Email<Get>) -> Option<MessageBody> {
    let mut is_truncated = false;
    let html = html_text(e, e.html_body().unwrap_or_default(), &mut is_truncated);
    let text = part_text(e, e.text_body().unwrap_or_default(), "text/plain", "\n", &mut is_truncated);
    Some(MessageBody {
        message_id: e.id()?.to_string(),
        blob_id: e.blob_id().unwrap_or_default().to_string(),
        to: addresses(e.to()),
        cc: addresses(e.cc()),
        bcc: addresses(e.bcc()),
        reply_to: addresses(e.reply_to()),
        sent_at: e.sent_at(),
        attachments: files(e, html.as_deref()),
        html,
        text,
        is_truncated,
    })
}

pub(super) fn to_mailbox(m: &JmapMailbox<Get>) -> Option<Mailbox> {
    Some(Mailbox {
        id: m.id()?.to_string(),
        name: m.name().unwrap_or_default().to_string(),
        role: role_name(m.role()),
        parent_id: m.parent_id().map(String::from),
        sort_order: m.sort_order(),
        total_emails: clamp_u32(m.total_emails()),
        unread_emails: clamp_u32(m.unread_emails()),
    })
}

fn clamp_u32(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

fn role_name(role: Role) -> Option<String> {
    let name = match role {
        Role::None => return None,
        Role::Archive => "archive".to_string(),
        Role::Drafts => "drafts".to_string(),
        Role::Important => "important".to_string(),
        Role::Inbox => "inbox".to_string(),
        Role::Junk => "junk".to_string(),
        Role::Sent => "sent".to_string(),
        Role::Trash => "trash".to_string(),
        Role::Other(s) => s.to_ascii_lowercase(),
    };
    (!name.is_empty()).then_some(name)
}

/// How many ids go into one `/get`, so a request never exceeds the server's limit.
pub(super) fn batch_size(max_objects_in_get: usize) -> usize {
    if max_objects_in_get == 0 {
        FALLBACK_BATCH
    } else {
        max_objects_in_get
    }
}

/// Puts `items` in the order of `ids`; the server may answer in any order. Items whose id
/// is not in `ids` go last.
pub(super) fn order_by_ids<T>(ids: &[String], mut items: Vec<T>, id_of: impl Fn(&T) -> &str) -> Vec<T> {
    let position: HashMap<&str, usize> = ids.iter().enumerate().map(|(i, id)| (id.as_str(), i)).collect();
    items.sort_by_key(|t| position.get(id_of(t)).copied().unwrap_or(usize::MAX));
    items
}

/// One page of a `/changes` answer.
#[derive(Debug, Default, PartialEq)]
pub(super) struct Page {
    pub created: Vec<String>,
    pub updated: Vec<String>,
    pub destroyed: Vec<String>,
    pub new_state: String,
    pub has_more: bool,
}

/// Folds the pages of one `/changes` loop into one answer. An id that was created in one page
/// and destroyed in a later one is reported only as destroyed; one created and then updated
/// only as created, so the caller never fetches something that is gone.
pub(super) fn merge_pages(pages: Vec<Page>) -> Changes {
    let mut out = Changes::default();
    for p in pages {
        out.created.extend(p.created);
        out.updated.extend(p.updated);
        out.destroyed.extend(p.destroyed);
        out.new_state = p.new_state;
    }
    let destroyed: HashSet<String> = out.destroyed.iter().cloned().collect();
    out.created.retain(|id| !destroyed.contains(id));
    out.updated.retain(|id| !destroyed.contains(id));
    let created: HashSet<String> = out.created.iter().cloned().collect();
    out.updated.retain(|id| !created.contains(id));
    dedup_keep_order(&mut out.created);
    dedup_keep_order(&mut out.updated);
    dedup_keep_order(&mut out.destroyed);
    out
}

fn dedup_keep_order(v: &mut Vec<String>) {
    let mut seen = HashSet::new();
    v.retain(|id| seen.insert(id.clone()));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn email(json: &str) -> Email<Get> {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn converts_a_header() {
        let e = email(
            r#"{"id":"e1","threadId":"t1","mailboxIds":{"b2":true,"b1":true,"b3":false},
                "keywords":{"$flagged":true},"from":[{"name":"Ann","email":"ann@x.org"}],
                "subject":"Hi","preview":"hello","receivedAt":"2026-01-02T03:04:05Z",
                "hasAttachment":true,"size":1234}"#,
        );
        let h = to_header(&e).unwrap();
        assert_eq!(h.id, "e1");
        assert_eq!(h.thread_id, "t1");
        assert_eq!(h.mailbox_ids, vec!["b1", "b2"]);
        assert_eq!((h.from_name.as_str(), h.from_email.as_str()), ("Ann", "ann@x.org"));
        assert!(h.is_unread && h.is_flagged && h.has_attachment);
        assert_eq!(h.received_at, 1_767_323_045);
        assert_eq!(h.size, 1234);
    }

    #[test]
    fn seen_keyword_means_read_and_missing_name_is_empty() {
        let e = email(r#"{"id":"e1","keywords":{"$seen":true},"from":[{"email":"a@b.c"}]}"#);
        let h = to_header(&e).unwrap();
        assert!(!h.is_unread && !h.is_flagged);
        assert_eq!(h.from_name, "");
        assert_eq!(h.from_email, "a@b.c");
    }

    #[test]
    fn no_sender_and_no_id() {
        let h = to_header(&email(r#"{"id":"e1"}"#)).unwrap();
        assert_eq!((h.from_name.as_str(), h.from_email.as_str()), ("", ""));
        assert!(to_header(&email(r#"{"subject":"x"}"#)).is_none());
    }

    #[test]
    fn preview_is_cut_by_characters_not_bytes() {
        let long: String = "é".repeat(300);
        let e = email(&format!(r#"{{"id":"e1","preview":"{long}"}}"#));
        let h = to_header(&e).unwrap();
        assert_eq!(h.preview.chars().count(), 200);
    }

    #[test]
    fn a_header_carries_the_to_list() {
        let e = email(r#"{"id":"e1","to":[{"name":"Bo","email":"bo@x.org"},{"email":"cy@x.org"}]}"#);
        let h = to_header(&e).unwrap();
        let to: Vec<_> = h.to.iter().map(|a| (a.name.as_str(), a.email.as_str())).collect();
        assert_eq!(to, [("Bo", "bo@x.org"), ("", "cy@x.org")]);
        assert!(to_header(&email(r#"{"id":"e1"}"#)).unwrap().to.is_empty());
    }

    fn part(id: &str, mime: &str) -> String {
        format!(r#"{{"partId":"{id}","blobId":"b{id}","size":10,"type":"{mime}"}}"#)
    }

    #[test]
    fn converts_a_message_with_both_versions_and_files() {
        let e = email(&format!(
            r#"{{"id":"e1","blobId":"whole","to":[{{"name":"Bo","email":"bo@x.org"}}],
                "cc":[{{"email":"cy@x.org"}}],"replyTo":[{{"email":"re@x.org"}}],
                "sentAt":"2026-01-02T03:04:05Z",
                "textBody":[{text}],"htmlBody":[{html},{logo}],
                "attachments":[{{"partId":"3","blobId":"bdoc","size":2048,"type":"application/pdf","name":"a.pdf"}}],
                "bodyValues":{{"1":{{"value":"plain"}},"2":{{"value":"<p>html <img src=\"cid:logo@x\"></p>"}}}}}}"#,
            text = part("1", "text/plain"),
            html = part("2", "text/html"),
            logo = r#"{"partId":"4","blobId":"blogo","size":99,"type":"image/png","name":"logo.png","cid":"<logo@x>","disposition":"inline"}"#,
        ));
        let b = to_body(&e).unwrap();
        assert_eq!((b.message_id.as_str(), b.blob_id.as_str()), ("e1", "whole"));
        assert_eq!(b.to[0].name, "Bo");
        assert_eq!((b.cc[0].email.as_str(), b.reply_to[0].email.as_str()), ("cy@x.org", "re@x.org"));
        assert!(b.bcc.is_empty());
        assert_eq!(b.sent_at, Some(1_767_323_045));
        assert_eq!(b.text.as_deref(), Some("plain"));
        assert_eq!(b.html.as_deref(), Some("<p>html <img src=\"cid:logo@x\"></p>"));
        assert!(!b.is_truncated);
        let files: Vec<_> = b.attachments.iter().map(|a| (a.blob_id.as_str(), a.is_inline)).collect();
        assert_eq!(files, [("bdoc", false), ("blogo", true)]);
        assert_eq!(b.attachments[0].name, "a.pdf");
        assert_eq!((b.attachments[0].mime_type.as_str(), b.attachments[0].size), ("application/pdf", 2048));
        assert_eq!(b.attachments[1].content_id.as_deref(), Some("logo@x"), "no angle brackets");
    }

    #[test]
    fn html_only_mail_has_no_text_version_even_though_the_server_lists_the_html_in_both() {
        let e = email(&format!(
            r#"{{"id":"e1","textBody":[{h}],"htmlBody":[{h}],"bodyValues":{{"2":{{"value":"<b>x</b>"}}}}}}"#,
            h = part("2", "text/html")
        ));
        let b = to_body(&e).unwrap();
        assert_eq!(b.html.as_deref(), Some("<b>x</b>"));
        assert_eq!(b.text, None);
    }

    #[test]
    fn text_only_mail_has_no_html_version() {
        let e = email(&format!(
            r#"{{"id":"e1","textBody":[{t}],"htmlBody":[{t}],"bodyValues":{{"1":{{"value":"hi"}}}}}}"#,
            t = part("1", "text/plain")
        ));
        let b = to_body(&e).unwrap();
        assert_eq!((b.text.as_deref(), b.html.as_deref()), (Some("hi"), None));
    }

    #[test]
    fn several_text_parts_are_joined_in_order() {
        let e = email(&format!(
            r#"{{"id":"e1","textBody":[{a},{b}],"htmlBody":[{c},{d}],
                "bodyValues":{{"1":{{"value":"one"}},"2":{{"value":"two"}},"3":{{"value":"<i>1</i>"}},"4":{{"value":"<i>2</i>"}}}}}}"#,
            a = part("1", "text/plain"),
            b = part("2", "text/plain"),
            c = part("3", "text/html"),
            d = part("4", "text/html"),
        ));
        let b = to_body(&e).unwrap();
        assert_eq!(b.text.as_deref(), Some("one\ntwo"));
        assert_eq!(b.html.as_deref(), Some("<i>1</i><i>2</i>"));
    }

    #[test]
    fn truncation_counts_only_for_the_values_used() {
        let truncated = |text: &str, html: &str| {
            let e = email(&format!(
                r#"{{"id":"e1","textBody":[{t}],"htmlBody":[{h}],
                    "bodyValues":{{"1":{{"value":"a","isTruncated":{text}}},"2":{{"value":"b","isTruncated":{html}}}}}}}"#,
                t = part("1", "text/plain"),
                h = part("2", "text/html"),
            ));
            to_body(&e).unwrap().is_truncated
        };
        assert!(!truncated("false", "false"));
        assert!(truncated("true", "false"));
        assert!(truncated("false", "true"));
        // A cut value that is not shown (the HTML part is not text/html here) does not count.
        let e = email(&format!(
            r#"{{"id":"e1","textBody":[{x}],"bodyValues":{{"1":{{"value":"a","isTruncated":true}}}}}}"#,
            x = part("1", "text/calendar")
        ));
        assert!(!to_body(&e).unwrap().is_truncated);
    }

    #[test]
    fn a_file_is_listed_once_and_inline_needs_the_html_to_refer_to_it() {
        let png = r#"{"partId":"5","blobId":"bimg","size":1,"type":"image/png","cid":"pic"}"#;
        let e = email(&format!(
            r#"{{"id":"e1","textBody":[{t},{png}],"htmlBody":[{h},{png}],"attachments":[{png}],
                "bodyValues":{{"1":{{"value":"x"}},"2":{{"value":"<p>no picture here</p>"}}}}}}"#,
            t = part("1", "text/plain"),
            h = part("2", "text/html"),
        ));
        let b = to_body(&e).unwrap();
        assert_eq!(b.attachments.len(), 1);
        assert_eq!(b.attachments[0].content_id.as_deref(), Some("pic"));
        assert!(!b.attachments[0].is_inline, "the HTML does not mention cid:pic");
        assert_eq!(b.attachments[0].name, "");
    }

    #[test]
    fn inline_means_a_picture_the_html_names_in_full() {
        assert!(refers_to(r#"<img src="cid:pic">"#, "pic"));
        assert!(refers_to("<img src=cid:pic>", "pic"));
        assert!(refers_to("background:url(cid:pic)", "pic"));
        assert!(!refers_to(r#"<img src="cid:pic2">"#, "pic"), "another id that starts the same");
        assert!(!refers_to("<p>no picture</p>", "pic"));
        // A document the HTML links by Content-ID stays a listed file: only pictures are shown inline.
        let pdf = r#"{"partId":"5","blobId":"bdoc","size":1,"type":"application/pdf","cid":"doc"}"#;
        let e = email(&format!(
            r#"{{"id":"e1","htmlBody":[{h}],"attachments":[{pdf}],
                "bodyValues":{{"2":{{"value":"<a href=\"cid:doc\">the file</a>"}}}}}}"#,
            h = part("2", "text/html"),
        ));
        assert!(!to_body(&e).unwrap().attachments[0].is_inline);
    }

    #[test]
    fn text_between_html_parts_is_kept_as_escaped_text() {
        // Text, then HTML, not as alternatives: the server lists both parts in both bodies.
        let e = email(&format!(
            r#"{{"id":"e1","textBody":[{t},{h}],"htmlBody":[{t},{h}],
                "bodyValues":{{"1":{{"value":"a < b & \"c\""}},"2":{{"value":"<p>html</p>"}}}}}}"#,
            t = part("1", "text/plain"),
            h = part("2", "text/html"),
        ));
        let b = to_body(&e).unwrap();
        assert_eq!(
            b.html.as_deref(),
            Some("<pre style=\"white-space:pre-wrap;font:inherit\">a &lt; b &amp; &quot;c&quot;</pre><p>html</p>")
        );
        assert_eq!(b.text.as_deref(), Some("a < b & \"c\""));
    }

    #[test]
    fn a_text_file_attachment_stays_an_attachment_and_a_part_without_a_blob_is_skipped() {
        let e = email(
            r#"{"id":"e1","attachments":[
                {"partId":"7","blobId":"btxt","size":5,"type":"text/plain","name":"notes.txt"},
                {"partId":"8","size":5,"type":"application/zip"}]}"#,
        );
        let b = to_body(&e).unwrap();
        assert_eq!(b.attachments.len(), 1);
        assert_eq!(b.attachments[0].name, "notes.txt");
        assert_eq!((b.text, b.html), (None, None));
        assert!(to_body(&email(r#"{"subject":"x"}"#)).is_none());
    }

    #[test]
    fn converts_a_mailbox() {
        let m: JmapMailbox<Get> = serde_json::from_str(
            r#"{"id":"b1","name":"Inbox","role":"inbox","sortOrder":1,"totalEmails":7,"unreadEmails":2}"#,
        )
        .unwrap();
        let m = to_mailbox(&m).unwrap();
        assert_eq!(m.role.as_deref(), Some("inbox"));
        assert_eq!((m.total_emails, m.unread_emails, m.sort_order), (7, 2, 1));
        assert_eq!(m.parent_id, None);
        let plain: JmapMailbox<Get> = serde_json::from_str(r#"{"id":"b2","name":"Mine","parentId":"b1"}"#).unwrap();
        let plain = to_mailbox(&plain).unwrap();
        assert_eq!(plain.role, None);
        assert_eq!(plain.parent_id.as_deref(), Some("b1"));
    }

    #[test]
    fn unusual_roles_are_lower_case() {
        let m: JmapMailbox<Get> = serde_json::from_str(r#"{"id":"b1","role":"Snoozed"}"#).unwrap();
        assert_eq!(to_mailbox(&m).unwrap().role.as_deref(), Some("snoozed"));
    }

    #[test]
    fn batches() {
        assert_eq!(batch_size(0), 50);
        assert_eq!(batch_size(500), 500);
        let ids: Vec<String> = (0..1050).map(|i| i.to_string()).collect();
        let sizes: Vec<usize> = ids.chunks(batch_size(500)).map(|c| c.len()).collect();
        assert_eq!(sizes, vec![500, 500, 50]);
    }

    #[test]
    fn restores_request_order() {
        let ids: Vec<String> = ["a", "b", "c"].iter().map(|s| s.to_string()).collect();
        let got = order_by_ids(&ids, vec!["c", "x", "a"], |s| s);
        assert_eq!(got, vec!["a", "c", "x"]);
    }

    fn page(created: &[&str], updated: &[&str], destroyed: &[&str], state: &str, more: bool) -> Page {
        let v = |s: &[&str]| s.iter().map(|x| x.to_string()).collect();
        Page {
            created: v(created),
            updated: v(updated),
            destroyed: v(destroyed),
            new_state: state.into(),
            has_more: more,
        }
    }

    #[test]
    fn merges_pages() {
        let merged = merge_pages(vec![
            page(&["a", "b"], &["u"], &[], "s1", true),
            page(&["c"], &["a", "u", "d"], &["b"], "s2", false),
        ]);
        assert_eq!(merged.created, vec!["a", "c"]);
        assert_eq!(merged.updated, vec!["u", "d"]);
        assert_eq!(merged.destroyed, vec!["b"]);
        assert_eq!(merged.new_state, "s2");
    }
}
