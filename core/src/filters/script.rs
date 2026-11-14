//! The rules as a Sieve script, and back.
//!
//! The rules are the truth. They are kept as JSON in comment lines at the top of the script
//! (base64url, so nothing a user types can end the comment or add a statement) and the Sieve
//! below is generated from them. Reading a script never parses Sieve: it decodes the comment,
//! generates the script again and compares. A script that is not exactly that was edited.

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde::{Deserialize, Serialize};

use super::check::needed_extensions;
use crate::model::{FilterCondition, FilterField, FilterMatch, FilterRule};

/// The format this code writes and reads.
pub const FORMAT: u32 = 1;

const FIRST_LINE: &str = "# kweb mail filters, format 1. Written by the app: change the rules in the app.";
const DATA_PREFIX: &str = "# kweb-rules ";
const CHUNK: usize = 72;

/// A rule and the folder path that was written into the script for it (the move target, or the
/// Trash for a delete). The path is what tells a renamed folder from a hand edit.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    pub rule: FilterRule,
    pub path: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct Payload {
    format: u32,
    rules: Vec<Entry>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Reading {
    Rules(Vec<Entry>),
    /// No readable rules: the comment is missing or invalid, the format is newer than this
    /// code knows, or the Sieve is not what the rules give.
    Edited,
}

/// The script for `entries`. Disabled rules are in the comment and not in the Sieve.
pub fn generate(entries: &[Entry]) -> String {
    let entries: Vec<Entry> = entries
        .iter()
        .map(|e| {
            let mut e = e.clone();
            e.rule.folder_missing = false;
            e
        })
        .collect();
    let json = serde_json::to_string(&Payload { format: FORMAT, rules: entries.clone() }).unwrap_or_default();
    let encoded = URL_SAFE_NO_PAD.encode(json);
    let mut out = format!("{FIRST_LINE}\n");
    for chunk in encoded.as_bytes().chunks(CHUNK) {
        out.push_str(DATA_PREFIX);
        out.push_str(&String::from_utf8_lossy(chunk));
        out.push('\n');
    }
    let enabled: Vec<&Entry> = entries.iter().filter(|e| e.rule.enabled).collect();
    let mut extensions: Vec<&str> = enabled.iter().flat_map(|e| needed_extensions(&e.rule)).collect();
    extensions.sort_unstable();
    extensions.dedup();
    if !extensions.is_empty() {
        let list: Vec<String> = extensions.iter().map(|e| quote(e)).collect();
        out.push_str(&format!("\nrequire [{}];\n", list.join(", ")));
    }
    for entry in enabled {
        out.push('\n');
        out.push_str(&block(entry));
    }
    out
}

/// The rules in `text`, or that it was edited.
pub fn read(text: &str) -> Reading {
    let encoded: String = text.lines().filter_map(|l| l.strip_prefix(DATA_PREFIX)).collect();
    if encoded.is_empty() {
        return Reading::Edited;
    }
    let Ok(bytes) = URL_SAFE_NO_PAD.decode(encoded) else { return Reading::Edited };
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes) else { return Reading::Edited };
    if value["format"].as_u64() != Some(u64::from(FORMAT)) {
        return Reading::Edited;
    }
    let Ok(payload) = serde_json::from_value::<Payload>(value) else { return Reading::Edited };
    if generate(&payload.rules) == text {
        Reading::Rules(payload.rules)
    } else {
        Reading::Edited
    }
}

/// A Sieve string: in quotes, with `"` and `\` escaped.
fn quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        if c == '"' || c == '\\' {
            out.push('\\');
        }
        out.push(c);
    }
    out.push('"');
    out
}

/// A `:matches` pattern that means `s` literally: `*`, `?` and `\` escaped.
fn literal_pattern(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if matches!(c, '*' | '?' | '\\') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

fn test(condition: &FilterCondition) -> String {
    let names = match &condition.field {
        FilterField::From => quote("from"),
        FilterField::Recipient => format!("[{}, {}]", quote("to"), quote("cc")),
        FilterField::Subject => quote("subject"),
        FilterField::Header { name } => quote(name),
    };
    let (kind, pattern) = match condition.how {
        FilterMatch::Contains => (":contains", condition.value.clone()),
        FilterMatch::Is => (":is", condition.value.clone()),
        FilterMatch::BeginsWith => (":matches", format!("{}*", literal_pattern(&condition.value))),
        FilterMatch::EndsWith => (":matches", format!("*{}", literal_pattern(&condition.value))),
    };
    // The recipient is tested address by address: a `header` test would compare the whole line,
    // "Team <team@example.org>, bob@example.org", and Is, Begins and Ends would almost never match.
    let test = if condition.field == FilterField::Recipient { "address" } else { "header" };
    format!("{test} {kind} {names} {}", quote(&pattern))
}

fn block(entry: &Entry) -> String {
    let rule = &entry.rule;
    let tests: Vec<String> = rule.conditions.iter().map(test).collect();
    let mut out = format!("if {} ({}) {{\n", if rule.match_all { "allof" } else { "anyof" }, tests.join(", "));
    if rule.mark_read {
        out.push_str("    addflag \"\\\\Seen\";\n");
    }
    if rule.star {
        out.push_str("    addflag \"\\\\Flagged\";\n");
    }
    if let Some(path) = &entry.path {
        if rule.move_to.is_some() || rule.delete {
            out.push_str(&format!("    fileinto {};\n", quote(path)));
        }
    }
    if rule.stop || rule.delete {
        out.push_str("    stop;\n");
    }
    out.push_str("}\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A small deterministic generator (xorshift) of awkward text and rules.
    struct Gen(u64);

    impl Gen {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }
        fn below(&mut self, n: usize) -> usize {
            (self.next() % n as u64) as usize
        }
        fn flag(&mut self) -> bool {
            self.next().is_multiple_of(2)
        }
        fn text(&mut self, line_breaks: bool) -> String {
            const PIECES: [&str; 22] = [
                "a", "Z", " ", "*", "?", "\\", "\"", "'", "*/", "/*", "#", ";", "{", "}", "[", "]", "ü", "日本", "😀",
                "\\\\", "\\\"", "require",
            ];
            let mut s: String = (0..1 + self.below(8)).map(|_| PIECES[self.below(PIECES.len())]).collect();
            if line_breaks && self.flag() {
                s.push_str(["\n", "\r\n", "\r", "\u{2028}", "\0"][self.below(5)]);
                s.push_str("fileinto \"x\"; stop;");
            }
            s
        }
        fn condition(&mut self) -> FilterCondition {
            let field = match self.below(4) {
                0 => FilterField::From,
                1 => FilterField::Recipient,
                2 => FilterField::Subject,
                _ => FilterField::Header { name: ["List-Id", "X-Mailer", "a"][self.below(3)].into() },
            };
            let how =
                [FilterMatch::Contains, FilterMatch::Is, FilterMatch::BeginsWith, FilterMatch::EndsWith][self.below(4)];
            FilterCondition { field, how, value: self.text(false) }
        }
        fn entry(&mut self, n: usize) -> Entry {
            let delete = self.below(5) == 0;
            let moved = !delete && self.flag();
            let path = (delete || moved).then(|| self.text(false));
            let rule = FilterRule {
                id: format!("r{n}"),
                name: self.text(true),
                enabled: self.below(4) != 0,
                match_all: self.flag(),
                conditions: (0..1 + self.below(3)).map(|_| self.condition()).collect(),
                move_to: moved.then(|| format!("mb{}", self.below(9))),
                mark_read: self.flag(),
                star: self.flag(),
                delete,
                stop: self.flag(),
                folder_missing: false,
            };
            Entry { rule, path }
        }
        fn entries(&mut self) -> Vec<Entry> {
            (0..self.below(6)).map(|n| self.entry(n)).collect()
        }
    }

    /// What `text` has at its top level and inside its blocks: `(statements at the top, commands
    /// inside blocks)`. A tiny scanner of exactly the Sieve the generator writes: comments,
    /// quoted strings with escapes, braces, semicolons.
    fn statements(text: &str) -> (usize, usize) {
        let (mut top, mut inner, mut depth) = (0, 0, 0);
        let mut chars = text.chars().peekable();
        while let Some(c) = chars.next() {
            match c {
                '#' => {
                    for c in chars.by_ref() {
                        if c == '\n' {
                            break;
                        }
                    }
                }
                '"' => {
                    while let Some(c) = chars.next() {
                        match c {
                            '\\' => {
                                chars.next();
                            }
                            '"' => break,
                            _ => {}
                        }
                    }
                }
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        top += 1;
                    }
                }
                ';' if depth == 0 => top += 1,
                ';' => inner += 1,
                _ => {}
            }
        }
        (top, inner)
    }

    #[test]
    fn the_rules_come_back_from_the_script() {
        let mut gen = Gen(0x9E37_79B9_7F4A_7C15);
        for round in 0..300 {
            let entries = gen.entries();
            let text = generate(&entries);
            assert_eq!(read(&text), Reading::Rules(entries), "round {round}:\n{text}");
        }
    }

    #[test]
    fn a_changed_byte_in_the_sieve_is_an_edit() {
        let mut gen = Gen(42);
        let mut tried = 0;
        for _ in 0..100 {
            let entries = gen.entries();
            let text = generate(&entries);
            let sieve_start = text.lines().take_while(|l| l.starts_with('#')).map(|l| l.len() + 1).sum::<usize>();
            for at in (sieve_start..text.len()).filter(|at| text.is_char_boundary(*at)).step_by(7) {
                let mut changed = text.clone();
                changed.insert(at, ' ');
                assert_eq!(read(&changed), Reading::Edited, "inserted at {at} in:\n{text}");
                let mut removed = text.clone();
                removed.remove(at);
                assert_eq!(read(&removed), Reading::Edited, "removed at {at} in:\n{text}");
                tried += 1;
            }
        }
        assert!(tried > 100);
    }

    #[test]
    fn nothing_a_user_types_adds_a_statement() {
        let mut gen = Gen(7);
        for _ in 0..300 {
            let entries = gen.entries();
            let text = generate(&entries);
            let enabled: Vec<&Entry> = entries.iter().filter(|e| e.rule.enabled).collect();
            let uses_extension = enabled.iter().any(|e| !needed_extensions(&e.rule).is_empty());
            let expected_top = enabled.len() + usize::from(uses_extension);
            let expected_inner: usize = enabled
                .iter()
                .map(|e| {
                    let r = &e.rule;
                    usize::from(r.mark_read)
                        + usize::from(r.star)
                        + usize::from(r.move_to.is_some() || r.delete)
                        + usize::from(r.stop || r.delete)
                })
                .sum();
            // The `require` line and every `if` block count as one statement each; the commands
            // inside a block are counted apart.
            let (top, inner) = statements(&text);
            assert_eq!(top, expected_top, "top level of:\n{text}");
            assert_eq!(inner, expected_inner, "inside blocks of:\n{text}");
        }
    }

    #[test]
    fn the_comment_cannot_end_early() {
        let rule_name = "x */ \n# \r\n \"quoted\" \\ back ü 日本 😀";
        let mut entry = Gen(1).entry(0);
        entry.rule.name = rule_name.into();
        let text = generate(std::slice::from_ref(&entry));
        for line in text.lines().take_while(|l| !l.is_empty()) {
            assert!(line.starts_with('#'), "{line}");
        }
        assert!(!text.contains(rule_name));
        assert_eq!(read(&text), Reading::Rules(vec![entry]));
    }

    fn two_rules() -> Vec<Entry> {
        let condition = |field, how, value: &str| FilterCondition { field, how, value: value.into() };
        let rule = |name: &str, conditions, move_to: Option<&str>, mark_read, star, stop| FilterRule {
            id: name.into(),
            name: name.into(),
            enabled: true,
            match_all: true,
            conditions,
            move_to: move_to.map(str::to_string),
            mark_read,
            star,
            delete: false,
            stop,
            folder_missing: false,
        };
        vec![
            Entry {
                rule: rule(
                    "Bills",
                    vec![
                        condition(FilterField::From, FilterMatch::Contains, "billing@"),
                        condition(FilterField::Subject, FilterMatch::EndsWith, "50%* \"off\""),
                    ],
                    Some("mb2"),
                    true,
                    true,
                    true,
                ),
                path: Some("Archive/Bills".into()),
            },
            Entry {
                rule: FilterRule {
                    match_all: false,
                    ..rule(
                        "Team",
                        vec![
                            condition(FilterField::Recipient, FilterMatch::Is, "team@example.org"),
                            condition(FilterField::Header { name: "List-Id".into() }, FilterMatch::BeginsWith, "dev?"),
                        ],
                        None,
                        true,
                        false,
                        false,
                    )
                },
                path: None,
            },
        ]
    }

    #[test]
    fn the_sieve_for_two_rules() {
        let text = generate(&two_rules());
        let sieve = &text[text.find("\nrequire").unwrap() + 1..];
        assert_eq!(
            sieve,
            "require [\"fileinto\", \"imap4flags\"];\n\
             \n\
             if allof (header :contains \"from\" \"billing@\", header :matches \"subject\" \"*50%\\\\* \\\"off\\\"\") {\n    \
             addflag \"\\\\Seen\";\n    \
             addflag \"\\\\Flagged\";\n    \
             fileinto \"Archive/Bills\";\n    \
             stop;\n\
             }\n\
             \n\
             if anyof (address :is [\"to\", \"cc\"] \"team@example.org\", header :matches \"List-Id\" \"dev\\\\?*\") {\n    \
             addflag \"\\\\Seen\";\n\
             }\n"
        );
    }

    #[test]
    fn an_empty_list_is_a_script_that_keeps_everything() {
        let text = generate(&[]);
        assert_eq!(statements(&text), (0, 0));
        assert!(!text.contains("require"));
        assert_eq!(read(&text), Reading::Rules(Vec::new()));
    }

    #[test]
    fn disabled_rules_are_kept_but_not_written() {
        let mut entries = two_rules();
        entries[0].rule.enabled = false;
        let text = generate(&entries);
        assert!(!text.contains("fileinto"));
        assert!(text.contains("anyof"));
        assert_eq!(read(&text), Reading::Rules(entries));
    }

    #[test]
    fn a_missing_broken_or_newer_comment_is_an_edit() {
        let text = generate(&two_rules());
        let sieve_only: String =
            text.lines().filter(|l| !l.starts_with(DATA_PREFIX)).map(|l| format!("{l}\n")).collect();
        assert_eq!(read(&sieve_only), Reading::Edited);
        assert_eq!(read(""), Reading::Edited);
        assert_eq!(read("keep;\n"), Reading::Edited);
        let broken = text.replacen(DATA_PREFIX, &format!("{DATA_PREFIX}!!"), 1);
        assert_eq!(read(&broken), Reading::Edited);
        let newer = format!("{FIRST_LINE}\n{DATA_PREFIX}{}\n", URL_SAFE_NO_PAD.encode(r#"{"format":2,"rules":[]}"#));
        assert_eq!(read(&newer), Reading::Edited);
        let not_rules = format!("{FIRST_LINE}\n{DATA_PREFIX}{}\n", URL_SAFE_NO_PAD.encode(r#"{"format":1,"rules":3}"#));
        assert_eq!(read(&not_rules), Reading::Edited);
        // The same text with Windows line endings is not what the generator gives.
        assert_eq!(read(&text.replace('\n', "\r\n")), Reading::Edited);
    }
}
