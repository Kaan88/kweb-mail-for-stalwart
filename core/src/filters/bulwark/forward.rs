//! How many forwards one message can cause (`lib/filters/forward-limit.ts`). A server lets one
//! message trigger only `maxNumberRedirects` redirects and skips the rest; rules run in order
//! and a rule that stops ends the script for the messages it matches, so what counts is the
//! most forwards a single message can collect, not how many forwards there are.

use super::model::{s, Forward, Rule};

struct Run {
    forwards: usize,
    stops: bool,
}

fn rule_forwards(rule: &Rule) -> usize {
    let mut n = 0;
    for a in &rule.actions {
        match s(a, "type") {
            Some("stop") => break,
            Some("forward") => n += 1,
            _ => {}
        }
    }
    n
}

fn rule_stops(rule: &Rule) -> bool {
    rule.stop_processing || rule.actions.iter().any(|a| s(a, "type") == Some("stop"))
}

/// The most forwards one message can collect going through the enabled rules in order, with the
/// vacation card's forwarding ahead of them when it is on.
pub fn max_forwards(rules: &[Rule], vacation_forward: Option<&Forward>) -> usize {
    let mut runs: Vec<Run> = Vec::new();
    if let Some(f) = vacation_forward.filter(|f| f.enabled) {
        runs.push(Run { forwards: 1, stops: !f.keep_copy });
    }
    runs.extend(rules.iter().filter(|r| r.enabled).map(|r| Run { forwards: rule_forwards(r), stops: rule_stops(r) }));
    // From the rules so far that let the message go on.
    let (mut open, mut worst) = (0, 0);
    for run in runs {
        if run.stops {
            worst = worst.max(open + run.forwards);
        } else {
            open += run.forwards;
        }
    }
    worst.max(open)
}

#[cfg(test)]
mod tests {
    use super::{super::json::J, *};

    fn rule(actions: &[&str], stop: bool) -> Rule {
        let text = format!(
            r#"{{"id":"1","name":"n","enabled":true,"matchType":"all","conditions":[],"actions":[{}],"stopProcessing":{stop}}}"#,
            actions.iter().map(|t| format!(r#"{{"type":"{t}","value":"a@b.c"}}"#)).collect::<Vec<_>>().join(",")
        );
        Rule::from_j(&J::parse(&text).unwrap()).unwrap()
    }

    #[test]
    fn a_stop_ends_the_count_for_what_is_below() {
        assert_eq!(max_forwards(&[], None), 0);
        assert_eq!(max_forwards(&[rule(&["forward"], false), rule(&["forward"], false)], None), 2);
        // The first forwards and stops: the second never meets it.
        assert_eq!(max_forwards(&[rule(&["forward"], true), rule(&["forward"], false)], None), 1);
        // A forward behind a stop action does not run.
        assert_eq!(max_forwards(&[rule(&["stop", "forward"], false)], None), 0);
        let vf = Forward { enabled: true, to: "a@b.c".into(), keep_copy: true, active_from: None, active_until: None };
        assert_eq!(max_forwards(&[rule(&["forward"], false)], Some(&vf)), 2);
        let vf = Forward { keep_copy: false, ..vf };
        assert_eq!(max_forwards(&[rule(&["forward"], false)], Some(&vf)), 1);
    }
}
