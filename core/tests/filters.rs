//! M8 against the mock server: mail filters through the `Core` object the way the app uses them.
//!
//! The mock behaves like Stalwart 1.1.0 where it matters (see `support/sieve.rs`): one script is
//! active at a time, the away message is a script too, a script is made from an uploaded blob,
//! and nothing checks that a folder exists.

mod support;

use std::sync::Arc;

use kweb_core::{
    ffi::{Core, Shade},
    model::{FilterCondition, FilterField, FilterMatch, FilterRule, Filters},
    secrets::MemorySecrets,
    Error,
};
use serde_json::json;
use support::MockServer;
use tokio::runtime::Runtime;

const SCRIPT: &str = "kweb-filters";

struct Rig {
    // Field order is drop order: the core goes first, the mock and its runtime last.
    core: Arc<Core>,
    server: MockServer,
    _rt: Runtime,
    _dir: tempfile::TempDir,
}

impl Rig {
    fn new() -> Rig {
        let rig = Rig::unread();
        // The editor reads before it writes.
        let _ = rig.core.filters();
        rig
    }

    /// A signed-in rig whose core has not read the filters (or the session) yet.
    fn unread() -> Rig {
        let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
        let server = rt.block_on(MockServer::start());
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("cache.db").to_string_lossy().into_owned();
        let core = Core::new(db_path, Arc::new(MemorySecrets::default())).unwrap();
        let url = core.begin_sign_in_with_redirect(server.url(), "ovh.kweb.mail:/oauth/callback".into()).unwrap();
        core.complete_sign_in(server.authorize(&url).unwrap()).unwrap();
        Rig { core, server, _rt: rt, _dir: dir }
    }

    /// How many times the server was asked to write a script.
    fn writes(&self) -> usize {
        self.server.request_count("SieveScript/set")
    }

    fn refusal(&self, result: Result<Filters, Error>) -> String {
        match result {
            Err(Error::Invalid { msg }) => msg,
            other => panic!("expected a refusal, got {other:?}"),
        }
    }
}

fn rule(name: &str) -> FilterRule {
    FilterRule {
        id: String::new(),
        name: name.into(),
        enabled: true,
        match_all: true,
        conditions: vec![FilterCondition {
            field: FilterField::From,
            how: FilterMatch::Contains,
            value: "news@example.org".into(),
        }],
        move_to: Some("mb2".into()),
        mark_read: false,
        star: false,
        delete: false,
        stop: false,
        folder_missing: false,
    }
}

// ---- reading -------------------------------------------------------------------------------

#[test]
fn a_server_without_filter_scripts_says_so() {
    let rig = Rig::unread();
    rig.server.set_sieve_enabled(false);
    match rig.core.filters() {
        Err(Error::Invalid { msg }) => assert_eq!(msg, "this server has no mail filters"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_new_account_has_no_rules_and_the_away_script_is_not_ours() {
    let rig = Rig::new();
    let f = rig.core.filters().unwrap();
    assert_eq!(
        f,
        Filters {
            rules: vec![],
            is_active: false,
            is_stale: false,
            is_edited: false,
            text: String::new(),
            away_is_on: false,
            other_active: None,
            other_text: None,
        }
    );
    // Reading changes nothing.
    assert_eq!(rig.writes(), 0);
    assert_eq!(rig.server.sieve_scripts(), vec![("vacation".to_string(), false)]);
}

// ---- saving --------------------------------------------------------------------------------

#[test]
fn rules_are_saved_as_one_script_that_starts_inactive_and_read_back() {
    let rig = Rig::new();
    let mut second = rule("Starred");
    second.move_to = None;
    second.star = true;
    second.mark_read = true;
    let saved = rig.core.save_filters(vec![rule("News"), second], false, false).unwrap();
    assert!(!saved.is_active && !saved.is_stale && !saved.is_edited);
    assert_eq!(saved.rules.len(), 2);
    // The core gave the new rules ids, and they differ.
    assert!(!saved.rules[0].id.is_empty() && saved.rules[0].id != saved.rules[1].id);
    assert_eq!(saved.rules[0].name, "News");
    assert_eq!(saved.rules[0].move_to.as_deref(), Some("mb2"));
    let text = rig.server.sieve_text(SCRIPT).unwrap();
    assert_eq!(saved.text, text);
    assert!(text.contains("require [\"fileinto\", \"imap4flags\"];"));
    assert!(text.contains("fileinto \"Archive\";"), "{text}");
    assert!(text.contains("addflag \"\\\\Flagged\";"), "{text}");
    // Not active, and the away script was not touched.
    assert_eq!(rig.server.sieve_active(), None);
    assert_eq!(rig.server.sieve_scripts(), vec![("vacation".to_string(), false), (SCRIPT.to_string(), false)]);
    // Another read gives the same, with the ids kept.
    assert_eq!(rig.core.filters().unwrap(), saved);
    // A second save changes the same script: still one, and the ids stay.
    let again = rig.core.save_filters(saved.rules.clone(), false, false).unwrap();
    assert_eq!(again.rules, saved.rules);
    assert_eq!(rig.server.sieve_scripts().len(), 2);
}

#[test]
fn saving_with_activate_makes_it_the_script_the_server_runs_and_an_update_keeps_that() {
    let rig = Rig::new();
    let saved = rig.core.save_filters(vec![rule("News")], true, false).unwrap();
    assert!(saved.is_active);
    assert_eq!(rig.server.sieve_active().as_deref(), Some(SCRIPT));
    // Saving again without `activate` does not switch it off.
    let mut changed = saved.rules.clone();
    changed[0].star = true;
    let saved = rig.core.save_filters(changed, false, false).unwrap();
    assert!(saved.is_active && saved.rules[0].star);
    assert!(rig.server.sieve_text(SCRIPT).unwrap().contains("Flagged"));
}

#[test]
fn an_empty_list_writes_an_empty_script_that_keeps_everything() {
    let rig = Rig::new();
    let saved = rig.core.save_filters(vec![], false, false).unwrap();
    assert!(saved.rules.is_empty() && !saved.is_edited && !saved.is_active);
    let text = rig.server.sieve_text(SCRIPT).unwrap();
    assert!(!text.contains("require") && !text.contains("if "));
    // Saving an empty list over rules replaces them: it does not remove the script.
    rig.core.save_filters(vec![rule("News")], false, false).unwrap();
    let emptied = rig.core.save_filters(vec![], false, false).unwrap();
    assert!(emptied.rules.is_empty());
    assert!(rig.server.sieve_text(SCRIPT).is_some());
}

#[test]
fn a_rule_that_is_off_is_kept_and_not_written() {
    let rig = Rig::new();
    let mut off = rule("Later");
    off.enabled = false;
    let saved = rig.core.save_filters(vec![off, rule("Now")], false, false).unwrap();
    assert!(!saved.rules[0].enabled && saved.rules[1].enabled);
    assert_eq!(rig.server.sieve_text(SCRIPT).unwrap().matches("fileinto").count(), 2, "one use and one require");
}

#[test]
fn delete_moves_to_the_trash_and_stops_and_a_folder_is_named_by_its_path() {
    let rig = Rig::new();
    rig.server.add_mailbox("mb9", "Work", None);
    rig.server.add_mailbox("mb10", "Bills", Some("mb9"));
    let mut gone = rule("Junk");
    gone.move_to = None;
    gone.delete = true;
    let mut nested = rule("Bills");
    nested.move_to = Some("mb10".into());
    rig.core.save_filters(vec![gone, nested], false, false).unwrap();
    let text = rig.server.sieve_text(SCRIPT).unwrap();
    assert!(text.contains("    fileinto \"Trash\";\n    stop;\n"), "{text}");
    assert!(text.contains("fileinto \"Work/Bills\";"), "{text}");
}

#[test]
fn a_wrong_rule_is_refused_by_name_and_nothing_is_written() {
    let rig = Rig::new();
    let mut cases: Vec<(FilterRule, &str)> = Vec::new();
    let mut none = rule("No condition");
    none.conditions.clear();
    cases.push((none, "no condition"));
    let mut blank = rule("Blank");
    blank.conditions[0].value = "   ".into();
    cases.push((blank, "nothing to look for"));
    let mut header = rule("Header");
    header.conditions[0].field = FilterField::Header { name: "X Mailer".into() };
    cases.push((header, "not a header name"));
    let mut both = rule("Both");
    both.delete = true;
    cases.push((both, "pick one"));
    let mut nothing = rule("Nothing");
    nothing.move_to = None;
    cases.push((nothing, "does nothing"));
    let mut broken = rule("Line");
    broken.conditions[0].value = "a\nb".into();
    cases.push((broken, "line break"));
    let mut folder = rule("Folder");
    folder.move_to = Some("nope".into());
    cases.push((folder, "no longer exists"));
    for (bad, why) in cases {
        let name = bad.name.clone();
        let msg = rig.refusal(rig.core.save_filters(vec![rule("Fine"), bad], true, false));
        assert!(msg.contains(&format!("\"{name}\"")) && msg.contains(why), "{name}: {msg}");
    }
    let many: Vec<FilterRule> = (0..201).map(|i| rule(&format!("r{i}"))).collect();
    assert!(rig.refusal(rig.core.save_filters(many, false, false)).contains("200"));
    assert_eq!(rig.writes(), 0);
    assert_eq!(rig.server.sieve_text(SCRIPT), None);
    assert_eq!(rig.server.sieve_active(), None);
}

#[test]
fn a_feature_the_server_does_not_offer_refuses_the_rule_in_plain_words() {
    let rig = Rig::new();
    rig.server.set_sieve_extensions(&["imap4flags"]);
    let msg = rig.refusal(rig.core.save_filters(vec![rule("Move")], false, false));
    assert!(msg.contains("\"Move\"") && msg.contains("\"fileinto\"") && msg.contains("does not offer"), "{msg}");
    // A rule that needs only what the server has is fine.
    let mut flags = rule("Flags");
    flags.move_to = None;
    flags.mark_read = true;
    rig.core.save_filters(vec![flags], false, false).unwrap();
    rig.server.set_sieve_extensions(&["fileinto"]);
    let mut flags = rule("Flags");
    flags.move_to = None;
    flags.star = true;
    assert!(rig.refusal(rig.core.save_filters(vec![flags], false, false)).contains("\"imap4flags\""));
    assert_eq!(rig.writes(), 1, "only the one that was fine");
}

#[test]
fn a_script_over_the_size_limit_is_refused_in_plain_words_before_anything_is_written() {
    let rig = Rig::new();
    rig.server.set_sieve_max_size(300);
    let msg = rig.refusal(rig.core.save_filters(vec![rule("News")], false, false));
    assert!(msg.contains("too big") && msg.contains("300"), "{msg}");
    assert_eq!(rig.writes(), 0);
    assert!(rig.server.file_uploads().is_empty(), "not even uploaded");
}

#[test]
fn what_the_server_says_about_the_script_goes_into_the_refusal_and_nothing_is_written() {
    let rig = Rig::new();
    rig.server.refuse_sieve_validation(Some("Expected token \"string\" at line 9, column 3."));
    for rfc in [false, true] {
        rig.server.set_sieve_rfc_error_names(rfc);
        let msg = rig.refusal(rig.core.save_filters(vec![rule("News")], true, false));
        assert!(msg.contains("Expected token \"string\" at line 9, column 3."), "{msg}");
    }
    assert_eq!(rig.writes(), 0);
    assert_eq!(rig.server.sieve_text(SCRIPT), None);
    assert_eq!(rig.server.sieve_active(), None);
}

#[test]
fn a_change_made_elsewhere_since_the_read_is_refused_and_a_new_read_makes_it_work() {
    let rig = Rig::new();
    rig.core.filters().unwrap();
    rig.server.put_sieve_script("someone-elses", "keep;\n", false);
    let msg = rig.refusal(rig.core.save_filters(vec![rule("News")], false, false));
    assert!(msg.contains("changed elsewhere"), "{msg}");
    assert_eq!(rig.server.sieve_text(SCRIPT), None);
    rig.core.filters().unwrap();
    rig.core.save_filters(vec![rule("News")], false, false).unwrap();
    assert!(rig.server.sieve_text(SCRIPT).is_some());
    // The same for switching on or off.
    rig.server.put_sieve_script("someone-elses", "stop;\n", false);
    let msg = rig.refusal(rig.core.set_filters_active(true));
    assert!(msg.contains("changed elsewhere"), "{msg}");
    assert_eq!(rig.server.sieve_active(), None);
}

// ---- a hand edit ---------------------------------------------------------------------------

#[test]
fn a_script_edited_by_hand_is_reported_and_never_lost_without_overwrite() {
    let rig = Rig::new();
    rig.core.save_filters(vec![rule("News")], true, false).unwrap();
    let generated = rig.server.sieve_text(SCRIPT).unwrap();
    let by_hand = generated.replace("fileinto \"Archive\";", "fileinto \"Archive\";\n    discard;");
    rig.server.put_sieve_script(SCRIPT, &by_hand, true);
    let f = rig.core.filters().unwrap();
    assert!(f.is_edited && f.rules.is_empty() && f.is_active);
    assert_eq!(f.text, by_hand);
    let msg = rig.refusal(rig.core.save_filters(vec![rule("Other")], false, false));
    assert!(msg.contains("overwrite"), "{msg}");
    assert_eq!(rig.server.sieve_text(SCRIPT).unwrap(), by_hand);
    // Switching on or off does not touch the text, either.
    rig.core.set_filters_active(false).unwrap();
    assert_eq!(rig.server.sieve_text(SCRIPT).unwrap(), by_hand);
    // With overwrite the rules replace it.
    let f = rig.core.save_filters(vec![rule("Other")], false, true).unwrap();
    assert!(!f.is_edited && f.rules.len() == 1 && f.rules[0].name == "Other");
    assert!(!rig.server.sieve_text(SCRIPT).unwrap().contains("discard"));
}

#[test]
fn a_script_of_that_name_that_holds_no_rules_is_an_edit_too() {
    let rig = Rig::new();
    for text in ["", "keep;\n", "# kweb-rules not-base64!\n"] {
        rig.server.put_sieve_script(SCRIPT, text, false);
        let f = rig.core.filters().unwrap();
        assert!(f.is_edited && f.rules.is_empty(), "{text:?}");
        assert_eq!(f.text, text);
        assert!(rig.refusal(rig.core.save_filters(vec![rule("News")], false, false)).contains("overwrite"));
    }
    assert_eq!(rig.writes(), 0);
}

// ---- other scripts and the away message --------------------------------------------------

#[test]
fn another_active_script_is_reported_and_its_text_is_never_changed() {
    let rig = Rig::new();
    let theirs = "require [\"fileinto\"];\nfileinto \"Archive\";\n";
    rig.server.put_sieve_script("my-own-script", theirs, true);
    let f = rig.core.filters().unwrap();
    assert_eq!(f.other_active.as_deref(), Some("my-own-script"));
    assert_eq!(f.other_text.as_deref(), Some(theirs));
    assert!(!f.is_active);
    // Saving without activating leaves theirs the one that runs.
    let f = rig.core.save_filters(vec![rule("News")], false, false).unwrap();
    assert_eq!(f.other_active.as_deref(), Some("my-own-script"));
    assert_eq!(rig.server.sieve_active().as_deref(), Some("my-own-script"));
    // Switching ours on displaces it, and its text stays.
    let f = rig.core.set_filters_active(true).unwrap();
    assert!(f.is_active && f.other_active.is_none() && f.other_text.is_none());
    assert_eq!(rig.server.sieve_text("my-own-script").unwrap(), theirs);
    assert_eq!(rig.server.sieve_active().as_deref(), Some(SCRIPT));
}

#[test]
fn switching_off_leaves_another_script_alone() {
    let rig = Rig::new();
    rig.core.save_filters(vec![rule("News")], false, false).unwrap();
    rig.server.put_sieve_script("theirs", "keep;\n", true);
    rig.core.filters().unwrap();
    let f = rig.core.set_filters_active(false).unwrap();
    assert_eq!(f.other_active.as_deref(), Some("theirs"));
    assert_eq!(rig.server.sieve_active().as_deref(), Some("theirs"));
    // Ours on, then off: nothing runs.
    rig.core.set_filters_active(true).unwrap();
    let f = rig.core.set_filters_active(false).unwrap();
    assert!(!f.is_active && f.other_active.is_none());
    assert_eq!(rig.server.sieve_active(), None);
}

#[test]
fn switching_on_or_off_with_no_script_of_ours_is_refused() {
    let rig = Rig::new();
    for on in [true, false] {
        assert!(rig.refusal(rig.core.set_filters_active(on)).contains("no filters saved"));
    }
    assert_eq!(rig.writes(), 0);
}

#[test]
fn the_away_message_is_reported_and_switching_filters_on_switches_it_off_as_the_server_does() {
    let rig = Rig::new();
    rig.core.save_filters(vec![rule("News")], false, false).unwrap();
    rig.server.set_vacation(json!({ "isEnabled": true, "textBody": "I am away." }));
    let f = rig.core.filters().unwrap();
    assert!(f.away_is_on && !f.is_active && f.other_active.is_none(), "the away script is not another script");
    assert_eq!(rig.server.sieve_active().as_deref(), Some("vacation"));
    let f = rig.core.set_filters_active(true).unwrap();
    assert!(f.is_active && !f.away_is_on);
    assert!(!rig.core.away_message().unwrap().is_enabled);
    // The away script itself is untouched: still there, text and all.
    assert!(rig.server.sieve_scripts().contains(&("vacation".to_string(), false)));
    // And the other way round: switching the away message on switches the filters off.
    let mut away = rig.core.away_message().unwrap();
    away.is_enabled = true;
    away.text = "Back soon.".into();
    rig.core.set_away_message(away).unwrap();
    let f = rig.core.filters().unwrap();
    assert!(f.away_is_on && !f.is_active);
    assert_eq!(f.rules.len(), 1, "the rules stay");
}

#[test]
fn saving_with_activate_while_the_away_message_is_on_switches_it_off() {
    let rig = Rig::new();
    rig.server.set_vacation(json!({ "isEnabled": true, "textBody": "I am away." }));
    rig.core.filters().unwrap();
    let f = rig.core.save_filters(vec![rule("News")], true, false).unwrap();
    assert!(f.is_active && !f.away_is_on);
    assert!(!rig.core.away_message().unwrap().is_enabled);
    // Saving without activate while away is on leaves it on.
    rig.core
        .set_away_message({
            let mut a = rig.core.away_message().unwrap();
            a.is_enabled = true;
            a
        })
        .unwrap();
    // The away message moved the state of the scripts, so the filters are read again first.
    rig.core.filters().unwrap();
    let f = rig.core.save_filters(vec![rule("News")], false, false).unwrap();
    assert!(f.away_is_on && !f.is_active);
}

#[test]
fn a_server_with_no_away_message_still_has_filters() {
    let rig = Rig::unread();
    rig.server.set_vacation_enabled(false);
    rig.core.filters().unwrap();
    let f = rig.core.save_filters(vec![rule("News")], true, false).unwrap();
    assert!(f.is_active && !f.away_is_on);
}

// ---- folders -------------------------------------------------------------------------------

#[test]
fn a_renamed_folder_makes_the_script_stale_and_a_save_fixes_it() {
    let rig = Rig::new();
    rig.server.add_mailbox("mb9", "Work", None);
    let mut r = rule("Work mail");
    r.move_to = Some("mb9".into());
    rig.core.save_filters(vec![r], true, false).unwrap();
    assert!(rig.server.sieve_text(SCRIPT).unwrap().contains("fileinto \"Work\""));
    assert!(!rig.core.filters().unwrap().is_stale);
    rig.server.rename_mailbox("mb9", "Projects");
    let f = rig.core.filters().unwrap();
    assert!(f.is_stale && !f.is_edited && !f.rules[0].folder_missing);
    assert_eq!(f.rules.len(), 1, "the rule is still read");
    // The text on the server still names the old path until a save.
    assert!(rig.server.sieve_text(SCRIPT).unwrap().contains("fileinto \"Work\""));
    let f = rig.core.save_filters(f.rules, false, false).unwrap();
    assert!(!f.is_stale && f.is_active, "{f:?}");
    assert!(rig.server.sieve_text(SCRIPT).unwrap().contains("fileinto \"Projects\""));
}

#[test]
fn a_renamed_parent_changes_the_path_of_the_folders_below_it() {
    let rig = Rig::new();
    rig.server.add_mailbox("mb9", "Work", None);
    rig.server.add_mailbox("mb10", "Bills", Some("mb9"));
    let mut r = rule("Bills");
    r.move_to = Some("mb10".into());
    rig.core.save_filters(vec![r], false, false).unwrap();
    rig.server.rename_mailbox("mb9", "Job");
    assert!(rig.core.filters().unwrap().is_stale);
}

#[test]
fn a_deleted_folder_marks_the_rule_and_refuses_the_save() {
    let rig = Rig::new();
    rig.server.add_mailbox("mb9", "Work", None);
    let mut r = rule("Work mail");
    r.move_to = Some("mb9".into());
    rig.core.save_filters(vec![r, rule("News")], true, false).unwrap();
    rig.server.remove_mailbox("mb9");
    let f = rig.core.filters().unwrap();
    assert!(f.rules[0].folder_missing && !f.rules[1].folder_missing);
    let before = rig.server.sieve_text(SCRIPT).unwrap();
    let msg = rig.refusal(rig.core.save_filters(f.rules.clone(), false, false));
    assert!(msg.contains("\"Work mail\"") && msg.contains("no longer exists"), "{msg}");
    assert_eq!(rig.server.sieve_text(SCRIPT).unwrap(), before);
    // Pointing the rule at another folder lets the save through.
    let mut fixed = f.rules.clone();
    fixed[0].move_to = Some("mb2".into());
    let saved = rig.core.save_filters(fixed, false, false).unwrap();
    assert!(!saved.rules[0].folder_missing);
    // A rule that is off does not write its folder, so it does not block the others.
    let mut off = saved.rules.clone();
    off[0].move_to = Some("mb9".into());
    off[0].enabled = false;
    let saved = rig.core.save_filters(off, false, false).unwrap();
    assert!(saved.rules[0].folder_missing && !saved.is_stale);
}

#[test]
fn a_delete_rule_needs_a_trash_folder() {
    let rig = Rig::new();
    rig.server.remove_mailbox("mb3");
    let mut r = rule("Junk");
    r.move_to = None;
    r.delete = true;
    let msg = rig.refusal(rig.core.save_filters(vec![r], false, false));
    assert!(msg.contains("no Trash folder"), "{msg}");
    assert_eq!(rig.writes(), 0);
}

// ---- starting a rule from a message ------------------------------------------------------

#[test]
fn a_rule_is_started_from_a_message_by_its_sender_or_its_list() {
    let rig = Rig::new();
    rig.core.check_once(Shade::Unknown).unwrap();
    let id = rig
        .core
        .messages("mb1".into(), 10, 0)
        .unwrap()
        .into_iter()
        .find(|m| m.from_email.contains("maria"))
        .unwrap()
        .id;
    let r = rig.core.filter_from_message(id.clone()).unwrap();
    assert_eq!(r.name, "Maria Keller");
    assert_eq!(
        r.conditions,
        vec![FilterCondition {
            field: FilterField::From,
            how: FilterMatch::Contains,
            value: "maria@example.test".into()
        }]
    );
    assert!(r.id.is_empty() && r.enabled && r.match_all && r.move_to.is_none());
    assert!(!r.mark_read && !r.star && !r.delete && !r.stop);
    // Nothing was stored.
    assert_eq!(rig.writes(), 0);
    // With a List-Id the rule is about the list.
    rig.server.set_list_id(&id, "Lunch club <lunch.lists.example.test>");
    let r = rig.core.filter_from_message(id).unwrap();
    assert_eq!(r.name, "lunch.lists.example.test");
    assert_eq!(
        r.conditions[0],
        FilterCondition {
            field: FilterField::Header { name: "List-Id".into() },
            how: FilterMatch::Contains,
            value: "lunch.lists.example.test".into()
        }
    );
    // A message that is not in the cache is refused.
    assert!(matches!(rig.core.filter_from_message("nope".into()), Err(Error::Invalid { .. })));
}

#[test]
fn a_rule_from_a_message_can_be_saved_once_it_has_an_action() {
    let rig = Rig::new();
    rig.core.check_once(Shade::Unknown).unwrap();
    let id = rig.core.messages("mb1".into(), 10, 0).unwrap().remove(0).id;
    let mut r = rig.core.filter_from_message(id).unwrap();
    assert!(rig.refusal(rig.core.save_filters(vec![r.clone()], false, false)).contains("does nothing"));
    r.move_to = Some("mb2".into());
    let saved = rig.core.save_filters(vec![r], false, false).unwrap();
    assert_eq!(saved.rules.len(), 1);
}

// ---- signed out ----------------------------------------------------------------------------

#[test]
fn the_functions_need_a_sign_in() {
    let rig = Rig::new();
    rig.core.sign_out().unwrap();
    assert!(matches!(rig.core.filters(), Err(Error::SignInNeeded { .. })));
    assert!(matches!(rig.core.set_filters_active(true), Err(Error::SignInNeeded { .. })));
    assert!(matches!(rig.core.save_filters(vec![], false, false), Err(Error::SignInNeeded { .. })));
}

// ---- the defects of the second check -----------------------------------------------------

#[test]
fn a_save_or_switch_on_without_an_earlier_read_is_refused() {
    let rig = Rig::new();
    let fresh = {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("cache.db").to_string_lossy().into_owned();
        let core = Core::new(db_path, Arc::new(MemorySecrets::default())).unwrap();
        let url = core.begin_sign_in_with_redirect(rig.server.url(), "ovh.kweb.mail:/oauth/callback".into()).unwrap();
        core.complete_sign_in(rig.server.authorize(&url).unwrap()).unwrap();
        (core, dir)
    };
    let (core, _dir) = fresh;
    assert!(rig.refusal(core.save_filters(vec![rule("A")], false, false)).contains("Read the filters first"));
    assert!(rig.refusal(core.set_filters_active(true)).contains("Read the filters first"));
    assert_eq!(rig.writes(), 0);
    core.filters().unwrap();
    core.save_filters(vec![rule("A")], false, false).unwrap();
}

#[test]
fn a_write_that_cannot_be_read_back_says_so_and_the_retry_after_a_read_works() {
    let rig = Rig::new();
    rig.server.break_sieve_reads_after_write(true);
    match rig.core.save_filters(vec![rule("A")], true, false) {
        Err(Error::Temporary { msg }) => assert!(msg.contains("saved"), "{msg}"),
        other => panic!("expected the saved-not-read error, got {other:?}"),
    }
    assert_eq!(rig.writes(), 1);
    assert!(rig.server.sieve_text(SCRIPT).is_some());
    // The old base is gone: a write now asks for a read first, not a false "changed elsewhere".
    assert!(rig.refusal(rig.core.save_filters(vec![rule("A")], true, false)).contains("Read the filters first"));
    rig.server.break_sieve_reads_after_write(false);
    let f = rig.core.filters().unwrap();
    assert_eq!(f.rules.len(), 1);
    rig.core.save_filters(f.rules, true, false).unwrap();
}

#[test]
fn an_unnameable_target_makes_an_enabled_rule_stale() {
    let rig = Rig::new();
    rig.server.add_mailbox("mb9", "Work", None);
    let mut r = rule("Work mail");
    r.move_to = Some("mb9".into());
    rig.core.save_filters(vec![r], true, false).unwrap();
    assert!(!rig.core.filters().unwrap().is_stale);
    rig.server.rename_mailbox("mb9", "Work/2026");
    let f = rig.core.filters().unwrap();
    assert!(f.is_stale && !f.rules[0].folder_missing);
}
