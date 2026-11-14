//! M10 against the mock server: labels (keywords on the server, kept and completed by the app)
//! and the marked search hits, through the `Core` object the way the app uses them. The mock
//! takes keywords as Stalwart 1.1.0 does: ASCII only, lower-cased, at most 128 bytes
//! (`dev/notes/stalwart-mail-features.md`, section 3), and answers `SearchSnippet/get` with
//! escaped text and `<mark>` tags (section 4).

mod support;

use std::sync::Arc;

use kweb_core::{
    ffi::{Core, Shade},
    model::{Label, MarkRange, Marked, MessageLabels},
    secrets::MemorySecrets,
    store::Store,
    Error,
};
use support::MockServer;
use tokio::runtime::Runtime;

/// Seeded: Maria's mail, unread, in the Inbox.
const E1: &str = "e1";
/// Seeded: the Build Bot mail, unread, in the Inbox.
const E2: &str = "e2";
/// Seeded: Tom's invoice, read, in the Inbox.
const E3: &str = "e3";

struct Rig {
    // Field order is drop order: the core goes first, the mock and its runtime last.
    core: Arc<Core>,
    server: MockServer,
    _rt: Runtime,
    db_path: String,
    _dir: tempfile::TempDir,
}

impl Rig {
    /// Signed in and synced once.
    fn new() -> Rig {
        Rig::build(|_| {})
    }

    /// Like `new`, with `prepare` run on the server before the core signs in.
    fn build(prepare: impl FnOnce(&MockServer)) -> Rig {
        let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
        let server = rt.block_on(MockServer::start());
        prepare(&server);
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("cache.db").to_string_lossy().into_owned();
        let core = Core::new(db_path.clone(), Arc::new(MemorySecrets::default())).unwrap();
        let url = core.begin_sign_in_with_redirect(server.url(), "ovh.kweb.mail:/oauth/callback".into()).unwrap();
        core.complete_sign_in(server.authorize(&url).unwrap()).unwrap();
        let rig = Rig { core, server, _rt: rt, db_path, _dir: dir };
        rig.sync();
        rig
    }

    fn sync(&self) {
        self.core.check_once(Shade::Unknown).expect("a pass");
    }

    /// The labels of one message as the core shows them.
    fn labels_of(&self, id: &str) -> Vec<String> {
        let all = self.core.message_labels(vec![id.to_string()]).unwrap();
        assert_eq!(all.len(), 1);
        all[0].label_ids.clone()
    }

    fn refusal<T: std::fmt::Debug>(&self, result: Result<T, Error>) -> String {
        match result {
            Err(Error::Invalid { msg }) => msg,
            other => panic!("expected a refusal, got {other:?}"),
        }
    }
}

fn names(labels: &[Label]) -> Vec<&str> {
    labels.iter().map(|l| l.name.as_str()).collect()
}

fn ids(messages: &[kweb_core::model::MessageHeader]) -> Vec<&str> {
    messages.iter().map(|m| m.id.as_str()).collect()
}

// ---- making and removing labels ------------------------------------------------------------

#[test]
fn a_plain_name_is_its_own_keyword_and_any_other_is_hex_behind_a_prefix() {
    let rig = Rig::new();
    assert!(rig.core.labels().unwrap().is_empty());
    let plain = rig.core.create_label("project-x_2".into()).unwrap();
    assert_eq!((plain.id.as_str(), plain.name.as_str()), ("project-x_2", "project-x_2"));
    let upper = rig.core.create_label("  Work  ".into()).unwrap();
    assert_eq!((upper.id.as_str(), upper.name.as_str()), ("kweb-576f726b", "Work"));
    let turkish = rig.core.create_label("İş ve aile".into()).unwrap();
    assert!(turkish.id.starts_with("kweb-") && turkish.id.is_ascii() && turkish.id.len() <= 128);
    // By name, and kept: a second core on the same cache sees them.
    let all = rig.core.labels().unwrap();
    assert_eq!(names(&all), ["İş ve aile", "project-x_2", "Work"]);
    let again = Core::new(rig.db_path.clone(), Arc::new(MemorySecrets::default())).unwrap();
    assert_eq!(again.labels().unwrap(), all);
}

#[test]
fn a_name_is_one_line_not_blank_and_at_most_sixty_characters() {
    let rig = Rig::new();
    for bad in ["", "   ", "two\nlines", "tab\there", &"x".repeat(61)] {
        rig.refusal(rig.core.create_label(bad.into()));
    }
    assert!(rig.core.create_label("é".repeat(30)).is_ok());
    assert!(rig.core.create_label("x".repeat(60)).is_ok());
    // Too long once encoded: refused, not cut into a keyword another name would share.
    rig.refusal(rig.core.create_label("é".repeat(31)));
    assert_eq!(rig.core.labels().unwrap().len(), 2);
}

#[test]
fn a_name_like_a_system_keyword_is_a_label_of_its_own() {
    let rig = Rig::new();
    let junk = rig.core.create_label("junk".into()).unwrap();
    assert_eq!((junk.id.as_str(), junk.name.as_str()), ("kweb-6a756e6b", "junk"));
    rig.core.set_label(vec![E1.into()], junk.id.clone(), true).unwrap();
    rig.sync();
    assert!(!rig.server.keywords_of(E1).contains(&"junk".to_string()));
}

#[test]
fn a_name_that_is_there_already_is_refused_without_regard_to_case() {
    let rig = Rig::new();
    rig.core.create_label("Work".into()).unwrap();
    for same in ["Work", "work", "WORK", " wOrK "] {
        rig.refusal(rig.core.create_label(same.into()));
    }
    assert_eq!(rig.core.labels().unwrap().len(), 1);
    // A label found on mail is there as well.
    rig.server.add_keyword(E1, "project-x");
    rig.sync();
    rig.refusal(rig.core.create_label("Project-X".into()));
}

#[test]
fn deleting_a_label_leaves_the_keyword_on_the_server_and_the_label_does_not_come_back() {
    let rig = Rig::new();
    let work = rig.core.create_label("Work".into()).unwrap();
    rig.core.set_label(vec![E1.into()], work.id.clone(), true).unwrap();
    rig.sync();
    assert!(rig.server.keywords_of(E1).contains(&work.id));

    rig.core.delete_label(work.id.clone()).unwrap();
    assert!(rig.core.labels().unwrap().is_empty());
    assert!(rig.server.keywords_of(E1).contains(&work.id), "the server keeps it");
    // The mail the app has cached still carries it; it is not offered again, even after a sync.
    rig.server.add_keyword(E2, "kweb-576f726b");
    rig.sync();
    assert!(rig.core.labels().unwrap().is_empty());
    assert!(rig.labels_of(E1).is_empty());
    // Deleting what is not there is as good as done.
    rig.core.delete_label(work.id.clone()).unwrap();
    rig.core.delete_label("never-was".into()).unwrap();
    // Making it again brings it back with the mail that has it.
    rig.core.create_label("Work".into()).unwrap();
    assert_eq!(rig.labels_of(E1), vec![work.id.clone()]);
    assert_eq!(rig.labels_of(E2), [work.id]);
}

// ---- putting labels on messages ------------------------------------------------------------

#[test]
fn a_label_is_on_the_cache_at_once_and_reaches_the_server_with_the_next_pass() {
    let rig = Rig::new();
    let work = rig.core.create_label("Work".into()).unwrap();
    let home = rig.core.create_label("home".into()).unwrap();
    rig.core.set_label(vec![E1.into(), E2.into()], work.id.clone(), true).unwrap();
    rig.core.set_label(vec![E1.into()], home.id.clone(), true).unwrap();
    // Before any pass: the cache has it, the server does not.
    assert_eq!(rig.labels_of(E1), [home.id.clone(), work.id.clone()]);
    assert_eq!(rig.labels_of(E2), vec![work.id.clone()]);
    assert!(rig.labels_of(E3).is_empty());
    assert!(!rig.server.keywords_of(E1).contains(&work.id));

    rig.sync();
    let on_server = rig.server.keywords_of(E1);
    assert!(on_server.contains(&"kweb-576f726b".to_string()) && on_server.contains(&"home".to_string()));
    assert!(rig.server.keywords_of(E2).contains(&work.id));
    assert!(!rig.server.keywords_of(E3).contains(&work.id));
    assert_eq!(rig.labels_of(E1), [home.id.clone(), work.id.clone()]);

    // And off again; the other label stays.
    rig.core.set_label(vec![E1.into()], work.id.clone(), false).unwrap();
    assert_eq!(rig.labels_of(E1), vec![home.id.clone()]);
    rig.sync();
    let on_server = rig.server.keywords_of(E1);
    assert!(!on_server.contains(&work.id) && on_server.contains(&"home".to_string()));
    // Setting it twice is no trouble.
    rig.core.set_label(vec![E1.into(), E1.into()], home.id.clone(), true).unwrap();
    rig.sync();
    assert_eq!(rig.labels_of(E1), [home.id]);
}

#[test]
fn a_label_changes_nothing_but_its_own_keyword() {
    let rig = Rig::new();
    let work = rig.core.create_label("Work".into()).unwrap();
    rig.server.add_keyword(E1, "$flagged");
    rig.server.add_keyword(E1, "$answered");
    rig.sync();
    let before: Vec<String> = rig.server.keywords_of(E1);
    rig.core.set_label(vec![E1.into()], work.id.clone(), true).unwrap();
    rig.sync();
    let mut expected = before;
    expected.push(work.id.clone());
    expected.sort();
    let mut now = rig.server.keywords_of(E1);
    now.sort();
    assert_eq!(now, expected);
    // Unread and starred mean what they meant: the keywords the sync stores do not change them.
    let header = rig.core.message(E1.into()).unwrap().unwrap();
    assert!(header.is_unread && header.is_flagged);
    let read = rig.core.message(E3.into()).unwrap().unwrap();
    assert!(!read.is_unread && !read.is_flagged);
}

#[test]
fn a_label_set_offline_is_kept_queued_and_survives_a_change_made_elsewhere() {
    let rig = Rig::new();
    let work = rig.core.create_label("Work".into()).unwrap();
    rig.server.set_down(true);
    rig.core.set_label(vec![E1.into()], work.id.clone(), true).unwrap();
    assert_eq!(rig.labels_of(E1), vec![work.id.clone()]);
    let _ = rig.core.check_once(Shade::Unknown);
    assert!(!rig.server.keywords_of(E1).contains(&work.id), "not before the server is back");
    rig.server.set_down(false);
    // The mail changes elsewhere meanwhile, so the sync writes the server's copy over the cache.
    rig.server.mark_read(E1);
    rig.sync();
    assert!(rig.server.keywords_of(E1).contains(&work.id), "flushed");
    assert_eq!(rig.labels_of(E1), vec![work.id.clone()]);
    assert!(!rig.core.message(E1.into()).unwrap().unwrap().is_unread);
}

#[test]
fn the_queue_keeps_the_order_of_a_label_put_on_and_taken_off() {
    let rig = Rig::new();
    let work = rig.core.create_label("Work".into()).unwrap();
    rig.core.set_label(vec![E1.into()], work.id.clone(), true).unwrap();
    rig.core.set_label(vec![E1.into()], work.id.clone(), false).unwrap();
    rig.core.set_label(vec![E2.into()], work.id.clone(), false).unwrap();
    rig.core.set_label(vec![E2.into()], work.id.clone(), true).unwrap();
    rig.sync();
    assert!(!rig.server.keywords_of(E1).contains(&work.id));
    assert!(rig.server.keywords_of(E2).contains(&work.id));
    assert_eq!(rig.server.request_count("Email/set"), 4);
}

#[test]
fn a_label_that_is_not_there_is_refused_and_queues_nothing() {
    let rig = Rig::new();
    rig.refusal(rig.core.set_label(vec![E1.into()], "nope".into(), true));
    rig.refusal(rig.core.label_messages("nope".into(), 10));
    // Nothing for no messages.
    let work = rig.core.create_label("Work".into()).unwrap();
    rig.core.set_label(Vec::new(), work.id, true).unwrap();
    rig.sync();
    assert_eq!(rig.server.request_count("Email/set"), 0);
}

#[test]
fn a_message_the_server_no_longer_has_drops_the_label_change() {
    let rig = Rig::new();
    let work = rig.core.create_label("Work".into()).unwrap();
    rig.server.destroy(E2);
    rig.core.set_label(vec![E2.into(), E1.into()], work.id.clone(), true).unwrap();
    rig.sync();
    // The one that is gone does not block the next.
    assert!(rig.server.keywords_of(E1).contains(&work.id));
    rig.sync();
    assert!(rig.core.message(E2.into()).unwrap().is_none());
    assert_eq!(rig.server.request_count("Email/set"), 2);
}

#[test]
fn the_server_lower_cases_and_refuses_what_it_does_not_take() {
    // What the mock does, as the notes say: the app's own keywords are always ones it takes.
    let rig = Rig::new();
    let long = rig.core.create_label("é".repeat(30)).unwrap();
    assert!(long.id.len() <= 128);
    rig.core.set_label(vec![E1.into()], long.id.clone(), true).unwrap();
    rig.sync();
    assert!(rig.server.keywords_of(E1).contains(&long.id), "taken: {}", long.id);
    assert_eq!(rig.server.request_count("Email/set"), 1);
}

// ---- labels found on mail ------------------------------------------------------------------

#[test]
fn keywords_on_mail_the_app_has_seen_complete_the_list() {
    let rig = Rig::new();
    let mine = rig.core.create_label("Work".into()).unwrap();
    // Other clients: a plain keyword, one the app would have made, and system keywords.
    rig.server.add_keyword(E1, "project-x");
    rig.server.add_keyword(E1, "kweb-c4b0c59f");
    rig.server.add_keyword(E2, "$label1");
    rig.server.add_keyword(E2, "$junk");
    rig.server.add_keyword(E2, "junk");
    rig.server.add_keyword(E2, "draft");
    rig.server.add_keyword(E3, "kweb-zz");
    rig.server.add_keyword(E3, "kweb-576f726b");
    rig.sync();
    let all = rig.core.labels().unwrap();
    let pairs: Vec<(&str, &str)> = all.iter().map(|l| (l.id.as_str(), l.name.as_str())).collect();
    assert_eq!(
        pairs,
        [("kweb-c4b0c59f", "İş"), ("kweb-zz", "kweb-zz"), ("project-x", "project-x"), ("kweb-576f726b", "Work")]
    );
    assert_eq!(rig.labels_of(E1), ["kweb-c4b0c59f", "project-x"]);
    assert!(rig.labels_of(E2).is_empty(), "system keywords are no labels");
    assert_eq!(rig.labels_of(E3), ["kweb-zz", "kweb-576f726b"]);
    assert_eq!(mine.id, "kweb-576f726b");
}

#[test]
fn a_cache_from_before_the_keywords_is_filled_by_one_full_fetch() {
    let rig = Rig::new();
    rig.server.add_keyword(E1, "project-x");
    // As if the cache had been made by an older version: no flag, so no keywords can be trusted.
    let account = rig.core.account().unwrap().id;
    let store = Store::open(std::path::Path::new(&rig.db_path)).unwrap();
    store.kv_delete(&account, "keywords_cached").unwrap();
    let before = rig.server.request_count("Email/query");
    rig.server.forget_changes_before_now();
    rig.sync();
    assert!(rig.server.request_count("Email/query") > before, "a full fetch");
    assert_eq!(rig.labels_of(E1), ["project-x"]);
    let after = rig.server.request_count("Email/query");
    rig.sync();
    assert_eq!(rig.server.request_count("Email/query"), after, "once");
}

#[test]
fn a_label_taken_off_elsewhere_is_gone_from_the_message_after_a_pass() {
    let rig = Rig::new();
    let work = rig.core.create_label("Work".into()).unwrap();
    rig.server.add_keyword(E1, &work.id);
    rig.sync();
    assert_eq!(rig.labels_of(E1), vec![work.id.clone()]);
    rig.server.remove_keyword(E1, &work.id);
    rig.sync();
    assert!(rig.labels_of(E1).is_empty());
}

#[test]
fn message_labels_answers_each_message_asked_from_the_cache_alone() {
    let rig = Rig::new();
    let work = rig.core.create_label("Work".into()).unwrap();
    rig.core.set_label(vec![E2.into()], work.id.clone(), true).unwrap();
    rig.server.set_down(true);
    let asked = vec![E1.to_string(), E2.to_string(), "unknown".to_string()];
    assert_eq!(
        rig.core.message_labels(asked).unwrap(),
        [
            MessageLabels { message_id: E1.into(), label_ids: vec![] },
            MessageLabels { message_id: E2.into(), label_ids: vec![work.id] },
            MessageLabels { message_id: "unknown".into(), label_ids: vec![] },
        ]
    );
    assert!(rig.core.message_labels(Vec::new()).unwrap().is_empty());
}

// ---- the mail that carries a label ---------------------------------------------------------

#[test]
fn label_messages_asks_the_server_newest_first_and_caches_what_it_finds() {
    let rig = Rig::new();
    let work = rig.core.create_label("Work".into()).unwrap();
    // The keyword is on the server only (another client): the app has not seen it yet.
    rig.server.add_keyword(E1, &work.id);
    rig.server.add_keyword(E3, &work.id);
    let found = rig.core.label_messages(work.id.clone(), 50).unwrap();
    assert!(!found.from_cache);
    assert_eq!(ids(&found.messages), [E1, E3], "newest first: Maria two hours ago, Tom a day ago");
    assert_eq!(rig.core.label_messages(work.id.clone(), 1).unwrap().messages.len(), 1);
    assert_eq!(rig.labels_of(E3), vec![work.id.clone()], "what was found is cached with its keywords");
    // No mail has it: nothing, and not an error.
    let other = rig.core.create_label("Other".into()).unwrap();
    assert!(rig.core.label_messages(other.id, 10).unwrap().messages.is_empty());
    assert!(rig.server.request_count("Email/query") >= 2);
}

#[test]
fn label_messages_falls_back_to_the_cache_when_the_server_cannot_be_reached() {
    let rig = Rig::new();
    let work = rig.core.create_label("Work".into()).unwrap();
    rig.core.set_label(vec![E3.into(), E1.into()], work.id.clone(), true).unwrap();
    rig.server.set_down(true);
    let found = rig.core.label_messages(work.id, 50).unwrap();
    assert!(found.from_cache);
    assert_eq!(ids(&found.messages), [E1, E3]);
}

#[test]
fn a_label_search_shows_what_is_queued_that_the_server_has_not_seen() {
    let rig = Rig::new();
    let work = rig.core.create_label("Work".into()).unwrap();
    rig.server.add_keyword(E1, &work.id);
    rig.sync();
    // Taken off here and queued: the server still has it when the search asks, and what the
    // search writes to the cache does not bring it back.
    rig.core.set_label(vec![E1.into()], work.id.clone(), false).unwrap();
    let found = rig.core.label_messages(work.id.clone(), 10).unwrap();
    assert!(!found.from_cache && found.messages.is_empty());
    assert!(rig.labels_of(E1).is_empty());
    // Put on here and queued: the server does not have it yet, the list does.
    rig.core.set_label(vec![E3.into()], work.id.clone(), true).unwrap();
    let found = rig.core.label_messages(work.id.clone(), 10).unwrap();
    assert!(!found.from_cache && ids(&found.messages) == [E3]);
    rig.sync();
    assert!(!rig.server.keywords_of(E1).contains(&work.id));
}

// ---- search snippets -----------------------------------------------------------------------

fn marked(text: &str, marks: &[(u32, u32)]) -> Marked {
    Marked {
        text: text.into(),
        marks: marks.iter().map(|(start, end)| MarkRange { start: *start, end: *end }).collect(),
    }
}

#[test]
fn a_snippet_gives_plain_text_and_ranges_for_the_subject_and_the_preview() {
    let rig = Rig::new();
    let id = rig.server.deliver("Dora", "dora@example.test", "Quarterly Invoice", "x");
    rig.server.set_bodies(
        &id,
        Some("The quarterly invoice is attached. Please review the invoice before Friday."),
        None,
    );
    rig.sync();
    let hits = rig.core.search("invoice".into(), None, 50).unwrap();
    let found: Vec<String> = hits.messages.iter().map(|m| m.id.clone()).collect();
    assert!(found.contains(&id) && found.contains(&E3.to_string()));
    let snippets = rig.core.search_snippets("invoice".into(), found.clone()).unwrap();
    let mine = snippets.iter().find(|s| s.message_id == id).unwrap();
    assert_eq!(mine.subject, Some(marked("Quarterly Invoice", &[(10, 17)])));
    assert_eq!(
        mine.preview,
        Some(marked(
            "The quarterly invoice is attached. Please review the invoice before Friday.",
            &[(14, 21), (53, 60)]
        ))
    );
    // Tom's mail has the word in the subject and in its preview text.
    let tom = snippets.iter().find(|s| s.message_id == E3).unwrap();
    assert_eq!(tom.subject, Some(marked("Invoice 2026-114", &[(0, 7)])));
    assert_eq!(tom.preview.as_ref().unwrap().marks, [MarkRange { start: 4, end: 11 }]);
    // In the order asked.
    let mut reversed = found;
    reversed.reverse();
    let again = rig.core.search_snippets("invoice".into(), reversed.clone()).unwrap();
    let order: Vec<&str> = again.iter().map(|s| s.message_id.as_str()).collect();
    let expected: Vec<&str> = reversed.iter().map(String::as_str).filter(|id| order.contains(id)).collect();
    assert_eq!(order, expected);
}

#[test]
fn a_message_without_a_hit_has_no_entry_and_a_hit_in_one_place_has_none_for_the_other() {
    let rig = Rig::new();
    let all = vec![E1.to_string(), E2.to_string(), E3.to_string()];
    let snippets = rig.core.search_snippets("noon".into(), all.clone()).unwrap();
    assert_eq!(snippets.len(), 1);
    assert_eq!(snippets[0].message_id, E1);
    assert_eq!(snippets[0].subject, None, "the hit is not in the subject");
    assert_eq!(
        snippets[0].preview,
        Some(marked("Are you free at noon? The new place on the corner has a table.", &[(16, 20)]))
    );
    // Nothing found anywhere: an empty list.
    assert!(rig.core.search_snippets("zebra".into(), all).unwrap().is_empty());
}

#[test]
fn the_servers_escapes_and_stray_tags_become_text_and_ranges_count_utf16_units() {
    let rig = Rig::new();
    let id = rig.server.deliver("Dora", "dora@example.test", "😀 Fish <b>bold</b> & chips", "x");
    rig.server.set_bodies(&id, Some("<script>alert(1)</script> fish & 'chips' \"fish\" "), None);
    rig.sync();
    let snippets = rig.core.search_snippets("fish".into(), vec![id.clone()]).unwrap();
    assert_eq!(snippets.len(), 1);
    // The emoji is two units, so the word starts at 3.
    assert_eq!(snippets[0].subject, Some(marked("😀 Fish <b>bold</b> & chips", &[(3, 7)])));
    let preview = snippets[0].preview.as_ref().unwrap();
    assert_eq!(preview.text, "<script>alert(1)</script> fish & 'chips' \"fish\"", "unescaped, trailing space cut");
    assert_eq!(preview.marks, [MarkRange { start: 26, end: 30 }, MarkRange { start: 42, end: 46 }]);
}

#[test]
fn snippets_are_asked_in_batches_the_server_takes() {
    let rig = Rig::build(|server| server.set_mail_get_max(10));
    let mut ids = Vec::new();
    for n in 0..30 {
        ids.push(rig.server.deliver("Dora", "dora@example.test", &format!("zebra {n}"), "x"));
    }
    rig.sync();
    let snippets = rig.core.search_snippets("zebra".into(), ids.clone()).unwrap();
    assert_eq!(snippets.len(), 30);
    assert_eq!(rig.server.request_count("SearchSnippet/get"), 3);
    assert_eq!(snippets[29].subject, Some(marked("zebra 29", &[(0, 5)])));
}

#[test]
fn every_failure_is_an_empty_list() {
    let rig = Rig::new();
    let some = vec![E1.to_string()];
    assert!(rig.core.search_snippets("".into(), some.clone()).unwrap().is_empty());
    assert!(rig.core.search_snippets("   ".into(), some.clone()).unwrap().is_empty());
    assert!(rig.core.search_snippets("noon".into(), Vec::new()).unwrap().is_empty());
    let asked = rig.server.request_count("SearchSnippet/get");
    assert_eq!(asked, 0, "a blank query or no messages asks nothing");
    // Ids the server does not have: no entry.
    assert!(rig.core.search_snippets("noon".into(), vec!["nothing".into()]).unwrap().is_empty());
    // The server cannot be reached.
    rig.server.set_down(true);
    assert_eq!(rig.core.search_snippets("noon".into(), some.clone()).unwrap(), Vec::new());
    rig.server.set_down(false);
    // Signed out.
    rig.core.sign_out().unwrap();
    assert!(rig.core.search_snippets("noon".into(), some).unwrap().is_empty());
}
