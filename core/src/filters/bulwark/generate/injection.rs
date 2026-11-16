//! Nothing a user types can add a statement to the script (the M8 rule, for every construct of
//! Bulwark's model): awkward text goes into every string slot and the Sieve written is scanned.

use super::*;

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

    fn text(&mut self) -> String {
        const PIECES: [&str; 20] = [
            "a", "Z", " ", "*", "?", "\\", "\"", "'", "*/", "/*", "#", ";", "{", "}", "[", "]", "ü", "😀", "\\\"",
            "\r\n",
        ];
        let mut s: String = (0..1 + self.below(6)).map(|_| PIECES[self.below(PIECES.len())]).collect();
        if self.below(3) == 0 {
            s.push_str("\"; fileinto \"x\"; stop; \"");
        }
        s
    }

    fn rule(&mut self, n: usize) -> J {
        let fields = ["from", "to", "cc", "subject", "header", "body", "size", "attachment", "all"];
        let comps = [
            "contains",
            "not_contains",
            "is",
            "not_is",
            "starts_with",
            "ends_with",
            "matches",
            "regex",
            "address_is",
            "domain_is",
            "greater_than",
            "has_any",
            "has_type",
            "any",
        ];
        let kinds = ["move", "copy", "forward", "mark_read", "star", "add_label", "discard", "reject", "keep", "stop"];
        let conditions = (0..1 + self.below(3))
            .map(|_| {
                let value = match self.below(3) {
                    0 => J::Arr(vec![J::Str(self.text()), J::Str(self.text())]),
                    _ => J::Str(self.text()),
                };
                let mut c = vec![
                    ("field".to_string(), J::str(fields[self.below(fields.len())])),
                    ("comparator".to_string(), J::str(comps[self.below(comps.len())])),
                    ("value".to_string(), value),
                ];
                if self.below(2) == 0 {
                    c.push(("headerName".to_string(), J::Str(self.text())));
                }
                J::Obj(c)
            })
            .collect();
        let actions = (0..1 + self.below(3))
            .map(|_| {
                let mut a = vec![
                    ("type".to_string(), J::str(kinds[self.below(kinds.len())])),
                    ("value".to_string(), J::Str(self.text())),
                ];
                if self.below(2) == 0 {
                    a.push(("mailboxId".to_string(), J::Str(self.text())));
                }
                a.push(("keepCopy".to_string(), J::Bool(self.below(2) == 0)));
                J::Obj(a)
            })
            .collect();
        let mut o = vec![
            ("id".to_string(), J::str(format!("r{n}"))),
            ("name".to_string(), J::Str(self.text())),
            ("enabled".to_string(), J::Bool(self.below(4) != 0)),
            ("matchType".to_string(), J::str(if self.below(2) == 0 { "all" } else { "any" })),
            ("conditions".to_string(), J::Arr(conditions)),
            ("actions".to_string(), J::Arr(actions)),
            ("stopProcessing".to_string(), J::Bool(self.below(2) == 0)),
        ];
        if self.below(3) == 0 {
            o.push(("includeSpam".to_string(), J::Bool(true)));
        }
        J::Obj(o)
    }
}

/// The number of statements at the top level after the metadata comment, and the command word of
/// every statement inside a block. A tiny scanner of exactly the Sieve the generator writes:
/// comments, quoted strings with escapes, braces, semicolons.
fn statements(text: &str, inner: &mut Vec<String>) -> usize {
    let text = &text[text.find("@metadata:end */").unwrap() + 16..];
    let (mut top, mut depth) = (0, 0);
    let mut chars = text.chars().peekable();
    let mut at_start = true;
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
                at_start = false;
            }
            '{' => {
                depth += 1;
                at_start = true;
            }
            '}' => {
                depth -= 1;
                at_start = false;
                if depth == 0 {
                    top += 1;
                }
            }
            ';' => {
                if depth == 0 {
                    top += 1;
                }
                at_start = true;
            }
            c if c.is_whitespace() => {}
            c => {
                if at_start && depth > 0 {
                    let mut word = String::from(c);
                    while let Some(n) = chars.peek().filter(|n| n.is_ascii_alphabetic()) {
                        word.push(*n);
                        chars.next();
                    }
                    inner.push(word);
                }
                at_start = false;
            }
        }
    }
    top
}

#[test]
fn nothing_a_user_types_can_add_a_statement() {
    let exts: Vec<String> = [
        "fileinto",
        "copy",
        "mailbox",
        "mailboxid",
        "imap4flags",
        "reject",
        "regex",
        "body",
        "mime",
        "date",
        "relational",
        "spamtestplus",
        "include",
        "envelope",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    let mut g = Gen(0x9E37_79B9_7F4A_7C15);
    for round in 0..300 {
        let rules: Vec<J> = (0..g.below(5)).map(|n| g.rule(n)).collect();
        let vacation = J::Obj(vec![
            ("isEnabled".into(), J::Bool(g.below(2) == 0)),
            ("subject".into(), J::Str(g.text())),
            ("textBody".into(), J::Str(g.text())),
        ]);
        let json = J::Obj(vec![
            ("version".into(), J::Num(1.into())),
            ("rules".into(), J::Arr(rules)),
            ("vacation".into(), vacation),
        ])
        .to_json();
        let m = Metadata::from_json(&json).unwrap();
        let include = g.below(2) == 0;
        let out = generate(&m, "", &Options { external_requires: &[], include_vacation: include, extensions: &exts });
        let comment = &out[..out.find("@metadata:end").unwrap()];
        assert!(!comment[2..].contains("*/"), "round {round}");
        let written = m.rules.iter().filter(|r| r.enabled && skip_reason(r).is_none()).count();
        let vacation_on = m.vacation.as_ref().unwrap().get("isEnabled") == Some(&J::Bool(true));
        let mut inner = Vec::new();
        let top = statements(&out, &mut inner);
        let has_require = out.contains("\nrequire [");
        let expected = usize::from(has_require) + usize::from(include) + usize::from(vacation_on) + written;
        assert_eq!(top, expected, "round {round}\n{out}");
        let known = ["fileinto", "redirect", "addflag", "discard", "reject", "stop"];
        assert!(inner.iter().all(|w| known.contains(&w.as_str())), "round {round}: {inner:?}\n{out}");
    }
}
