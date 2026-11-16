//! The script Bulwark writes (`lib/sieve/generator.ts`), byte for byte. Every string, the order
//! of the sections, the `require` list, the spam guard, the period tests and the vacation
//! include come from there; where this differs from it, a client that reads the script back
//! would see another script than the one it wrote.
//!
//! Nothing a user types can add a statement: every string is escaped as Bulwark escapes it
//! (`"` and `\`), a rule name is one line, numbers and header names are checked, and the JSON
//! in the metadata comment cannot contain `*/`.

use std::collections::BTreeSet;

use super::{
    json::J,
    model::{s, uses_regex, values_of, Audience, Forward, Metadata, Rule, METADATA_BEGIN, METADATA_END},
    period::{period_tests, PERIOD_REQUIRES},
};

/// The reserved script the server keeps for the away message.
pub const VACATION_SCRIPT_NAME: &str = "vacation";
/// The comment that marks the forwarding block as Bulwark's.
pub const VACATION_FORWARD_MARKER: &str = "# Vacation forwarding";
/// The comment above the external section.
pub const EXTERNAL_HEADER: &str = "# --- External rules (managed outside Bulwark) ---";

const SPAM_GUARD: &str = "not spamtest :percent :value \"ge\" :comparator \"i;ascii-numeric\" \"50\"";
const SPAM_GUARD_REQUIRES: [&str; 3] = ["spamtestplus", "relational", "comparator-i;ascii-numeric"];

/// What the server offers, and what else the script carries.
#[derive(Debug, Clone, Default)]
pub struct Options<'a> {
    /// `require` tokens of the external section (and of whatever was in the script before).
    pub external_requires: &'a [String],
    /// Run the server's `vacation` script from this one.
    pub include_vacation: bool,
    /// The server's `sieveExtensions`.
    pub extensions: &'a [String],
}

/// `JSON.stringify` writes a string; this is for the Sieve one: `\` and `"` escaped.
pub fn escape_string(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

fn quoted(value: &str) -> String {
    format!("\"{}\"", escape_string(value))
}

/// JavaScript's `\s`.
fn js_space(c: char) -> bool {
    matches!(
        c,
        '\t' | '\n'
            | '\u{b}'
            | '\u{c}'
            | '\r'
            | ' '
            | '\u{a0}'
            | '\u{1680}'
            | '\u{2028}'
            | '\u{2029}'
            | '\u{202f}'
            | '\u{205f}'
            | '\u{3000}'
            | '\u{feff}'
    ) || ('\u{2000}'..='\u{200a}').contains(&c)
}

/// JavaScript's `String.prototype.trim`.
pub fn js_trim(text: &str) -> &str {
    text.trim_matches(js_space)
}

/// `name.replace(/\s+/g, ' ')`.
fn collapse_spaces(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut in_run = false;
    for c in name.chars() {
        if js_space(c) {
            if !in_run {
                out.push(' ');
            }
            in_run = true;
        } else {
            in_run = false;
            out.push(c);
        }
    }
    out
}

/// Header names Stalwart's Sieve compiler accepts.
pub fn is_valid_sieve_header_name(name: &str) -> bool {
    !name.is_empty() && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// The header a condition tests; a header condition without a name tests X-Unknown.
pub fn condition_header_name(c: &J) -> Option<String> {
    Some(match s(c, "field")? {
        "header" => match js_trim(s(c, "headerName").unwrap_or("")) {
            "" => "X-Unknown".to_string(),
            name => name.to_string(),
        },
        "from" => "From".into(),
        "to" => "To".into(),
        "cc" => "Cc".into(),
        "subject" => "Subject".into(),
        _ => return None,
    })
}

fn string_arg(values: &[String], transform: impl Fn(&str) -> String) -> String {
    match values {
        [one] => quoted(&transform(one)),
        _ => format!("[{}]", values.iter().map(|v| quoted(&transform(v))).collect::<Vec<_>>().join(", ")),
    }
}

/// Whether `value` is `\d+[KMG]?` (case-insensitive).
fn is_size(value: &str) -> bool {
    let digits = value.trim_end_matches(['K', 'M', 'G', 'k', 'm', 'g']);
    let suffix = value.len() - digits.len();
    suffix <= 1 && !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit())
}

fn size_value(c: &J) -> String {
    let raw = match c.get("value") {
        Some(J::Str(v)) => v.as_str(),
        Some(J::Arr(items)) => items.first().and_then(J::as_str).unwrap_or(""),
        _ => "",
    };
    let raw = js_trim(raw);
    if is_size(raw) {
        raw.to_string()
    } else {
        "0".to_string()
    }
}

/// The Sieve test of one condition; `None` when the field is one this code does not know.
fn condition(c: &J) -> Option<String> {
    let field = s(c, "field")?;
    let comparator = s(c, "comparator").unwrap_or("");
    if field == "all" {
        return Some("true".into());
    }
    if field == "size" {
        let op = if comparator == "greater_than" { ":over" } else { ":under" };
        return Some(format!("size {op} {}", size_value(c)));
    }
    let values: Vec<String> = values_of(c).into_iter().filter(|v| !v.is_empty()).collect();
    let plain = |v: &str| v.to_string();
    if field == "body" {
        let kind = match comparator {
            "is" => ":is",
            "regex" => ":regex",
            _ => ":contains",
        };
        return Some(format!("body {kind} {}", string_arg(&values, plain)));
    }
    if field == "attachment" {
        if comparator == "has_any" {
            return Some("header :mime :anychild :contains \"Content-Disposition\" \"attachment\"".into());
        }
        let normalised: Vec<String> = values
            .iter()
            .map(|v| js_trim(v.trim_start_matches(['.', '*'])).to_string())
            .filter(|v| !v.is_empty())
            .collect();
        return Some(format!(
            "header :mime :anychild :matches [\"Content-Disposition\", \"Content-Type\"] {}",
            string_arg(&normalised, |ext| format!("*.{ext}*"))
        ));
    }
    let name = escape_string(&condition_header_name(c)?);
    if matches!(comparator, "address_is" | "domain_is") && matches!(field, "from" | "to" | "cc") {
        let part = if comparator == "domain_is" { ":domain " } else { "" };
        return Some(format!("address {part}:is \"{name}\" {}", string_arg(&values, plain)));
    }
    Some(match comparator {
        "not_contains" => format!("not header :contains \"{name}\" {}", string_arg(&values, plain)),
        "is" => format!("header :is \"{name}\" {}", string_arg(&values, plain)),
        "not_is" => format!("not header :is \"{name}\" {}", string_arg(&values, plain)),
        "starts_with" => format!("header :matches \"{name}\" {}", string_arg(&values, |v| format!("{v}*"))),
        "ends_with" => format!("header :matches \"{name}\" {}", string_arg(&values, |v| format!("*{v}"))),
        "matches" => format!("header :matches \"{name}\" {}", string_arg(&values, plain)),
        "regex" => format!("header :regex \"{name}\" {}", string_arg(&values, plain)),
        _ => format!("header :contains \"{name}\" {}", string_arg(&values, plain)),
    })
}

fn is_flag_action(a: &J) -> bool {
    matches!(s(a, "type"), Some("mark_read" | "star" | "add_label"))
}

fn fileinto_target(a: &J, use_mailbox_id: bool) -> String {
    let path = quoted(s(a, "value").unwrap_or(""));
    match s(a, "mailboxId").filter(|id| !id.is_empty()) {
        Some(id) if use_mailbox_id => format!(":mailboxid {} {path}", quoted(id)),
        _ => path,
    }
}

fn is_known_action(a: &J) -> bool {
    matches!(
        s(a, "type"),
        Some("move" | "copy" | "forward" | "mark_read" | "star" | "add_label" | "discard" | "reject" | "keep" | "stop")
    )
}

fn action_lines(actions: &[J], use_mailbox_id: bool) -> Vec<String> {
    // fileinto (and the implicit keep) store the flags set at that moment: flags first.
    let ordered = actions.iter().filter(|a| is_flag_action(a)).chain(actions.iter().filter(|a| !is_flag_action(a)));
    ordered
        .filter_map(|a| {
            Some(match s(a, "type")? {
                "move" => format!("fileinto {};", fileinto_target(a, use_mailbox_id)),
                "copy" => format!("fileinto :copy {};", fileinto_target(a, use_mailbox_id)),
                "forward" => {
                    let copy = if a.get("keepCopy").is_some_and(J::truthy) { ":copy " } else { "" };
                    format!("redirect {copy}{};", quoted(s(a, "value").unwrap_or("")))
                }
                "mark_read" => "addflag \"\\\\Seen\";".to_string(),
                "star" => "addflag \"\\\\Flagged\";".to_string(),
                "add_label" => format!("addflag \"$label:{}\";", escape_string(s(a, "value").unwrap_or(""))),
                "discard" => "discard;".to_string(),
                "reject" => format!("reject {};", quoted(s(a, "value").unwrap_or(""))),
                "keep" => "fileinto \"INBOX\";".to_string(),
                "stop" => "stop;".to_string(),
                _ => return None,
            })
        })
        .collect()
}

pub fn supports_spam_guard(extensions: &[String]) -> bool {
    ["spamtestplus", "relational"].iter().all(|e| extensions.iter().any(|x| x == e))
}

fn needs_spam_guard(rule: &Rule, extensions: &[String]) -> bool {
    !rule.include_spam
        && rule.actions.iter().any(|a| matches!(s(a, "type"), Some("move" | "copy")))
        && supports_spam_guard(extensions)
}

fn compute_requires(
    rules: &[&Rule],
    vacation_on: bool,
    use_mailbox_id: bool,
    extensions: &[String],
) -> BTreeSet<String> {
    let mut out: BTreeSet<String> = BTreeSet::new();
    let mut add = |e: &str| {
        out.insert(e.to_string());
    };
    if vacation_on {
        add("vacation");
    }
    for rule in rules.iter().filter(|r| r.enabled) {
        if needs_spam_guard(rule, extensions) {
            SPAM_GUARD_REQUIRES.iter().for_each(|e| add(e));
        }
        if period_tests(rule.active_from.as_deref(), rule.active_until.as_deref()).is_some_and(|t| !t.is_empty()) {
            PERIOD_REQUIRES.iter().for_each(|e| add(e));
        }
        for c in &rule.conditions {
            match s(c, "field") {
                Some("body") => add("body"),
                Some("attachment") => add("mime"),
                _ => {}
            }
            if uses_regex(c) {
                add("regex");
            }
        }
        for a in &rule.actions {
            match s(a, "type") {
                Some(kind @ ("move" | "copy")) => {
                    add("fileinto");
                    if kind == "copy" {
                        add("copy");
                    }
                    if use_mailbox_id && s(a, "mailboxId").is_some_and(|id| !id.is_empty()) {
                        add("mailbox");
                        add("mailboxid");
                    }
                }
                Some("keep") => add("fileinto"),
                Some("forward") => {
                    if a.get("keepCopy").is_some_and(J::truthy) {
                        add("copy");
                    }
                }
                Some("mark_read" | "star" | "add_label") => add("imap4flags"),
                Some("reject") => add("reject"),
                _ => {}
            }
        }
    }
    out
}

/// The `require` tokens the rules, the vacation parts and the include need (not the external
/// section's).
pub fn bulwark_requires(meta: &Metadata, include_vacation: bool, extensions: &[String]) -> BTreeSet<String> {
    let rules: Vec<&Rule> = meta.rules.iter().collect();
    let vacation_on = meta.vacation.as_ref().is_some_and(|v| v.get("isEnabled").is_some_and(J::truthy));
    let use_mailbox_id = extensions.iter().any(|e| e == "mailboxid");
    let mut requires = compute_requires(&rules, vacation_on, use_mailbox_id, extensions);
    if include_vacation {
        requires.insert("include".into());
        if meta.vacation_audience.is_some() {
            requires.insert("envelope".into());
        }
    }
    let forward = meta.vacation_forward.as_ref().filter(|f| f.enabled);
    if let Some((f, period)) =
        forward.and_then(|f| period_tests(f.active_from.as_deref(), f.active_until.as_deref()).map(|p| (f, p)))
    {
        if f.keep_copy {
            requires.insert("copy".into());
        }
        if !period.is_empty() {
            PERIOD_REQUIRES.iter().for_each(|e| {
                requires.insert((*e).into());
            });
        }
        if supports_spam_guard(extensions) {
            SPAM_GUARD_REQUIRES.iter().for_each(|e| {
                requires.insert((*e).into());
            });
        }
    }
    requires
}

/// The `require` tokens one rule needs.
pub fn rule_requires(rule: &Rule, use_mailbox_id: bool, extensions: &[String]) -> BTreeSet<String> {
    compute_requires(&[rule], false, use_mailbox_id, extensions)
}

/// Why a rule is left out of the Sieve (it stays in the metadata so it can be corrected), or
/// `None` when it is written.
pub fn skip_reason(rule: &Rule) -> Option<&'static str> {
    if rule.conditions.is_empty() || rule.actions.is_empty() {
        return Some("it has no condition or no action");
    }
    for c in &rule.conditions {
        match s(c, "field") {
            Some("all" | "size" | "body" | "attachment" | "from" | "to" | "cc" | "subject") => {}
            Some("header") => {
                if !condition_header_name(c).is_some_and(|n| is_valid_sieve_header_name(&n)) {
                    return Some("its header name is not one the server accepts");
                }
            }
            _ => return Some("it has a condition this app does not know"),
        }
    }
    if !rule.actions.iter().all(is_known_action) {
        return Some("it has an action this app does not know");
    }
    if period_tests(rule.active_from.as_deref(), rule.active_until.as_deref()).is_none() {
        return Some("its period cannot be read");
    }
    None
}

/// The script for `meta` and the external section `external` (kept as it was, trimmed).
pub fn generate(meta: &Metadata, external: &str, opts: &Options) -> String {
    let extensions = opts.extensions;
    let vacation = meta.vacation.as_ref().filter(|v| v.get("isEnabled").is_some_and(J::truthy));
    let audience: Option<&Audience> = meta.vacation_audience.as_ref();
    let forward: Option<&Forward> = meta.vacation_forward.as_ref().filter(|f| f.enabled);
    let rules: Vec<&Rule> = meta.rules.iter().collect();
    let mut lines: Vec<String> = Vec::new();

    lines.push(METADATA_BEGIN.to_string());
    lines.push(meta.to_json());
    lines.push(METADATA_END.to_string());
    lines.push(String::new());

    let use_mailbox_id = extensions.iter().any(|e| e == "mailboxid");
    let forward_period = forward.and_then(|f| period_tests(f.active_from.as_deref(), f.active_until.as_deref()));
    let forward_spam_guard = supports_spam_guard(extensions);
    let mut requires = bulwark_requires(meta, opts.include_vacation, extensions);
    requires.extend(opts.external_requires.iter().cloned());
    if !requires.is_empty() {
        lines
            .push(format!("require [{}];", requires.iter().map(|r| format!("\"{r}\"")).collect::<Vec<_>>().join(", ")));
    }

    if opts.include_vacation {
        lines.push(String::new());
        lines.push("# Vacation auto-reply".into());
        let include = format!("include :personal :optional \"{VACATION_SCRIPT_NAME}\";");
        match audience {
            Some(a) => {
                let domains = a.domains.iter().map(|d| quoted(d)).collect::<Vec<_>>().join(", ");
                let not = if a.only == "external" { "not " } else { "" };
                lines.push(format!("if {not}envelope :domain :is \"from\" [{domains}] {{"));
                lines.push(format!("    {include}"));
                lines.push("}".into());
            }
            None => lines.push(include),
        }
    }

    if let Some(v) = vacation {
        lines.push(String::new());
        lines.push("# Vacation auto-reply".into());
        let mut parts = Vec::new();
        if let Some(subject) = v.get("subject").and_then(J::as_str).filter(|x| !x.is_empty()) {
            parts.push(format!(":subject {}", quoted(subject)));
        }
        parts.push(quoted(v.get("textBody").and_then(J::as_str).unwrap_or("")));
        lines.push(format!("vacation {};", parts.join(" ")));
    }

    if let (Some(f), Some(period)) = (forward, &forward_period) {
        // Spam is not passed on: it stays here for the server's own handling.
        let mut tests: Vec<String> = period.clone();
        if forward_spam_guard {
            tests.push(SPAM_GUARD.to_string());
        }
        let test = match tests.len() {
            0 => "true".to_string(),
            1 => tests[0].clone(),
            _ => format!("allof({})", tests.join(", ")),
        };
        lines.push(String::new());
        lines.push(VACATION_FORWARD_MARKER.into());
        lines.push(format!("if {test} {{"));
        let copy = if f.keep_copy { ":copy " } else { "" };
        lines.push(format!("    redirect {copy}{};", quoted(&f.to)));
        if !f.keep_copy {
            lines.push("    stop;".into());
        }
        lines.push("}".into());
    }

    for rule in rules.iter().filter(|r| r.enabled) {
        if skip_reason(rule).is_some() {
            continue;
        }
        let period = period_tests(rule.active_from.as_deref(), rule.active_until.as_deref()).unwrap_or_default();
        lines.push(String::new());
        lines.push(format!("# Rule: {}", collapse_spaces(&rule.name)));

        let conditions: Vec<String> = rule.conditions.iter().filter_map(condition).collect();
        let mut test = match conditions.len() {
            0 => "true".to_string(),
            1 => conditions[0].clone(),
            _ => format!("{}({})", if rule.match_all { "allof" } else { "anyof" }, conditions.join(", ")),
        };
        if needs_spam_guard(rule, extensions) {
            test = if conditions.len() > 1 && rule.match_all {
                format!("allof({}, {SPAM_GUARD})", conditions.join(", "))
            } else {
                format!("allof({test}, {SPAM_GUARD})")
            };
        }
        // The period comes first: outside it, nothing else is evaluated.
        if !period.is_empty() {
            let mut all = period;
            all.push(test);
            test = format!("allof({})", all.join(", "));
        }
        let mut actions = action_lines(&rule.actions, use_mailbox_id);
        // discard and reject only cancel the implicit keep; "stop processing" always writes a
        // stop, unless the block has one already.
        if rule.stop_processing && !actions.iter().any(|a| a == "stop;") {
            actions.push("stop;".into());
        }
        lines.push(format!("if {test} {{"));
        lines.extend(actions.into_iter().map(|a| format!("    {a}")));
        lines.push("}".into());
    }

    let external = js_trim(external);
    if !external.is_empty() {
        lines.push(String::new());
        lines.push(EXTERNAL_HEADER.into());
        lines.push(external.to_string());
    }

    lines.push(String::new());
    lines.join("\n")
}

#[cfg(test)]
mod tests;
