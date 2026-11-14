//! Deleted-item recovery. Nothing here was seen on a licensed server with recovery switched on
//! (the only licensed server available had it off, and had no archived item): the shapes are
//! those of the documentation and the server's schema (`dev/notes/stalwart-admin-features.md`,
//! section on recovery), and what is read from them is read loosely.

use serde_json::Value;

use super::{history::parse_time, DeletedItem, DeletedKind, RecoveryState};

const DAY_MS: f64 = 86_400_000.0;

/// `archiveDeletedItemsFor` is a time in milliseconds, or null when the server keeps nothing.
/// A number given as text is read too.
pub fn state_from(setting: &Value) -> RecoveryState {
    let ms = setting.as_f64().or_else(|| setting.as_str().and_then(|s| s.trim().parse().ok()));
    match ms.filter(|ms| *ms > 0.0) {
        Some(ms) => {
            RecoveryState { is_on: true, keeps_days: Some((ms / DAY_MS).ceil().min(f64::from(u32::MAX)) as u32) }
        }
        None => RecoveryState { is_on: false, keeps_days: None },
    }
}

/// An archived item as the list shows it. The kind is the variant (`@type`); the title is the
/// subject, name or title the variant has. `None` for an item without an id.
pub fn item_from(item: &Value) -> Option<DeletedItem> {
    let id = item["id"].as_str()?.to_string();
    let text = |key: &str| item[key].as_str().map(str::trim).filter(|t| !t.is_empty()).map(str::to_string);
    let (kind, title) = match item["@type"].as_str() {
        Some("Email") => (DeletedKind::Mail, text("subject").or_else(|| text("from"))),
        Some("CalendarEvent") => (DeletedKind::Event, text("title")),
        Some("ContactCard") => (DeletedKind::Contact, text("name")),
        Some("FileNode") => (DeletedKind::File, text("name")),
        Some("SieveScript") => (DeletedKind::Other, text("name")),
        _ => (DeletedKind::Other, text("name").or_else(|| text("title")).or_else(|| text("subject"))),
    };
    let deleted_at = item["archivedAt"].as_str().and_then(parse_time);
    Some(DeletedItem { id, kind, title: title.unwrap_or_default(), deleted_at })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn retention_is_days() {
        assert_eq!(state_from(&Value::Null), RecoveryState { is_on: false, keeps_days: None });
        assert_eq!(state_from(&json!(0)), RecoveryState { is_on: false, keeps_days: None });
        assert_eq!(state_from(&json!(2_592_000_000u64)), RecoveryState { is_on: true, keeps_days: Some(30) });
        assert_eq!(state_from(&json!("3600000")), RecoveryState { is_on: true, keeps_days: Some(1) });
    }

    #[test]
    fn items_take_the_title_of_their_kind() {
        let mail = item_from(
            &json!({"id": "a", "@type": "Email", "subject": " Plans ", "archivedAt": "2026-11-14T10:00:00Z"}),
        );
        let mail = mail.unwrap();
        assert_eq!((mail.kind, mail.title.as_str()), (DeletedKind::Mail, "Plans"));
        assert!(mail.deleted_at.is_some());
        let file = item_from(&json!({"id": "b", "@type": "FileNode", "name": "a.txt"})).unwrap();
        assert_eq!((file.kind, file.deleted_at), (DeletedKind::File, None));
        assert!(item_from(&json!({"@type": "Email"})).is_none());
    }
}
