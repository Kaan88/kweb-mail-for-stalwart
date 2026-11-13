//! Pure conversions from jmap-client objects to the core's model types, and the small
//! arithmetic around them. Kept apart from the network code so it can be tested directly.

use std::collections::{HashMap, HashSet};

use jmap_client::{
    email::{Email, Property as EmailProperty},
    mailbox::{Mailbox as JmapMailbox, Property as MailboxProperty, Role},
    Get,
};

use super::Changes;
use crate::model::{Mailbox, MessageHeader};

const PREVIEW_CHARS: usize = 200;
/// Used when the server does not state `maxObjectsInGet` (RFC 8620 says at least 500, but
/// a smaller batch costs little).
const FALLBACK_BATCH: usize = 50;

/// Only what the message list and notifications show.
pub(super) fn header_properties() -> Vec<EmailProperty> {
    use EmailProperty::*;
    vec![Id, ThreadId, MailboxIds, Keywords, From, Subject, Preview, ReceivedAt, HasAttachment, Size]
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
        subject: e.subject().unwrap_or_default().to_string(),
        preview: e.preview().unwrap_or_default().chars().take(PREVIEW_CHARS).collect(),
        received_at: e.received_at().unwrap_or(0),
        is_unread: !keywords.contains(&"$seen"),
        is_flagged: keywords.contains(&"$flagged"),
        has_attachment: e.has_attachment(),
        size: e.size() as u64,
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
