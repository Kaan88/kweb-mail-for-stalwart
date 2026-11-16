//! What a rule must be before it is written: every refusal that needs no server, each naming
//! the rule. The server's own check of the script comes after.

use std::collections::HashSet;

use super::{
    generate::{is_valid_sieve_header_name, js_trim},
    model::{is_valid_email, s, values_of, Rule},
    period::utc_parts,
};

/// The most rules the app writes.
pub const MAX_RULES: usize = 500;
const MAX_NAME: usize = 200;

fn label(rule: &Rule, index: usize) -> String {
    match js_trim(&rule.name) {
        "" => format!("Rule {}", index + 1),
        name => format!("The rule \"{name}\""),
    }
}

fn comparators(field: &str) -> &'static [&'static str] {
    match field {
        "from" | "to" | "cc" => &[
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
        ],
        "subject" | "header" | "body" => {
            &["contains", "not_contains", "is", "not_is", "starts_with", "ends_with", "matches", "regex"]
        }
        "size" => &["greater_than", "less_than"],
        "attachment" => &["has_any", "has_type"],
        _ => &[],
    }
}

fn is_size(v: &str) -> bool {
    let digits = v.trim_end_matches(['K', 'M', 'G', 'k', 'm', 'g']);
    v.len() - digits.len() <= 1 && !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit())
}

/// Refuses the first rule that cannot be written. `extensions` is what the server lists (empty:
/// not known, nothing is gated on it).
pub fn check_rules(rules: &[Rule], extensions: &[String]) -> Result<(), String> {
    if rules.len() > MAX_RULES {
        return Err(format!("There are more than {MAX_RULES} rules; remove some."));
    }
    let has = |e: &str| extensions.is_empty() || extensions.iter().any(|x| x == e);
    let mut ids = HashSet::new();
    for (index, rule) in rules.iter().enumerate() {
        let who = label(rule, index);
        if rule.id.is_empty() || !ids.insert(rule.id.as_str()) {
            return Err(format!("{who} has no id of its own."));
        }
        if js_trim(&rule.name).is_empty() {
            return Err(format!("{who} needs a name."));
        }
        if rule.name.chars().count() > MAX_NAME {
            return Err(format!("{who} has a name longer than {MAX_NAME} characters."));
        }
        if rule.conditions.is_empty() {
            return Err(format!("{who} has no condition."));
        }
        if rule.actions.is_empty() {
            return Err(format!("{who} has no action."));
        }
        for c in &rule.conditions {
            let field = s(c, "field").unwrap_or("");
            let comparator = s(c, "comparator").unwrap_or("");
            if field == "all" {
                continue;
            }
            let allowed = comparators(field);
            if allowed.is_empty() {
                return Err(format!("{who} has a condition on \"{field}\", which this app does not know."));
            }
            if !allowed.contains(&comparator) {
                return Err(format!("{who} cannot test {field} with \"{comparator}\"."));
            }
            if comparator == "regex" && !has("regex") {
                return Err(format!("{who} uses a regular expression, which this server does not support."));
            }
            if field == "body" && !has("body") {
                return Err(format!("{who} looks into the message text, which this server does not support."));
            }
            if field == "attachment" && !has("mime") {
                return Err(format!("{who} tests attachments, which this server does not support."));
            }
            if field == "header" && !is_valid_sieve_header_name(js_trim(s(c, "headerName").unwrap_or(""))) {
                return Err(format!("{who} names a header with something other than letters, digits, - and _ in it."));
            }
            if comparator == "has_any" {
                continue;
            }
            let values = values_of(c);
            if values.iter().all(|v| js_trim(v).is_empty()) {
                return Err(format!("{who} has a condition without a value."));
            }
            if field == "size" && !values.first().is_some_and(|v| is_size(js_trim(v))) {
                return Err(format!("{who} has a size that is not a number (with K, M or G)."));
            }
        }
        for a in &rule.actions {
            let value = js_trim(s(a, "value").unwrap_or(""));
            match s(a, "type").unwrap_or("") {
                "move" | "copy" if value.is_empty() => return Err(format!("{who} has no folder to put mail in.")),
                "forward" if !is_valid_email(value) => {
                    return Err(format!("{who} forwards to \"{value}\", which is not an address."))
                }
                "reject" if value.is_empty() => return Err(format!("{who} has no text for rejecting.")),
                "add_label" if value.is_empty() => return Err(format!("{who} has no label.")),
                "move" | "copy" | "forward" | "reject" | "add_label" | "mark_read" | "star" | "discard" | "keep"
                | "stop" => {}
                other => return Err(format!("{who} has an action \"{other}\", which this app does not know.")),
            }
        }
        if let (Some(from), Some(until)) = (&rule.active_from, &rule.active_until) {
            if utc_parts(from) > utc_parts(until) {
                return Err(format!("{who} ends before it starts."));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::filters::bulwark::json::J;

    fn rule(text: &str) -> Rule {
        Rule::from_j(&J::parse(text).unwrap()).unwrap()
    }

    const OK: &str = r#"{"id":"1","name":"N","enabled":true,"matchType":"all","conditions":[{"field":"from","comparator":"contains","value":"a"}],"actions":[{"type":"move","value":"A"}],"stopProcessing":false}"#;

    #[test]
    fn a_good_rule_passes_and_each_fault_is_named() {
        assert!(check_rules(&[rule(OK)], &[]).is_ok());
        let cases = [
            (OK.replace("\"name\":\"N\"", "\"name\":\" \""), "needs a name"),
            (OK.replace("\"value\":\"a\"", "\"value\":\"\""), "without a value"),
            (OK.replace("\"value\":\"A\"", "\"value\":\"\""), "no folder"),
            (OK.replace("contains", "has_type"), "cannot test"),
            (OK.replace("\"move\",\"value\":\"A\"", "\"forward\",\"value\":\"nope\""), "not an address"),
            (OK.replace("\"conditions\":[{\"field\":\"from\",\"comparator\":\"contains\",\"value\":\"a\"}]", "\"conditions\":[]"), "no condition"),
            (OK.replace("\"field\":\"from\"", "\"field\":\"header\",\"headerName\":\"X Y\""), "header"),
            (OK.replace("\"stopProcessing\":false", "\"stopProcessing\":false,\"activeFrom\":\"2026-10-06T00:00:00Z\",\"activeUntil\":\"2026-10-05T00:00:00Z\""), "ends before"),
        ];
        for (text, word) in cases {
            let err = check_rules(&[rule(&text)], &[]).unwrap_err();
            assert!(err.contains(word), "{err}");
        }
        let regex = OK.replace("contains", "regex");
        assert!(check_rules(&[rule(&regex)], &["fileinto".to_string()]).unwrap_err().contains("regular expression"));
        assert!(check_rules(&[rule(OK), rule(OK)], &[]).unwrap_err().contains("own"));
    }
}
