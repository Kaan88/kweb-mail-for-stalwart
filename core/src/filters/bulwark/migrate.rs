//! The rules of the app's old script (`kweb-filters`, M8) in Bulwark's model.

use super::{json::J, model::Rule};
use crate::{
    filters::{path_of, Entry, Path},
    model::{FilterField, FilterMatch, FilterRule, Mailbox},
};

fn obj(pairs: Vec<(&str, J)>) -> J {
    J::Obj(pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

fn condition(field: &str, comparator: &str, value: &str, header: Option<&str>) -> J {
    let mut pairs = vec![("field", J::str(field)), ("comparator", J::str(comparator)), ("value", J::str(value))];
    if let Some(h) = header {
        pairs.push(("headerName", J::str(h)));
    }
    obj(pairs)
}

fn comparator(how: FilterMatch, addresses: bool) -> &'static str {
    match how {
        FilterMatch::Contains => "contains",
        FilterMatch::Is if addresses => "address_is",
        FilterMatch::Is => "is",
        FilterMatch::BeginsWith => "starts_with",
        FilterMatch::EndsWith => "ends_with",
    }
}

/// One old rule as Bulwark's. The old script had no spam guard, so a rule that files mail
/// keeps doing so for spam too (`includeSpam`). A "To or Cc" test inside an "all" rule with
/// other conditions has no equivalent: the rule comes over switched off.
pub fn convert(rule: &FilterRule, boxes: &[Mailbox], fallback_id: &str) -> Rule {
    let mut enabled = rule.enabled;
    let mut match_all = rule.match_all;
    let recipients = rule.conditions.iter().filter(|c| c.field == FilterField::Recipient).count();
    if recipients > 0 && rule.conditions.len() == 1 {
        match_all = false;
    } else if recipients > 0 && match_all {
        enabled = false;
    }
    let mut conditions = Vec::new();
    for c in &rule.conditions {
        match &c.field {
            FilterField::From => conditions.push(condition("from", comparator(c.how, false), &c.value, None)),
            FilterField::Subject => conditions.push(condition("subject", comparator(c.how, false), &c.value, None)),
            FilterField::Header { name } => {
                conditions.push(condition("header", comparator(c.how, false), &c.value, Some(name)))
            }
            FilterField::Recipient => {
                for field in ["to", "cc"] {
                    conditions.push(condition(field, comparator(c.how, true), &c.value, None));
                }
                if enabled && match_all {
                    break;
                }
            }
        }
    }
    let mut actions = Vec::new();
    if rule.mark_read {
        actions.push(obj(vec![("type", J::str("mark_read"))]));
    }
    if rule.star {
        actions.push(obj(vec![("type", J::str("star"))]));
    }
    let target = match (&rule.move_to, rule.delete) {
        (Some(id), _) => Some(id.clone()),
        (None, true) => boxes.iter().find(|m| m.role.as_deref() == Some("trash")).map(|m| m.id.clone()),
        (None, false) => None,
    };
    let filed = target.is_some();
    if let Some(id) = target {
        if let Path::Found(path) = path_of(boxes, &id) {
            actions.push(obj(vec![("type", J::str("move")), ("value", J::str(path)), ("mailboxId", J::str(id))]));
        }
    }
    if actions.is_empty() || conditions.is_empty() {
        enabled = false;
    }
    let id = if rule.id.is_empty() { fallback_id.to_string() } else { rule.id.clone() };
    let mut name = rule.name.clone();
    if recipients > 0 && rule.enabled && !enabled {
        name.push_str(" (check the recipients)");
    }
    Rule {
        id,
        name,
        enabled,
        match_all,
        conditions,
        actions,
        stop_processing: rule.stop || rule.delete,
        include_spam: filed,
        active_from: None,
        active_until: None,
        extra: Vec::new(),
    }
}

/// All the old rules, in order.
pub fn convert_all(entries: &[Entry], boxes: &[Mailbox]) -> Vec<Rule> {
    entries.iter().enumerate().map(|(i, e)| convert(&e.rule, boxes, &format!("kweb-{}", i + 1))).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::FilterCondition;

    fn old(field: FilterField, how: FilterMatch) -> FilterRule {
        FilterRule {
            id: "o1".into(),
            name: "Old".into(),
            enabled: true,
            match_all: true,
            conditions: vec![FilterCondition { field, how, value: "x".into() }],
            move_to: Some("b1".into()),
            mark_read: true,
            star: false,
            delete: false,
            stop: true,
            folder_missing: false,
        }
    }

    fn boxes() -> Vec<Mailbox> {
        vec![Mailbox {
            id: "b1".into(),
            name: "News".into(),
            role: None,
            parent_id: None,
            sort_order: 0,
            total_emails: 0,
            unread_emails: 0,
            notify: false,
        }]
    }

    #[test]
    fn an_old_rule_becomes_a_bulwark_rule() {
        let r = convert(&old(FilterField::From, FilterMatch::BeginsWith), &boxes(), "k");
        assert_eq!(
            r.to_j().to_json(),
            r#"{"id":"o1","name":"Old","enabled":true,"matchType":"all","conditions":[{"field":"from","comparator":"starts_with","value":"x"}],"actions":[{"type":"mark_read"},{"type":"move","value":"News","mailboxId":"b1"}],"stopProcessing":true,"includeSpam":true}"#
        );
    }

    #[test]
    fn to_or_cc_becomes_two_conditions_matching_any() {
        let r = convert(&old(FilterField::Recipient, FilterMatch::Is), &boxes(), "k");
        assert!(!r.match_all && r.enabled && r.conditions.len() == 2);
        assert_eq!(r.conditions[1].get("comparator").and_then(J::as_str), Some("address_is"));
    }

    #[test]
    fn a_rule_that_cannot_be_carried_over_comes_over_off() {
        let mut rule = old(FilterField::Recipient, FilterMatch::Is);
        rule.conditions.push(FilterCondition { field: FilterField::From, how: FilterMatch::Is, value: "y".into() });
        let r = convert(&rule, &boxes(), "k");
        assert!(!r.enabled && r.name.ends_with("(check the recipients)"));
    }
}
