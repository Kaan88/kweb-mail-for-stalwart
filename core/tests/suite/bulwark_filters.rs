//! M12 stage 2a against the mock server: the filters in Bulwark's model, through the `Core` object
//! the way the app uses them (`filters_state`, `filters_save`, ...).

use std::sync::Arc;

use crate::support::MockServer;
use kweb_core::{
    ffi::{Core, FiltersState, RuleMove},
    model::{FilterCondition, FilterField, FilterMatch, FilterRule},
    secrets::MemorySecrets,
    Error,
};
use serde_json::{json, Value};
use tokio::runtime::Runtime;

const EXTENSIONS: [&str; 13] = [
    "body",
    "copy",
    "date",
    "fileinto",
    "imap4flags",
    "include",
    "mailbox",
    "mailboxid",
    "regex",
    "relational",
    "spamtestplus",
    "comparator-i;ascii-numeric",
    "reject",
];

struct Rig {
    core: Arc<Core>,
    server: MockServer,
    _rt: Runtime,
    _dir: tempfile::TempDir,
}

impl Rig {
    fn new() -> Rig {
        let rig = Rig::bare();
        // The editor reads before it writes.
        let _ = rig.core.filters_state();
        rig
    }

    /// A signed-in rig whose core has not read anything yet.
    fn bare() -> Rig {
        let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
        let server = rt.block_on(MockServer::start());
        server.set_sieve_extensions(&EXTENSIONS);
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("cache.db").to_string_lossy().into_owned();
        let core = Core::new(db_path, Arc::new(MemorySecrets::default())).unwrap();
        let url = core.begin_sign_in_with_redirect(server.url(), "ovh.kweb.mail:/oauth/callback".into()).unwrap();
        core.complete_sign_in(server.authorize(&url).unwrap()).unwrap();
        Rig { core, server, _rt: rt, _dir: dir }
    }

    fn state(&self) -> FiltersState {
        self.core.filters_state().unwrap()
    }

    fn refusal(&self, result: Result<FiltersState, Error>) -> String {
        match result {
            Err(Error::Invalid { msg }) => msg,
            other => panic!("expected a refusal, got {other:?}"),
        }
    }
}

fn rules(state: &FiltersState) -> Vec<Value> {
    serde_json::from_str::<Value>(&state.rules_json).unwrap().as_array().unwrap().clone()
}

fn rule(id: &str, name: &str, extra: &str) -> String {
    format!(
        "{{\"id\":\"{id}\",\"name\":\"{name}\",\"enabled\":true,\"matchType\":\"all\",\"conditions\":[{{\"field\":\"from\",\"comparator\":\"contains\",\"value\":\"news@example.org\"}}],\"actions\":[{{\"type\":\"move\",\"value\":\"Archive\",\"mailboxId\":\"mb2\"}}],\"stopProcessing\":false{extra}}}"
    )
}

fn list(items: &[String]) -> String {
    format!("[{}]", items.join(","))
}

const FIXTURE: &str = include_str!("../fixtures/bulwark/mixed-origins.sieve");

#[test]
fn a_new_account_has_no_script_and_the_server_says_what_it_can() {
    let rig = Rig::bare();
    let s = rig.state();
    assert!(s.supported && s.script_name.is_empty() && !s.active && !s.opaque && !s.foreign);
    assert_eq!(s.rules_json, "[]");
    assert!(s.capabilities.regex && s.capabilities.period && s.capabilities.mailboxid && s.capabilities.spamguard);
    assert_eq!(s.capabilities.redirect_limit, Some(1));
    assert!(!s.vacation_included && !s.old_script_present && !s.migration_pending);
    let none = Rig::bare();
    none.server.set_sieve_enabled(false);
    assert!(!none.core.filters_state().unwrap().supported);
}

#[test]
fn a_script_made_by_bulwark_is_read_and_saved_back_byte_for_byte() {
    let rig = Rig::new();
    rig.server.set_sieve_extensions(&["body", "copy", "fileinto", "imap4flags", "include", "relational"]);
    // Bulwark writes the external section straight under its header; the hand-made fixture has a
    // blank line there.
    let text = FIXTURE.replace("\r\n", "\n").replace("Bulwark) ---\n\n", "Bulwark) ---\n");
    rig.server.put_sieve_script("bulwark-made", &text, true);
    let s = rig.state();
    assert_eq!((s.script_name.as_str(), s.active, s.opaque, s.foreign), ("bulwark-made", true, false, false));
    assert_eq!(rules(&s).len(), 2);
    assert_eq!(rules(&s)[0]["name"], "Archive newsletters");
    assert_eq!(s.external_blocks.len(), 1);
    assert_eq!(s.external_blocks[0].origin_label, "Roundcube");
    assert!(
        s.external_blocks[0].raw.contains("Nextcloud Mail - begin") && s.external_blocks[0].raw.contains("LowPriority")
    );
    // Saving the rules as they are changes nothing, the external section included.
    let saved = rig.core.filters_save(s.rules_json.clone()).unwrap();
    assert_eq!(rig.server.sieve_text("bulwark-made").unwrap(), text);
    assert_eq!(saved.external_blocks, s.external_blocks);
    assert!(saved.active && rig.server.sieve_active().as_deref() == Some("bulwark-made"));
}

#[test]
fn editing_one_rule_leaves_a_rule_with_regex_period_and_unknown_keys_intact() {
    let rig = Rig::new();
    let special = "{\"id\":\"sp\",\"name\":\"Special\",\"enabled\":true,\"matchType\":\"any\",\"conditions\":[{\"field\":\"subject\",\"comparator\":\"regex\",\"value\":\"^inv\\\\d+\"},{\"field\":\"size\",\"comparator\":\"greater_than\",\"value\":\"2M\"}],\"actions\":[{\"type\":\"forward\",\"value\":\"a@example.org\",\"keepCopy\":true}],\"stopProcessing\":false,\"activeFrom\":\"2026-10-05T08:00:00+02:00\",\"activeUntil\":\"2026-10-16T16:00:00.000Z\",\"future\":{\"a\":[1,2]}}";
    let first = rule("r1", "Plain", "");
    let saved = rig.core.filters_save(list(&[first.clone(), special.to_string()])).unwrap();
    assert!(saved.active, "a first script is switched on");
    assert_eq!(rig.server.sieve_active().as_deref(), Some("filters"));
    let text = rig.server.sieve_text("filters").unwrap();
    assert!(text.starts_with("/* @metadata:begin\n{\"version\":2,"), "{text}");
    assert!(text.contains("header :regex \"Subject\" \"^inv\\\\d+\""), "{text}");
    assert!(text.contains("currentdate :zone \"+0000\" :value \"ge\" \"time\" \"06:00:00\""), "{text}");
    assert!(text.contains("redirect :copy \"a@example.org\";"));
    let before = rules(&saved)[1].clone();
    assert_eq!(before["future"], json!({"a": [1, 2]}));
    let after = rig.core.filters_rule_toggle("r1".into(), false).unwrap();
    assert_eq!(rules(&after)[1], before);
    assert_eq!(rules(&after)[0]["enabled"], false);
    assert!(!rig.server.sieve_text("filters").unwrap().contains("# Rule: Plain"));
    // The host sends a rule without the key it does not know: the core puts it back.
    let mut sent = rules(&after);
    sent[1].as_object_mut().unwrap().remove("future");
    let kept = rig.core.filters_save(Value::Array(sent).to_string()).unwrap();
    assert_eq!(rules(&kept)[1]["future"], json!({"a": [1, 2]}));
}

#[test]
fn rules_move_and_are_deleted_and_the_switch_stops_and_starts_the_script() {
    let rig = Rig::new();
    rig.core.filters_save(list(&[rule("a", "A", ""), rule("b", "B", ""), rule("c", "C", "")])).unwrap();
    let s = rig.core.filters_rule_move("c".into(), RuleMove::Up).unwrap();
    let ids: Vec<String> = rules(&s).iter().map(|r| r["id"].as_str().unwrap().to_string()).collect();
    assert_eq!(ids, ["a", "c", "b"]);
    let s = rig.core.filters_rule_delete("a".into()).unwrap();
    assert_eq!(rules(&s).len(), 2);
    let off = rig.core.filters_set_enabled(false).unwrap();
    assert!(!off.active);
    assert_eq!(rig.server.sieve_active(), None);
    let writes = rig.server.request_count("SieveScript/set");
    let on = rig.core.filters_set_enabled(true).unwrap();
    assert!(on.active && rig.server.sieve_active().as_deref() == Some("filters"));
    assert_eq!(rig.server.request_count("SieveScript/set"), writes + 1, "only the activation");
    let gone = rig.core.filters_rule_delete("zz".into());
    assert!(rig.refusal(gone).contains("not there"));
}

#[test]
fn a_foreign_or_opaque_script_is_shown_and_never_rewritten() {
    let rig = Rig::new();
    rig.server.put_sieve_script("theirs", "require \"fileinto\";\nfileinto \"A\";\n", true);
    let s = rig.state();
    assert!(s.foreign && !s.opaque && s.raw_text.contains("fileinto \"A\""));
    let writes = rig.server.request_count("SieveScript/set");
    let refused = rig.core.filters_save(list(&[rule("a", "A", "")]));
    assert!(rig.refusal(refused).contains("cannot be read"));
    assert_eq!(rig.server.request_count("SieveScript/set"), writes);
    assert_eq!(rig.server.sieve_text("theirs").unwrap(), "require \"fileinto\";\nfileinto \"A\";\n");
    // A broken metadata comment is opaque.
    rig.server.put_sieve_script("theirs", "/* @metadata:begin\n{oops\n@metadata:end */\nkeep;\n", true);
    let s = rig.state();
    assert!(s.opaque && !s.foreign);
    // The raw editor writes text as it is; the server's refusal is shown.
    rig.server.refuse_sieve_validation(Some("Expected token at line 1"));
    let refused = rig.core.filters_raw_save("nonsense".into());
    assert!(rig.refusal(refused).contains("Expected token"));
    rig.server.refuse_sieve_validation(None);
    let s = rig.core.filters_raw_save("keep;\n".into()).unwrap();
    assert!(s.foreign && rig.server.sieve_text("theirs").unwrap() == "keep;\n");
    // Starting over replaces it with an empty set of rules.
    let s = rig.core.filters_reset_to_visual().unwrap();
    assert!(!s.foreign && !s.opaque && s.rules_json == "[]" && s.active);
    let nothing = rig.core.filters_reset_to_visual();
    assert!(rig.refusal(nothing).contains("nothing to start over"));
}

#[test]
fn a_change_made_elsewhere_is_never_written_over() {
    let rig = Rig::new();
    rig.core.filters_save(list(&[rule("a", "A", "")])).unwrap();
    let s = rig.state();
    rig.server.put_sieve_script("filters", "keep;\n", true);
    let refused = rig.core.filters_save(s.rules_json.clone());
    assert!(rig.refusal(refused).contains("changed elsewhere"));
    assert_eq!(rig.server.sieve_text("filters").unwrap(), "keep;\n");
}

#[test]
fn a_write_needs_a_read_in_this_run() {
    let rig = Rig::new();
    let fresh = {
        let dir = tempfile::tempdir().unwrap();
        let core =
            Core::new(dir.path().join("c.db").to_string_lossy().into_owned(), Arc::new(MemorySecrets::default()))
                .unwrap();
        let url = core.begin_sign_in_with_redirect(rig.server.url(), "ovh.kweb.mail:/oauth/callback".into()).unwrap();
        core.complete_sign_in(rig.server.authorize(&url).unwrap()).unwrap();
        (core, dir)
    };
    let refused = fresh.0.filters_save("[]".into());
    assert!(rig.refusal(refused).contains("Read the filters first"));
}

#[test]
fn the_old_script_is_offered_and_removed_after_the_first_save() {
    let rig = Rig::new();
    let _ = rig.core.filters().unwrap();
    let old = FilterRule {
        id: "old1".into(),
        name: "News".into(),
        enabled: true,
        match_all: true,
        conditions: vec![FilterCondition {
            field: FilterField::From,
            how: FilterMatch::Contains,
            value: "news@example.org".into(),
        }],
        move_to: Some("mb2".into()),
        mark_read: true,
        star: false,
        delete: false,
        stop: false,
        folder_missing: false,
    };
    rig.core.save_filters(vec![old], true, false).unwrap();
    assert_eq!(rig.server.sieve_active().as_deref(), Some("kweb-filters"));
    let s = rig.state();
    assert!(s.old_script_present && s.migration_pending && s.old_active && s.script_name.is_empty());
    let converted = rules(&s);
    assert_eq!(converted.len(), 1);
    assert_eq!(converted[0]["id"], "old1");
    assert_eq!(converted[0]["actions"][1], json!({"type": "move", "value": "Archive", "mailboxId": "mb2"}));
    assert_eq!(converted[0]["includeSpam"], true);
    // Not removed before the new script is there.
    let early = rig.core.filters_migrate_old();
    assert!(rig.refusal(early).contains("Save the filters first"));
    let saved = rig.core.filters_save(s.rules_json.clone()).unwrap();
    assert!(!saved.migration_pending && saved.old_script_present && saved.script_name == "filters");
    assert_eq!(rig.server.sieve_active().as_deref(), Some("filters"));
    let done = rig.core.filters_migrate_old().unwrap();
    assert!(!done.old_script_present);
    assert!(rig.server.sieve_scripts().iter().all(|(name, _)| name != "kweb-filters"));
    assert_eq!(rules(&done).len(), 1);
}

#[test]
fn the_away_message_runs_beside_the_filters_through_the_include() {
    let rig = Rig::new();
    rig.server.set_vacation(json!({"isEnabled": true, "subject": "Away", "textBody": "Back soon"}));
    assert_eq!(rig.server.sieve_active().as_deref(), Some("vacation"));
    let s = rig.state();
    assert!(s.vacation_included && !s.vacation_conflict, "the away message could be included");
    let saved = rig.core.filters_save(list(&[rule("a", "A", "")])).unwrap();
    assert!(saved.vacation_included && saved.active);
    let text = rig.server.sieve_text("filters").unwrap();
    assert!(text.contains("# Vacation auto-reply\ninclude :personal :optional \"vacation\";\n"), "{text}");
    assert!(text.contains("\"includeVacation\":true"));
    // Another edit keeps the include, even though the away message is now reported off.
    let again = rig.core.filters_rule_toggle("a".into(), false).unwrap();
    assert!(again.vacation_included);
    assert!(rig.server.sieve_text("filters").unwrap().contains("include :personal :optional \"vacation\";"));
    // Switched on elsewhere the away message displaces the filters; switching them on includes it.
    rig.server.set_vacation(json!({"isEnabled": true}));
    let s = rig.state();
    assert!(s.displaced && !s.active);
    let on = rig.core.filters_set_enabled(true).unwrap();
    assert!(on.active && !on.displaced);
}

#[test]
fn forwards_are_counted_against_the_servers_limit() {
    let rig = Rig::new();
    let fwd = |id: &str, stop: bool| {
        format!(
            "{{\"id\":\"{id}\",\"name\":\"{id}\",\"enabled\":true,\"matchType\":\"all\",\"conditions\":[{{\"field\":\"all\",\"comparator\":\"any\",\"value\":\"\"}}],\"actions\":[{{\"type\":\"forward\",\"value\":\"x@example.org\"}}],\"stopProcessing\":{stop}}}"
        )
    };
    let s = rig.core.filters_save(list(&[fwd("a", false), fwd("b", false)])).unwrap();
    assert_eq!(s.capabilities.max_forwards_now, 2);
    assert_eq!(s.capabilities.redirect_limit, Some(1));
    let s = rig.core.filters_save(list(&[fwd("a", true), fwd("b", false)])).unwrap();
    assert_eq!(s.capabilities.max_forwards_now, 1);
}

#[test]
fn a_rule_the_server_cannot_run_is_refused_with_words() {
    let rig = Rig::new();
    rig.server.set_sieve_extensions(&["fileinto"]);
    let regex = rule("a", "R", "").replace("\"comparator\":\"contains\"", "\"comparator\":\"regex\"");
    let refused = rig.core.filters_save(list(&[regex]));
    assert!(rig.refusal(refused).contains("regular expression"));
    let copy = rule("a", "C", "").replace("\"type\":\"move\"", "\"type\":\"copy\"");
    let refused = rig.core.filters_save(list(&[copy]));
    assert!(rig.refusal(refused).contains("\"copy\""));
    let bad = rule("a", "", "");
    assert!(rig.refusal(rig.core.filters_save(list(&[bad]))).contains("needs a name"));
    assert!(rig.server.sieve_text("filters").is_none());
}
