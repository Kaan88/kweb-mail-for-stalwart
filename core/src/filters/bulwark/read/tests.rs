use super::*;
use crate::filters::bulwark::generate::{generate, Options};

fn ext(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| s.to_string()).collect()
}

const RULE: &str = "{\"id\":\"r1\",\"name\":\"A\",\"enabled\":true,\"matchType\":\"all\",\"conditions\":[{\"field\":\"subject\",\"comparator\":\"regex\",\"value\":\"^x\"}],\"actions\":[{\"type\":\"move\",\"value\":\"Archive\",\"mailboxId\":\"m1\"}],\"stopProcessing\":false,\"activeFrom\":\"2026-10-05T06:00:00.000Z\",\"future\":1}";

fn meta() -> Metadata {
    Metadata::from_json(&format!("{{\"version\":2,\"rules\":[{RULE}],\"x\":true}}")).unwrap()
}

fn written(external: &str, reqs: &[String]) -> String {
    let e = ext(&["fileinto", "mailbox", "mailboxid", "regex", "date", "relational"]);
    generate(&meta(), external, &Options { external_requires: reqs, include_vacation: false, extensions: &e })
}

fn parsed(text: &str) -> Parsed {
    match read(text, &ext(&["fileinto", "mailbox", "mailboxid", "regex", "date", "relational"])) {
        Reading::Rules(p) => *p,
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_script_made_by_the_generator_reads_and_writes_back_identically() {
    let text = written("", &[]);
    let p = parsed(&text);
    assert!(!p.differs && p.external.is_empty() && p.meta.rules.len() == 1);
    let again = generate(
        &p.meta,
        &p.external,
        &Options {
            external_requires: &p.external_requires,
            include_vacation: p.meta.include_vacation,
            extensions: &ext(&["fileinto", "mailbox", "mailboxid", "regex", "date", "relational"]),
        },
    );
    assert_eq!(again, text);
}

#[test]
fn the_external_section_and_its_requires_survive() {
    let external = "# rule:[X]\nif header :contains \"Subject\" \"y\" {\n    fileinto :copy \"Z\";\n    keep;\n}";
    let reqs = ext(&["copy", "fileinto"]);
    let text = written(external, &reqs);
    let p = parsed(&text);
    assert_eq!(p.external, external);
    assert_eq!(p.external_requires, ext(&["copy", "date", "fileinto", "mailbox", "mailboxid", "regex", "relational"]));
    let e = ext(&["fileinto", "mailbox", "mailboxid", "regex", "date", "relational"]);
    let opts = Options { external_requires: &p.external_requires, include_vacation: false, extensions: &e };
    assert_eq!(generate(&p.meta, &p.external, &opts), text);
}

#[test]
fn the_bulwark_fixture_is_read_with_its_external_rules() {
    let text = include_str!("../../../../tests/fixtures/bulwark/mixed-origins.sieve").replace("\r\n", "\n");
    let e = ext(&["body", "copy", "fileinto", "imap4flags", "relational"]);
    match read(&text, &e) {
        Reading::Rules(p) => {
            assert_eq!(p.meta.rules.len(), 2);
            assert!(!p.differs);
            assert!(p.external.starts_with("# rule:[Finance - auto-file invoices]"));
            assert!(p.external.ends_with("stop;\n}"));
            assert_eq!(p.external_requires, ext(&["body", "copy", "fileinto", "imap4flags", "relational"]));
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn foreign_opaque_and_empty_scripts() {
    let e = ext(&[]);
    assert_eq!(read("  \n", &e), Reading::Empty);
    assert_eq!(read("require \"fileinto\";\nfileinto \"A\";\n", &e), Reading::Foreign);
    assert_eq!(read("/* @metadata:begin\n{not json\n@metadata:end */\n", &e), Reading::Opaque);
    assert_eq!(read("/* @metadata:begin\n{\"version\":3,\"rules\":[]}\n@metadata:end */\n", &e), Reading::Opaque);
    assert_eq!(read("/* @metadata:begin\n{\"version\":1,\"rules\":[]}\n", &e), Reading::Opaque);
    // A rule it cannot read.
    assert_eq!(
        read("/* @metadata:begin\n{\"version\":1,\"rules\":[{\"id\":1}]}\n@metadata:end */\n", &e),
        Reading::Opaque
    );
    // Text where the rule part should be, and no external header: not touched.
    let text = written("", &[]).replace("\"m1\" \"Archive\";", "\"m1\" \"Elsewhere\";");
    assert_eq!(
        read(&text, &ext(&["fileinto", "mailbox", "mailboxid", "regex", "date", "relational"])),
        Reading::Opaque
    );
}

#[test]
fn another_versions_rule_part_is_replaced_but_the_external_part_kept() {
    let text =
        written("if true { keep; }", &[]).replace("fileinto :mailboxid \"m1\" \"Archive\"", "fileinto \"Archive\"");
    let p = parsed(&text);
    assert!(p.differs);
    assert_eq!(p.external, "if true { keep; }");
}

#[test]
fn include_is_the_apps_unless_the_external_part_uses_it() {
    let m = "/* @metadata:begin\n{\"version\":1,\"rules\":[]}\n@metadata:end */\n\nrequire [\"include\"];\n";
    match read(m, &ext(&[])) {
        Reading::Rules(p) => assert!(p.external_requires.is_empty()),
        other => panic!("{other:?}"),
    }
    let m = format!("{m}\n# --- External rules (managed outside Bulwark) ---\ninclude :personal \"x\";\n");
    match read(&m, &ext(&[])) {
        Reading::Rules(p) => assert_eq!(p.external_requires, ext(&["include"])),
        other => panic!("{other:?}"),
    }
}
