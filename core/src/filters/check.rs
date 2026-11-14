//! What a rule must be before it is written: every refusal of `Core::save_filters` that needs
//! no server, each naming the rule.

use std::collections::HashSet;

use crate::{
    model::{FilterField, FilterRule},
    Error, Result,
};

/// The most rules the app writes.
pub const MAX_RULES: usize = 200;
/// The longest value, in characters.
const MAX_VALUE: usize = 500;
/// The longest header name.
const MAX_HEADER_NAME: usize = 100;

/// The Sieve extensions a rule needs, in the order they are named in `require`.
pub fn needed_extensions(rule: &FilterRule) -> Vec<&'static str> {
    let mut out = Vec::new();
    if rule.move_to.is_some() || rule.delete {
        out.push("fileinto");
    }
    if rule.mark_read || rule.star {
        out.push("imap4flags");
    }
    out
}

/// How a rule is called in a message: its name, or its place in the list.
pub fn label(rule: &FilterRule, index: usize) -> String {
    match rule.name.trim() {
        "" => format!("Rule {}", index + 1),
        name => format!("The rule \"{name}\""),
    }
}

/// A header name is letters, digits and hyphens, and at least one.
pub fn is_header_name(name: &str) -> bool {
    !name.is_empty()
        && name.chars().count() <= MAX_HEADER_NAME
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

/// Refuses the first rule that cannot be written. `extensions` is what the server lists: a
/// rule that needs one the server does not have is refused, because the server would then
/// silently skip the whole script.
pub fn check_rules(rules: &[FilterRule], extensions: &[String]) -> Result<()> {
    if rules.len() > MAX_RULES {
        return Err(Error::invalid(format!("There are more than {MAX_RULES} rules; remove some.")));
    }
    let mut ids = HashSet::new();
    for (index, rule) in rules.iter().enumerate() {
        let who = label(rule, index);
        if !rule.id.is_empty() && !ids.insert(rule.id.as_str()) {
            return Err(Error::invalid(format!("{who} has the same id as another rule.")));
        }
        if rule.conditions.is_empty() {
            return Err(Error::invalid(format!("{who} has no condition.")));
        }
        for condition in &rule.conditions {
            if let FilterField::Header { name } = &condition.field {
                if !is_header_name(name) {
                    return Err(Error::invalid(format!(
                        "{who} looks at \"{name}\", which is not a header name (use letters, digits and hyphens)."
                    )));
                }
            }
            if condition.value.trim().is_empty() {
                return Err(Error::invalid(format!("{who} has a condition with nothing to look for.")));
            }
            if condition.value != condition.value.trim() {
                return Err(Error::invalid(format!(
                    "{who} has a value with a space at its start or end; the server ignores those, so the rule \
                     would never match. Remove the space."
                )));
            }
            if condition.value.chars().any(char::is_control) {
                return Err(Error::invalid(format!(
                    "{who} has a value with a line break or another control character; use one plain line."
                )));
            }
            if condition.value.chars().count() > MAX_VALUE {
                return Err(Error::invalid(format!("{who} has a value longer than {MAX_VALUE} characters.")));
            }
        }
        if rule.delete && rule.move_to.is_some() {
            return Err(Error::invalid(format!("{who} both moves the mail to a folder and deletes it; pick one.")));
        }
        if rule.move_to.is_none() && !rule.mark_read && !rule.star && !rule.delete && !rule.stop {
            return Err(Error::invalid(format!("{who} does nothing; choose what it should do.")));
        }
        for needed in needed_extensions(rule) {
            if !extensions.iter().any(|e| e == needed) {
                return Err(Error::invalid(format!(
                    "{who} needs the \"{needed}\" feature, which this server does not offer for filters."
                )));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{FilterCondition, FilterMatch};

    fn rule() -> FilterRule {
        FilterRule {
            id: String::new(),
            name: "News".into(),
            enabled: true,
            match_all: true,
            conditions: vec![FilterCondition {
                field: FilterField::Subject,
                how: FilterMatch::Contains,
                value: "news".into(),
            }],
            move_to: Some("mb2".into()),
            mark_read: false,
            star: false,
            delete: false,
            stop: false,
            folder_missing: false,
        }
    }

    fn all() -> Vec<String> {
        vec!["fileinto".into(), "imap4flags".into()]
    }

    fn refusal(rules: &[FilterRule]) -> String {
        match check_rules(rules, &all()) {
            Err(Error::Invalid { msg }) => msg,
            other => panic!("expected a refusal, got {other:?}"),
        }
    }

    #[test]
    fn a_good_rule_passes() {
        assert!(check_rules(&[rule()], &all()).is_ok());
        assert!(check_rules(&[], &[]).is_ok());
    }

    #[test]
    fn each_refusal_names_the_rule() {
        let mut r = rule();
        r.conditions.clear();
        assert_eq!(refusal(&[r]), "The rule \"News\" has no condition.");
        let mut r = rule();
        r.conditions[0].value = "  ".into();
        assert!(refusal(&[r]).contains("nothing to look for"));
        let mut r = rule();
        r.conditions[0].value = "a\nb".into();
        assert!(refusal(&[r]).contains("line break"));
        let mut r = rule();
        r.conditions[0].field = FilterField::Header { name: "X Mailer".into() };
        assert!(refusal(&[r]).contains("not a header name"));
        let mut r = rule();
        r.delete = true;
        assert!(refusal(&[r]).contains("pick one"));
        let mut r = rule();
        r.move_to = None;
        assert!(refusal(&[r]).contains("does nothing"));
        let mut r = rule();
        r.name = " ".into();
        r.move_to = None;
        assert!(refusal(&[r]).starts_with("Rule 1 "));
        let many: Vec<FilterRule> = (0..=MAX_RULES).map(|_| rule()).collect();
        assert!(refusal(&many).contains("200"));
        let mut a = rule();
        a.id = "x".into();
        assert!(refusal(&[a.clone(), a]).contains("same id"));
    }

    #[test]
    fn a_stop_alone_is_an_action_and_a_missing_extension_is_said_plainly() {
        let mut r = rule();
        r.move_to = None;
        r.stop = true;
        assert!(check_rules(&[r], &[]).is_ok());
        let msg = match check_rules(&[rule()], &["imap4flags".to_string()]) {
            Err(Error::Invalid { msg }) => msg,
            other => panic!("{other:?}"),
        };
        assert!(msg.contains("\"fileinto\"") && msg.contains("does not offer"));
    }

    #[test]
    fn a_value_with_a_space_at_its_edge_is_refused() {
        for value in ["boss@example.org ", " Invoice"] {
            let mut r = rule();
            r.conditions[0].value = value.into();
            let msg = refusal(&[r]);
            assert!(msg.contains("\"News\"") && msg.contains("space"), "{msg}");
        }
    }
}
