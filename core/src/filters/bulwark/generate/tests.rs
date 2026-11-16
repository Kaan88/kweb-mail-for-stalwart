//! Expected outputs written out from Bulwark's generator (`lib/sieve/generator.ts`) and its tests
//! (`lib/sieve/__tests__/generator.test.ts`, `vacation-forward.test.ts`, `period.test.ts`):
//! whole scripts, compared as exact strings.

use super::*;

const GUARD: &str = "not spamtest :percent :value \"ge\" :comparator \"i;ascii-numeric\" \"50\"";

fn ext(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| s.to_string()).collect()
}

fn meta(rules: &[&str]) -> Metadata {
    let json = format!("{{\"version\":1,\"rules\":[{}]}}", rules.join(","));
    Metadata::from_json(&json).unwrap()
}

/// Bulwark's own test rule: id rule-1, "Test Rule", From contains, move to Archive.
fn test_rule(conditions: &str, actions: &str, tail: &str) -> String {
    format!(
        "{{\"id\":\"rule-1\",\"name\":\"Test Rule\",\"enabled\":true,\"matchType\":\"all\",\"conditions\":[{conditions}],\"actions\":[{actions}],\"stopProcessing\":false{tail}}}"
    )
}

const FROM: &str = "{\"field\":\"from\",\"comparator\":\"contains\",\"value\":\"test@example.com\"}";
const MOVE: &str = "{\"type\":\"move\",\"value\":\"Archive\"}";

fn head(json: &str) -> String {
    format!("/* @metadata:begin\n{json}\n@metadata:end */\n")
}

fn run(m: &Metadata, extensions: &[&str]) -> String {
    generate(m, "", &Options { external_requires: &[], include_vacation: false, extensions: &ext(extensions) })
}

#[test]
fn an_empty_script_is_the_metadata_and_a_blank_line() {
    assert_eq!(run(&meta(&[]), &[]), format!("{}\n", head("{\"version\":1,\"rules\":[]}")));
}

#[test]
fn a_single_move_rule() {
    let r = test_rule(FROM, MOVE, "");
    let want = format!(
        "{}\nrequire [\"fileinto\"];\n\n# Rule: Test Rule\nif header :contains \"From\" \"test@example.com\" {{\n    fileinto \"Archive\";\n}}\n",
        head(&format!("{{\"version\":1,\"rules\":[{r}]}}"))
    );
    assert_eq!(run(&meta(&[&r]), &[]), want);
}

#[test]
fn the_spam_guard_and_its_requires() {
    let r = test_rule(FROM, MOVE, "");
    let want = format!(
        "{}\nrequire [\"comparator-i;ascii-numeric\", \"fileinto\", \"relational\", \"spamtestplus\"];\n\n# Rule: Test Rule\nif allof(header :contains \"From\" \"test@example.com\", {GUARD}) {{\n    fileinto \"Archive\";\n}}\n",
        head(&format!("{{\"version\":1,\"rules\":[{r}]}}"))
    );
    assert_eq!(run(&meta(&[&r]), &["fileinto", "relational", "spamtest", "spamtestplus"]), want);
}

#[test]
fn the_guard_extends_an_allof_wraps_an_anyof_and_skips_opted_in_rules() {
    let two = "{\"field\":\"from\",\"comparator\":\"contains\",\"value\":\"a\"},{\"field\":\"subject\",\"comparator\":\"contains\",\"value\":\"b\"}";
    let exts = ["fileinto", "relational", "spamtest", "spamtestplus"];
    let all = run(&meta(&[&test_rule(two, MOVE, "")]), &exts);
    assert!(all.contains(&format!(
        "if allof(header :contains \"From\" \"a\", header :contains \"Subject\" \"b\", {GUARD}) {{"
    )));
    let any = test_rule(two, MOVE, "").replace("\"matchType\":\"all\"", "\"matchType\":\"any\"");
    let any = run(&meta(&[&any]), &exts);
    assert!(any.contains(&format!(
        "if allof(anyof(header :contains \"From\" \"a\", header :contains \"Subject\" \"b\"), {GUARD}) {{"
    )));
    let opted = test_rule(FROM, MOVE, ",\"includeSpam\":true");
    let out = run(&meta(&[&opted]), &exts);
    assert!(!out.contains("spamtest"));
    assert!(out.contains("\"includeSpam\":true}]"));
}

#[test]
fn flags_come_first_and_stop_processing_adds_one_stop() {
    let actions = "{\"type\":\"move\",\"value\":\"Archive\"},{\"type\":\"star\"},{\"type\":\"mark_read\"}";
    let r = test_rule(FROM, actions, "").replace("\"stopProcessing\":false", "\"stopProcessing\":true");
    let want = format!(
        "{}\nrequire [\"fileinto\", \"imap4flags\"];\n\n# Rule: Test Rule\nif header :contains \"From\" \"test@example.com\" {{\n    addflag \"\\\\Flagged\";\n    addflag \"\\\\Seen\";\n    fileinto \"Archive\";\n    stop;\n}}\n",
        head(&format!("{{\"version\":1,\"rules\":[{r}]}}"))
    );
    assert_eq!(run(&meta(&[&r]), &[]), want);
    // A stop action already in the block: no second one.
    let r = test_rule(FROM, "{\"type\":\"stop\"},{\"type\":\"discard\"}", "")
        .replace("\"stopProcessing\":false", "\"stopProcessing\":true");
    assert!(run(&meta(&[&r]), &[]).contains("    stop;\n    discard;\n}\n"));
    let r = test_rule(FROM, "{\"type\":\"reject\",\"value\":\"No\"}", "")
        .replace("\"stopProcessing\":false", "\"stopProcessing\":true");
    assert!(run(&meta(&[&r]), &[]).contains("    reject \"No\";\n    stop;\n}\n"));
}

#[test]
fn a_move_to_a_folder_id_needs_mailbox_and_mailboxid() {
    let r = test_rule(FROM, "{\"type\":\"move\",\"value\":\"Work/Old\",\"mailboxId\":\"m42\"}", "");
    let out = run(&meta(&[&r]), &["fileinto", "mailbox", "mailboxid"]);
    assert!(out.contains("require [\"fileinto\", \"mailbox\", \"mailboxid\"];\n"));
    assert!(out.contains("    fileinto :mailboxid \"m42\" \"Work/Old\";\n"));
    let r = test_rule(FROM, "{\"type\":\"copy\",\"value\":\"Work\",\"mailboxId\":\"m42\"}", "");
    let out = run(&meta(&[&r]), &["fileinto", "copy"]);
    assert!(out.contains("    fileinto :copy \"Work\";\n") && !out.contains("mailboxid"));
}

#[test]
fn a_regex_and_a_period_make_version_two() {
    let cond = "{\"field\":\"subject\",\"comparator\":\"regex\",\"value\":\"^a\"}";
    let r = test_rule(cond, "{\"type\":\"mark_read\"}", ",\"activeFrom\":\"2026-10-05T06:00:00.000Z\"");
    let from = "anyof(currentdate :zone \"+0000\" :value \"gt\" \"date\" \"2026-10-05\", allof(currentdate :zone \"+0000\" :is \"date\" \"2026-10-05\", currentdate :zone \"+0000\" :value \"ge\" \"time\" \"06:00:00\"))";
    let want = format!(
        "{}\nrequire [\"date\", \"imap4flags\", \"regex\", \"relational\"];\n\n# Rule: Test Rule\nif allof({from}, header :regex \"Subject\" \"^a\") {{\n    addflag \"\\\\Seen\";\n}}\n",
        head(&format!("{{\"version\":2,\"rules\":[{r}]}}"))
    );
    assert_eq!(run(&meta(&[&r]), &[]), want);
}

#[test]
fn the_vacation_include_comes_before_the_rules() {
    let r = test_rule(FROM, MOVE, "");
    let mut m = meta(&[&r]);
    m.include_vacation = true;
    let out = generate(&m, "", &Options { external_requires: &[], include_vacation: true, extensions: &[] });
    let want = format!(
        "{}\nrequire [\"fileinto\", \"include\"];\n\n# Vacation auto-reply\ninclude :personal :optional \"vacation\";\n\n# Rule: Test Rule\nif header :contains \"From\" \"test@example.com\" {{\n    fileinto \"Archive\";\n}}\n",
        head(&format!("{{\"version\":1,\"rules\":[{r}],\"includeVacation\":true}}"))
    );
    assert_eq!(out, want);
}

#[test]
fn forwarding_from_the_vacation_card_runs_ahead_of_the_rules() {
    let r = test_rule(
        "{\"field\":\"from\",\"comparator\":\"contains\",\"value\":\"autoscout24\"}",
        "{\"type\":\"mark_read\"}",
        "",
    );
    let mut m = meta(&[&r]);
    m.include_vacation = true;
    m.vacation_forward = Some(Forward {
        enabled: true,
        to: "kollege@example.com".into(),
        keep_copy: false,
        active_from: None,
        active_until: None,
    });
    let exts = [
        "fileinto",
        "imap4flags",
        "include",
        "copy",
        "date",
        "relational",
        "spamtestplus",
        "comparator-i;ascii-numeric",
    ];
    let out = generate(&m, "", &Options { external_requires: &[], include_vacation: true, extensions: &ext(&exts) });
    let want = format!(
        "{}\nrequire [\"comparator-i;ascii-numeric\", \"imap4flags\", \"include\", \"relational\", \"spamtestplus\"];\n\n# Vacation auto-reply\ninclude :personal :optional \"vacation\";\n\n# Vacation forwarding\nif {GUARD} {{\n    redirect \"kollege@example.com\";\n    stop;\n}}\n\n# Rule: Test Rule\nif header :contains \"From\" \"autoscout24\" {{\n    addflag \"\\\\Seen\";\n}}\n",
        head(&format!(
            "{{\"version\":2,\"rules\":[{r}],\"includeVacation\":true,\"vacationForward\":{{\"enabled\":true,\"to\":\"kollege@example.com\",\"keepCopy\":false}}}}"
        ))
    );
    assert_eq!(out, want);
    // Without the spam test on the server: `true`; keeping a copy: no stop.
    let only_include = ext(&["include"]);
    let opts = Options { external_requires: &[], include_vacation: true, extensions: &only_include };
    let out = generate(&m, "", &opts);
    assert!(out.contains("# Vacation forwarding\nif true {\n    redirect \"kollege@example.com\";\n    stop;\n}\n"));
    m.vacation_forward.as_mut().unwrap().keep_copy = true;
    let out = generate(&m, "", &opts);
    assert!(out.contains("    redirect :copy \"kollege@example.com\";\n}\n") && out.contains("\"copy\""));
}

#[test]
fn an_audience_narrows_the_include() {
    let mut m = meta(&[]);
    m.include_vacation = true;
    m.vacation_audience = Some(Audience { only: "external".into(), domains: vec!["a.com".into(), "b.org".into()] });
    let out = generate(&m, "", &Options { external_requires: &[], include_vacation: true, extensions: &[] });
    let want = format!(
        "{}\nrequire [\"envelope\", \"include\"];\n\n# Vacation auto-reply\nif not envelope :domain :is \"from\" [\"a.com\", \"b.org\"] {{\n    include :personal :optional \"vacation\";\n}}\n",
        head("{\"version\":2,\"rules\":[],\"includeVacation\":true,\"vacationAudience\":{\"only\":\"external\",\"domains\":[\"a.com\",\"b.org\"]}}")
    );
    assert_eq!(out, want);
}

#[test]
fn bulwarks_own_auto_reply_and_the_comment_escape() {
    let r = test_rule(
        "{\"field\":\"subject\",\"comparator\":\"contains\",\"value\":[\"a*/b\",\"*/\"]}",
        "{\"type\":\"keep\"}",
        "",
    )
    .replace("Test Rule", "Invoices */ keep");
    let mut m = meta(&[&r]);
    m.vacation = J::parse("{\"isEnabled\":true,\"subject\":\"Away */\",\"textBody\":\"Back \\\"soon\\\"\"}");
    let out = run(&m, &[]);
    let comment = &out[..out.find("@metadata:end").unwrap()];
    assert!(!comment[2..].contains("*/"));
    assert!(
        comment.contains("Invoices *\\/ keep")
            && comment.contains("\"vacation\":{\"isEnabled\":true,\"subject\":\"Away *\\/\"")
    );
    assert!(out.contains("\nrequire [\"fileinto\", \"vacation\"];\n"));
    assert!(out.contains("# Vacation auto-reply\nvacation :subject \"Away */\" \"Back \\\"soon\\\"\";\n"));
    assert!(out.contains(
        "# Rule: Invoices */ keep\nif header :contains \"Subject\" [\"a*/b\", \"*/\"] {\n    fileinto \"INBOX\";\n}\n"
    ));
}

#[test]
fn each_condition_has_its_sieve() {
    let cases = [
        ("{\"field\":\"all\",\"comparator\":\"any\",\"value\":\"\"}", "true"),
        ("{\"field\":\"size\",\"comparator\":\"greater_than\",\"value\":\"1M\"}", "size :over 1M"),
        ("{\"field\":\"size\",\"comparator\":\"less_than\",\"value\":\"x; stop\"}", "size :under 0"),
        ("{\"field\":\"body\",\"comparator\":\"is\",\"value\":\"exact\"}", "body :is \"exact\""),
        ("{\"field\":\"body\",\"comparator\":\"regex\",\"value\":\"a.b\"}", "body :regex \"a.b\""),
        (
            "{\"field\":\"attachment\",\"comparator\":\"has_any\",\"value\":\"\"}",
            "header :mime :anychild :contains \"Content-Disposition\" \"attachment\"",
        ),
        (
            "{\"field\":\"attachment\",\"comparator\":\"has_type\",\"value\":[\".pdf\",\"*xml\"]}",
            "header :mime :anychild :matches [\"Content-Disposition\", \"Content-Type\"] [\"*.pdf*\", \"*.xml*\"]",
        ),
        ("{\"field\":\"from\",\"comparator\":\"address_is\",\"value\":\"a@b.c\"}", "address :is \"From\" \"a@b.c\""),
        (
            "{\"field\":\"to\",\"comparator\":\"domain_is\",\"value\":[\"b.c\",\"d.e\"]}",
            "address :domain :is \"To\" [\"b.c\", \"d.e\"]",
        ),
        ("{\"field\":\"subject\",\"comparator\":\"address_is\",\"value\":\"x\"}", "header :contains \"Subject\" \"x\""),
        (
            "{\"field\":\"from\",\"comparator\":\"not_contains\",\"value\":\"spam\"}",
            "not header :contains \"From\" \"spam\"",
        ),
        (
            "{\"field\":\"from\",\"comparator\":\"not_is\",\"value\":\"bad@x.com\"}",
            "not header :is \"From\" \"bad@x.com\"",
        ),
        (
            "{\"field\":\"subject\",\"comparator\":\"starts_with\",\"value\":\"Re:\"}",
            "header :matches \"Subject\" \"Re:*\"",
        ),
        (
            "{\"field\":\"subject\",\"comparator\":\"ends_with\",\"value\":\"urgent\"}",
            "header :matches \"Subject\" \"*urgent\"",
        ),
        (
            "{\"field\":\"from\",\"comparator\":\"matches\",\"value\":\"*@company.com\"}",
            "header :matches \"From\" \"*@company.com\"",
        ),
        (
            "{\"field\":\"header\",\"comparator\":\"is\",\"value\":\"yes\",\"headerName\":\" X-Spam_Flag-2 \"}",
            "header :is \"X-Spam_Flag-2\" \"yes\"",
        ),
        ("{\"field\":\"header\",\"comparator\":\"contains\",\"value\":\"t\"}", "header :contains \"X-Unknown\" \"t\""),
        (
            "{\"field\":\"subject\",\"comparator\":\"contains\",\"value\":\"say \\\"hi\\\" \\\\ ok\"}",
            "header :contains \"Subject\" \"say \\\"hi\\\" \\\\ ok\"",
        ),
    ];
    for (cond, sieve) in cases {
        let out = run(&meta(&[&test_rule(cond, "{\"type\":\"stop\"}", "")]), &[]);
        assert!(out.contains(&format!("if {sieve} {{\n    stop;\n}}\n")), "{sieve}\n{out}");
    }
}

#[test]
fn each_action_has_its_sieve() {
    let cases = [
        ("{\"type\":\"copy\",\"value\":\"Backup\"}", "fileinto :copy \"Backup\";"),
        ("{\"type\":\"forward\",\"value\":\"f@x.com\",\"keepCopy\":true}", "redirect :copy \"f@x.com\";"),
        ("{\"type\":\"forward\",\"value\":\"f@x.com\"}", "redirect \"f@x.com\";"),
        ("{\"type\":\"add_label\",\"value\":\"Imp\\\"x\"}", "addflag \"$label:Imp\\\"x\";"),
        ("{\"type\":\"discard\"}", "discard;"),
        ("{\"type\":\"move\",\"value\":\"My \\\"Folder\\\"\"}", "fileinto \"My \\\"Folder\\\"\";"),
    ];
    for (action, sieve) in cases {
        let out = run(&meta(&[&test_rule(FROM, action, "")]), &[]);
        assert!(out.contains(&format!("    {sieve}\n}}\n")), "{sieve}\n{out}");
    }
}

#[test]
fn rules_that_cannot_be_written_are_left_out_but_kept_in_the_metadata() {
    let bad = test_rule(
        "{\"field\":\"header\",\"comparator\":\"contains\",\"value\":\"y\",\"headerName\":\"X-Foo.Bar\"}",
        MOVE,
        "",
    )
    .replace("rule-1", "bad");
    let none = test_rule("", MOVE, "").replace("rule-1", "none");
    let off = test_rule(FROM, MOVE, "").replace("rule-1", "off").replace("\"enabled\":true", "\"enabled\":false");
    let good = test_rule(FROM, MOVE, "").replace("rule-1", "good").replace("Test Rule", "Good  rule\\r\\nstop;");
    let out = run(&meta(&[&bad, &none, &off, &good]), &[]);
    assert_eq!(out.matches("# Rule:").count(), 1);
    assert!(out.contains("# Rule: Good rule stop;\n"));
    assert!(!out.contains("X-Foo.Bar\" \"y\""));
    assert!(out.contains("\"id\":\"bad\"") && out.contains("\"id\":\"off\"") && out.contains("\"id\":\"none\""));
}

#[test]
fn the_external_section_is_kept_under_its_header_and_its_requires_are_merged() {
    let r = test_rule(FROM, MOVE, "");
    let ext_text = "\n# rule:[X]\nif header :contains \"Subject\" \"y\" {\n    fileinto :copy \"Z\";\n}\n\n";
    let reqs = vec!["copy".to_string(), "fileinto".to_string(), "include".to_string()];
    let out = generate(
        &meta(&[&r]),
        ext_text,
        &Options { external_requires: &reqs, include_vacation: false, extensions: &[] },
    );
    let want = format!(
        "{}\nrequire [\"copy\", \"fileinto\", \"include\"];\n\n# Rule: Test Rule\nif header :contains \"From\" \"test@example.com\" {{\n    fileinto \"Archive\";\n}}\n\n# --- External rules (managed outside Bulwark) ---\n# rule:[X]\nif header :contains \"Subject\" \"y\" {{\n    fileinto :copy \"Z\";\n}}\n",
        head(&format!("{{\"version\":1,\"rules\":[{r}]}}"))
    );
    assert_eq!(out, want);
}
