//! Bulwark's rule model (`lib/jmap/sieve-types.ts`) and the metadata comment it lives in.
//!
//! Conditions and actions are kept as ordered JSON objects: what a client wrote, key by key,
//! comes back as it was, including keys this code does not know. A rule keeps the unknown keys
//! of its own object in `extra`.

use super::{
    json::{set, J},
    period::is_period_boundary,
};

pub const METADATA_BEGIN: &str = "/* @metadata:begin";
pub const METADATA_END: &str = "@metadata:end */";

#[derive(Debug, Clone, PartialEq)]
pub struct Rule {
    pub id: String,
    pub name: String,
    pub enabled: bool,
    /// `matchType` is "all" (true) or "any".
    pub match_all: bool,
    pub conditions: Vec<J>,
    pub actions: Vec<J>,
    pub stop_processing: bool,
    pub include_spam: bool,
    pub active_from: Option<String>,
    pub active_until: Option<String>,
    /// Keys of the rule's object that this code does not know, in their order.
    pub extra: Vec<(String, J)>,
}

const KNOWN: [&str; 10] = [
    "id",
    "name",
    "enabled",
    "matchType",
    "conditions",
    "actions",
    "stopProcessing",
    "includeSpam",
    "activeFrom",
    "activeUntil",
];

fn valid_condition(c: &J) -> bool {
    let (Some(_), Some(_)) = (c.get("field").and_then(J::as_str), c.get("comparator").and_then(J::as_str)) else {
        return false;
    };
    match c.get("value") {
        Some(J::Str(_)) => true,
        Some(J::Arr(items)) => items.iter().all(|v| matches!(v, J::Str(_))),
        _ => false,
    }
}

fn valid_action(a: &J) -> bool {
    a.get("type").and_then(J::as_str).is_some()
}

impl Rule {
    /// A rule from its JSON, or why it is not one. The checks are Bulwark's `isValidRule`.
    pub fn from_j(j: &J) -> Result<Rule, String> {
        let pairs = j.as_obj().ok_or("a rule is a JSON object")?;
        let field = |k: &str| j.get(k);
        let bad = |what: &str| format!("a rule has no valid {what}");
        let id = field("id").and_then(J::as_str).ok_or_else(|| bad("id"))?.to_string();
        let name = field("name").and_then(J::as_str).ok_or_else(|| bad("name"))?.to_string();
        let enabled = field("enabled").and_then(J::as_bool).ok_or_else(|| bad("enabled"))?;
        let match_all = match field("matchType").and_then(J::as_str) {
            Some("all") => true,
            Some("any") => false,
            _ => return Err(bad("matchType")),
        };
        let conditions = field("conditions").and_then(J::as_arr).ok_or_else(|| bad("conditions"))?.to_vec();
        let actions = field("actions").and_then(J::as_arr).ok_or_else(|| bad("actions"))?.to_vec();
        let stop_processing = field("stopProcessing").and_then(J::as_bool).ok_or_else(|| bad("stopProcessing"))?;
        let boundary = |k: &str| -> Result<Option<String>, String> {
            match field(k) {
                None => Ok(None),
                Some(J::Str(s)) if is_period_boundary(s) => Ok(Some(s.clone())),
                Some(_) => Err(bad(k)),
            }
        };
        let (active_from, active_until) = (boundary("activeFrom")?, boundary("activeUntil")?);
        if !conditions.iter().all(valid_condition) || !actions.iter().all(valid_action) {
            return Err("a rule has a condition or an action that is not valid".into());
        }
        // Bulwark writes `includeSpam` only as `true`; any other value is carried, unused.
        let include_spam = field("includeSpam").is_some_and(|v| v == &J::Bool(true));
        let mut extra: Vec<(String, J)> = pairs.iter().filter(|(k, _)| !KNOWN.contains(&k.as_str())).cloned().collect();
        if let Some(v) = field("includeSpam").filter(|v| *v != &J::Bool(true)) {
            extra.insert(0, ("includeSpam".to_string(), v.clone()));
        }
        Ok(Rule {
            id,
            name,
            enabled,
            match_all,
            conditions,
            actions,
            stop_processing,
            include_spam,
            active_from,
            active_until,
            extra,
        })
    }

    /// The rule as Bulwark's `stripRuleForMetadata` writes it.
    pub fn to_j(&self) -> J {
        let mut o = vec![
            ("id".to_string(), J::str(&self.id)),
            ("name".to_string(), J::str(&self.name)),
            ("enabled".to_string(), J::Bool(self.enabled)),
            ("matchType".to_string(), J::str(if self.match_all { "all" } else { "any" })),
            ("conditions".to_string(), J::Arr(self.conditions.clone())),
            ("actions".to_string(), J::Arr(self.actions.clone())),
            ("stopProcessing".to_string(), J::Bool(self.stop_processing)),
        ];
        if self.include_spam {
            o.push(("includeSpam".to_string(), J::Bool(true)));
        }
        if let Some(v) = &self.active_from {
            o.push(("activeFrom".to_string(), J::str(v)));
        }
        if let Some(v) = &self.active_until {
            o.push(("activeUntil".to_string(), J::str(v)));
        }
        for (k, v) in &self.extra {
            set(&mut o, k, v.clone());
        }
        J::Obj(o)
    }

    pub fn has_period(&self) -> bool {
        self.active_from.is_some() || self.active_until.is_some()
    }
}

// ---- conditions and actions ---------------------------------------------------------------

/// The string value of `key`, when it is one.
pub fn s<'a>(o: &'a J, key: &str) -> Option<&'a str> {
    o.get(key).and_then(J::as_str)
}

/// A condition's values as strings: `value` as one, or its array; non-strings are not there.
pub fn values_of(c: &J) -> Vec<String> {
    match c.get("value") {
        Some(J::Str(v)) => vec![v.clone()],
        Some(J::Arr(items)) => items.iter().filter_map(|v| v.as_str().map(str::to_string)).collect(),
        _ => Vec::new(),
    }
}

pub fn uses_regex(c: &J) -> bool {
    s(c, "comparator") == Some("regex")
        && matches!(s(c, "field"), Some("from" | "to" | "cc" | "subject" | "header" | "body"))
}

// ---- the vacation card's parts --------------------------------------------------------------

/// Forwarding from the vacation card.
#[derive(Debug, Clone, PartialEq)]
pub struct Forward {
    pub enabled: bool,
    pub to: String,
    pub keep_copy: bool,
    pub active_from: Option<String>,
    pub active_until: Option<String>,
}

/// Bulwark's `isValidEmail`.
pub fn is_valid_email(email: &str) -> bool {
    if email.is_empty() || email.chars().count() > 254 {
        return false;
    }
    if email.chars().any(|c| matches!(c, '\r' | '\n' | '\0' | '<' | '>')) {
        return false;
    }
    let Some((local, domain)) = email.split_once('@') else { return false };
    if local.is_empty() || domain.contains('@') {
        return false;
    }
    if !local.chars().all(|c| c.is_ascii_alphanumeric() || ".!#$%&'*+/=?^_`{|}~-".contains(c)) {
        return false;
    }
    domain.split('.').all(|label| {
        let b = label.as_bytes();
        !b.is_empty()
            && b.len() <= 63
            && b.iter().all(|c| c.is_ascii_alphanumeric() || *c == b'-')
            && b[0] != b'-'
            && b[b.len() - 1] != b'-'
    })
}

impl Forward {
    pub fn from_j(j: &J) -> Option<Forward> {
        j.as_obj()?;
        let enabled = j.get("enabled")?.as_bool()?;
        let keep_copy = j.get("keepCopy")?.as_bool()?;
        let to = j.get("to")?.as_str().filter(|t| is_valid_email(t))?.to_string();
        let boundary = |k: &str| match j.get(k) {
            None => Some(None),
            Some(J::Str(s)) if is_period_boundary(s) => Some(Some(s.clone())),
            Some(_) => None,
        };
        Some(Forward {
            enabled,
            to,
            keep_copy,
            active_from: boundary("activeFrom")?,
            active_until: boundary("activeUntil")?,
        })
    }

    pub fn to_j(&self) -> J {
        let mut o = vec![
            ("enabled".to_string(), J::Bool(self.enabled)),
            ("to".to_string(), J::str(&self.to)),
            ("keepCopy".to_string(), J::Bool(self.keep_copy)),
        ];
        if let Some(v) = &self.active_from {
            o.push(("activeFrom".to_string(), J::str(v)));
        }
        if let Some(v) = &self.active_until {
            o.push(("activeUntil".to_string(), J::str(v)));
        }
        J::Obj(o)
    }
}

/// Who gets the auto-reply: only senders from the own domains, or only the others.
#[derive(Debug, Clone, PartialEq)]
pub struct Audience {
    /// "internal" or "external".
    pub only: String,
    pub domains: Vec<String>,
}

fn is_domain(d: &str) -> bool {
    let labels: Vec<&str> = d.split('.').collect();
    labels.len() >= 2
        && labels.iter().all(|l| {
            let b = l.as_bytes();
            !b.is_empty()
                && b.iter().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'-')
                && b[0] != b'-'
                && b[b.len() - 1] != b'-'
        })
}

impl Audience {
    pub fn from_j(j: &J) -> Option<Audience> {
        let only = j.get("only")?.as_str().filter(|o| *o == "internal" || *o == "external")?.to_string();
        let items = j.get("domains")?.as_arr().filter(|d| !d.is_empty())?;
        let domains: Option<Vec<String>> =
            items.iter().map(|d| d.as_str().filter(|d| is_domain(d)).map(str::to_string)).collect();
        Some(Audience { only, domains: domains? })
    }

    pub fn to_j(&self) -> J {
        J::Obj(vec![
            ("only".to_string(), J::str(&self.only)),
            ("domains".to_string(), J::Arr(self.domains.iter().map(J::str).collect())),
        ])
    }
}

// ---- the metadata ---------------------------------------------------------------------------

/// What the comment at the top of the script holds.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Metadata {
    pub rules: Vec<Rule>,
    /// Bulwark's own auto-reply (`{isEnabled, subject, textBody}`), written into the script by
    /// Bulwark. Carried as it is.
    pub vacation: Option<J>,
    pub include_vacation: bool,
    pub vacation_forward: Option<Forward>,
    pub vacation_audience: Option<Audience>,
    /// Other keys of the top-level object, in their order.
    pub extra: Vec<(String, J)>,
}

impl Metadata {
    /// The metadata from the JSON between the markers; `None` when Bulwark would take the script
    /// as opaque (invalid JSON, a version other than 1 or 2, a rule it cannot read, forwarding or
    /// an audience it cannot read).
    pub fn from_json(text: &str) -> Option<Metadata> {
        let j = J::parse(text.trim())?;
        let pairs = j.as_obj()?;
        match j.get("version") {
            Some(J::Num(n)) if n.as_u64() == Some(1) || n.as_u64() == Some(2) => {}
            _ => return None,
        }
        let rules = j.get("rules")?.as_arr()?.iter().map(|r| Rule::from_j(r).ok()).collect::<Option<Vec<_>>>()?;
        let vacation_forward = match j.get("vacationForward") {
            None => None,
            Some(f) => Some(Forward::from_j(f)?),
        };
        let vacation_audience = match j.get("vacationAudience") {
            None => None,
            Some(a) => Some(Audience::from_j(a)?),
        };
        let known = ["version", "rules", "vacation", "includeVacation", "vacationForward", "vacationAudience"];
        Some(Metadata {
            rules,
            vacation: j.get("vacation").cloned(),
            include_vacation: j.get("includeVacation") == Some(&J::Bool(true)),
            vacation_forward,
            vacation_audience,
            extra: pairs.iter().filter(|(k, _)| !known.contains(&k.as_str())).cloned().collect(),
        })
    }

    /// Bulwark's `version`: 2 once a rule has a period or a regex, forwarding is on, or an
    /// audience is set; older builds leave such a script alone.
    pub fn version(&self) -> u8 {
        let two = self.rules.iter().any(Rule::has_period)
            || self.rules.iter().any(|r| r.conditions.iter().any(uses_regex))
            || self.vacation_forward.as_ref().is_some_and(|f| f.enabled)
            || self.vacation_audience.is_some();
        if two {
            2
        } else {
            1
        }
    }

    /// The metadata as one line of JSON, with `*/` written `*\/` so it cannot end the comment.
    pub fn to_json(&self) -> String {
        let mut o = vec![
            ("version".to_string(), J::Num(u64::from(self.version()).into())),
            ("rules".to_string(), J::Arr(self.rules.iter().map(Rule::to_j).collect())),
        ];
        if let Some(v) = self.vacation.as_ref().filter(|v| v.get("isEnabled").is_some_and(J::truthy)) {
            o.push(("vacation".to_string(), v.clone()));
        }
        if self.include_vacation {
            o.push(("includeVacation".to_string(), J::Bool(true)));
        }
        if let Some(f) = &self.vacation_forward {
            o.push(("vacationForward".to_string(), f.to_j()));
        }
        if let Some(a) = &self.vacation_audience {
            o.push(("vacationAudience".to_string(), a.to_j()));
        }
        for (k, v) in &self.extra {
            set(&mut o, k, v.clone());
        }
        J::Obj(o).to_json().replace("*/", "*\\/")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_rule_keeps_unknown_keys_and_the_order_of_its_conditions() {
        let text = r#"{"id":"1","name":"N","enabled":true,"matchType":"any","conditions":[{"comparator":"is","field":"from","value":["a","b"],"x":1}],"actions":[{"type":"move","value":"A","mailboxId":"m"}],"stopProcessing":false,"future":{"a":1}}"#;
        let rule = Rule::from_j(&J::parse(text).unwrap()).unwrap();
        assert_eq!(rule.to_j().to_json(), text);
    }

    #[test]
    fn a_rule_without_a_valid_part_is_refused() {
        for bad in [
            r#"{"id":1}"#,
            r#"{"id":"1","name":"N","enabled":true,"matchType":"some","conditions":[],"actions":[],"stopProcessing":false}"#,
            r#"{"id":"1","name":"N","enabled":true,"matchType":"all","conditions":[{"field":"from","comparator":"is"}],"actions":[],"stopProcessing":false}"#,
            r#"{"id":"1","name":"N","enabled":true,"matchType":"all","conditions":[],"actions":[],"stopProcessing":false,"activeFrom":"2026-10-05T08:00"}"#,
        ] {
            assert!(Rule::from_j(&J::parse(bad).unwrap()).is_err(), "{bad}");
        }
    }

    #[test]
    fn emails_and_domains_are_checked_as_bulwark_does() {
        assert!(is_valid_email("a.b+c@example.com"));
        assert!(!is_valid_email("a@b@c"));
        assert!(!is_valid_email("a b@example.com"));
        assert!(!is_valid_email("a@-x.com"));
        assert!(is_domain("example.co.uk"));
        assert!(!is_domain("localhost"));
        assert!(!is_domain("Example.com"));
    }

    #[test]
    fn the_version_is_two_for_a_period_or_a_regex() {
        let mut m = Metadata::default();
        assert_eq!(m.version(), 1);
        m.vacation_audience = Some(Audience { only: "internal".into(), domains: vec!["a.b".into()] });
        assert_eq!(m.version(), 2);
    }
}
