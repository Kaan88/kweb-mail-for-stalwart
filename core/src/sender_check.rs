//! The sender check (M12 stage 6, item 9): is the From address of a message the one it says it is?
//!
//! The answer comes from the `Authentication-Results` header the user's own server wrote when it
//! took the message in (RFC 8601): SPF, DKIM and DMARC as the server judged them. Nothing is
//! checked again here. Only that one header is believed; every other copy is the sender's own
//! writing, and a sender can write `dmarc=pass` as easily as anyone. What was probed on Stalwart
//! 1.1.0 is in `dev/notes/stalwart-sender-check.md`.
//!
//! Which header is the server's: the topmost one whose authserv-id (the first word) is the host
//! the topmost `Received` header says it was received `by`, with no header of the sender's own
//! (From, Date, Subject, ...) above it. A message that came from a local account and was never
//! through the server's checks has none, and is "not checked", not "failed".

use crate::model::{Address, MethodResult, SenderCheck, SenderReason, SenderStatus};

/// Headers of the message, in the order they stand, name and raw value.
pub type Headers = [(String, String)];

/// Names only the sender's mail program writes. One of them above an `Authentication-Results`
/// header means the header is not in the part the server wrote.
const SENDER_HEADERS: [&str; 8] =
    ["from", "to", "subject", "date", "message-id", "mime-version", "content-type", "reply-to"];

/// What one `method=result` entry of the header said.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Entry {
    method: String,
    result: String,
    /// `header.d`, `smtp.mailfrom` and the like, name lower-cased, value as written.
    props: Vec<(String, String)>,
}

impl Entry {
    fn prop(&self, name: &str) -> Option<&str> {
        self.props.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
    }
}

/// The sender check worked out from the message's headers (in their order) and its From address.
pub fn evaluate(headers: &Headers, from: &[Address]) -> SenderCheck {
    let from_domain = from.first().and_then(|a| domain_of(&a.email)).unwrap_or_default();
    let base = |status, reason| SenderCheck {
        status,
        reason,
        from_domain: from_domain.clone(),
        dmarc: MethodResult::NotRun,
        dkim: MethodResult::NotRun,
        spf: MethodResult::NotRun,
        signed_by: String::new(),
    };
    if from_domain.is_empty() {
        return base(SenderStatus::Unverified, SenderReason::NoSender);
    }
    let Some(header) = trusted_header(headers) else {
        return base(SenderStatus::NotChecked, SenderReason::NotFromThisServer);
    };
    let entries = parse_entries(&header);
    let mut out = base(SenderStatus::Unverified, SenderReason::NoResults);

    // DMARC: only a result about the From domain counts.
    let dmarc: Vec<&Entry> = entries
        .iter()
        .filter(|e| e.method == "dmarc")
        .filter(|e| e.prop("header.from").is_none_or(|d| aligned(&from_domain, d)))
        .collect();
    let dkim: Vec<&Entry> = entries.iter().filter(|e| e.method == "dkim").collect();
    let spf: Vec<&Entry> = entries.iter().filter(|e| e.method == "spf").collect();
    let dkim_domain =
        |e: &Entry| e.prop("header.d").map(str::to_string).or_else(|| e.prop("header.i").and_then(domain_of));
    let spf_domain = |e: &Entry| e.prop("smtp.mailfrom").map(|m| domain_of(m).unwrap_or_else(|| m.to_lowercase()));
    let dkim_aligned = |e: &Entry| dkim_domain(e).is_some_and(|d| aligned(&from_domain, &d));
    let spf_aligned = |e: &Entry| spf_domain(e).is_some_and(|d| aligned(&from_domain, &d));

    out.dmarc = summary(&dmarc, |_| true);
    out.dkim = summary(&dkim, dkim_aligned);
    out.spf = summary(&spf, spf_aligned);
    if let Some(e) = dkim.iter().find(|e| e.result == "pass" && dkim_aligned(e)) {
        out.signed_by = dkim_domain(e).unwrap_or_default().to_lowercase();
    }

    let (status, reason) = if out.dmarc == MethodResult::Pass {
        (SenderStatus::Verified, SenderReason::DmarcPass)
    } else if out.dmarc == MethodResult::Fail {
        (SenderStatus::Failed, SenderReason::DmarcFail)
    } else if out.dkim == MethodResult::Pass {
        (SenderStatus::Verified, SenderReason::DkimPass)
    } else if out.spf == MethodResult::Pass {
        (SenderStatus::Verified, SenderReason::SpfPass)
    } else if out.dkim == MethodResult::Fail {
        (SenderStatus::Failed, SenderReason::DkimFail)
    } else if out.spf == MethodResult::Fail {
        (SenderStatus::Failed, SenderReason::SpfFail)
    } else if dkim.iter().chain(spf.iter()).any(|e| e.result == "pass") {
        // Somebody vouches, but not for the domain the From line shows.
        (SenderStatus::Unverified, SenderReason::NotAligned)
    } else {
        (SenderStatus::Unverified, SenderReason::NoResults)
    };
    out.status = status;
    out.reason = reason;
    out
}

/// Pass when one entry passes (and counts), fail when one fails and none passes, else other;
/// not run when nothing of that method was there.
fn summary(entries: &[&Entry], counts: impl Fn(&Entry) -> bool) -> MethodResult {
    if entries.is_empty() {
        return MethodResult::NotRun;
    }
    if entries.iter().any(|e| e.result == "pass" && counts(e)) {
        MethodResult::Pass
    } else if entries.iter().any(|e| e.result == "fail" && counts(e)) {
        MethodResult::Fail
    } else {
        MethodResult::Other
    }
}

/// The value of the one `Authentication-Results` header the server wrote, or `None`.
fn trusted_header(headers: &Headers) -> Option<String> {
    let received = headers.iter().find(|(n, _)| n.eq_ignore_ascii_case("received"))?;
    let by = received_by(&received.1)?;
    let (index, (_, value)) =
        headers.iter().enumerate().find(|(_, (n, _))| n.eq_ignore_ascii_case("authentication-results"))?;
    if headers[..index].iter().any(|(n, _)| SENDER_HEADERS.iter().any(|s| n.eq_ignore_ascii_case(s))) {
        return None;
    }
    let value = unfold(value);
    let id = value.split(';').next()?.split_whitespace().next()?.trim_end_matches('.').to_lowercase();
    (id == by).then_some(value)
}

/// The host a `Received` header says the message was received by, lower case.
fn received_by(value: &str) -> Option<String> {
    let text = strip_comments(value);
    let mut words = text.split_whitespace();
    while let Some(word) = words.next() {
        if word.eq_ignore_ascii_case("by") {
            return words.next().map(|h| h.trim_end_matches('.').to_lowercase());
        }
    }
    None
}

fn parse_entries(value: &str) -> Vec<Entry> {
    let text = strip_comments(value);
    let mut parts = text.split(';');
    parts.next(); // the authserv-id (and a version)
    parts
        .filter_map(|part| {
            let mut words = part.split_whitespace();
            let first = words.next()?;
            let (method, result) = first.split_once('=')?;
            let method = method.split('/').next()?.to_lowercase();
            let mut props = Vec::new();
            for word in words {
                if let Some((k, v)) = word.split_once('=') {
                    if k.contains('.') {
                        props.push((k.to_lowercase(), v.trim_matches('"').to_string()));
                    }
                }
            }
            Some(Entry { method, result: result.to_lowercase(), props })
        })
        .collect()
}

/// Folding undone: line breaks and the white space around them become one space.
fn unfold(value: &str) -> String {
    value.split(['\r', '\n']).map(str::trim).filter(|l| !l.is_empty()).collect::<Vec<_>>().join(" ")
}

/// Comments in parentheses (they may nest and may hold a `;`) taken out.
fn strip_comments(value: &str) -> String {
    let (mut depth, mut out) = (0usize, String::with_capacity(value.len()));
    for c in unfold(value).chars() {
        match c {
            '(' => depth += 1,
            ')' if depth > 0 => depth -= 1,
            _ if depth == 0 => out.push(c),
            _ => {}
        }
    }
    out
}

/// The domain of an address: the part after the last `@`, lower case, no brackets or dots around.
fn domain_of(address: &str) -> Option<String> {
    let (_, domain) = address.trim().trim_matches(['<', '>']).rsplit_once('@')?;
    let domain = domain.trim().trim_matches('.').to_lowercase();
    (!domain.is_empty()).then_some(domain)
}

/// The two domains belong together: the same, or one under the other (a name with a dot in it,
/// so that `com` alone never vouches for anything). Relaxed alignment as DMARC does it, without
/// the public suffix list.
fn aligned(a: &str, b: &str) -> bool {
    let (a, b) = (a.trim().trim_matches('.').to_lowercase(), b.trim().trim_matches('.').to_lowercase());
    if a.is_empty() || b.is_empty() {
        return false;
    }
    a == b || (b.contains('.') && a.ends_with(&format!(".{b}"))) || (a.contains('.') && b.ends_with(&format!(".{a}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn h(list: &[(&str, &str)]) -> Vec<(String, String)> {
        list.iter().map(|(n, v)| (n.to_string(), v.to_string())).collect()
    }

    fn from(email: &str) -> Vec<Address> {
        vec![Address { name: String::new(), email: email.into() }]
    }

    const STALWART_RECEIVED: &str = " from mail.example.org (netlimiter.com [127.0.0.1])\r\n\tby localhost (Stalwart SMTP) with ESMTP id 4BB28FE69600400;\r\n\tMon, 16 Nov 2026 14:26:01 +0000";

    /// As Stalwart 1.1.0 wrote it for mail that failed everything (probed, `dev/notes/stalwart-sender-check.md`).
    const STALWART_FAIL: &str = " localhost;\r\n\tspf=none (localhost: no SPF records found for postmaster@mail.example.org) smtp.helo=mail.example.org;\r\n\tspf=fail (localhost: domain of someone@example.org does not designate 127.0.0.1 as permitted sender) smtp.mailfrom=someone@example.org;\r\n\tiprev=pass policy.iprev=127.0.0.1;\r\n\tdmarc=fail (policy not aligned) header.from=example.org policy.dmarc=reject";

    fn stalwart(value: &str) -> Vec<(String, String)> {
        h(&[
            ("Delivered-To", " tester@kweb.test"),
            ("Received", STALWART_RECEIVED),
            ("Authentication-Results", value),
            ("Received-SPF", " fail (x)"),
            ("Return-Path", " <someone@example.org>"),
            ("Date", " Mon, 16 Nov 2026 14:25:54 +0000"),
            ("From", " someone@example.org"),
        ])
    }

    #[test]
    fn a_dmarc_fail_from_the_servers_own_header_is_failed() {
        let c = evaluate(&stalwart(STALWART_FAIL), &from("someone@example.org"));
        assert_eq!((c.status, c.reason), (SenderStatus::Failed, SenderReason::DmarcFail));
        assert_eq!((c.dmarc, c.spf, c.dkim), (MethodResult::Fail, MethodResult::Fail, MethodResult::NotRun));
        assert_eq!(c.from_domain, "example.org");
    }

    #[test]
    fn a_header_the_sender_wrote_is_not_believed() {
        // Probed: the sender's own copy stands below the server's, after From and Date.
        let mut headers = stalwart(STALWART_FAIL);
        headers.push((
            "Authentication-Results".into(),
            " kweb.test; dkim=pass header.d=paypal.com; spf=pass;\r\n dmarc=pass header.from=paypal.com".into(),
        ));
        let c = evaluate(&headers, &from("service@paypal.com"));
        // The server's header says nothing about paypal.com: its dmarc result is for another domain.
        assert_ne!(c.status, SenderStatus::Verified);
        assert_eq!(c.reason, SenderReason::NoResults);
    }

    #[test]
    fn a_forged_header_alone_is_not_believed_even_with_the_servers_name() {
        // The sender's own header sits below the sender's From, even when it names the server.
        let headers = h(&[
            ("From", " a@paypal.com"),
            ("Authentication-Results", " localhost; dmarc=pass header.from=paypal.com"),
            ("Received", STALWART_RECEIVED),
        ]);
        assert_eq!(evaluate(&headers, &from("a@paypal.com")).status, SenderStatus::NotChecked);
        // No Received at all: nothing says who the server is.
        let headers = h(&[("Authentication-Results", " localhost; dmarc=pass header.from=paypal.com")]);
        assert_eq!(evaluate(&headers, &from("a@paypal.com")).reason, SenderReason::NotFromThisServer);
        // A name other than the receiving host's.
        let headers = h(&[
            ("Received", STALWART_RECEIVED),
            ("Authentication-Results", " mx.paypal.com; dmarc=pass header.from=paypal.com"),
        ]);
        assert_eq!(evaluate(&headers, &from("a@paypal.com")).status, SenderStatus::NotChecked);
    }

    #[test]
    fn gmail_form() {
        let ar = " mx.google.com;\r\n       dkim=pass header.i=@example.com header.s=sel header.b=abc123;\r\n       spf=pass (google.com: domain of bounce@example.com designates 1.2.3.4 as permitted sender) smtp.mailfrom=bounce@example.com;\r\n       dmarc=pass (p=REJECT sp=REJECT dis=NONE) header.from=example.com";
        // Gmail's own first Received names an internal host, not the authserv-id.
        let headers = h(&[
            ("Received", " by 2002:a05:6a00:1 with SMTP id q1csp1;\r\n        Mon, 16 Nov 2026 01:00:00 -0800 (PST)"),
            ("Authentication-Results", ar),
        ]);
        assert_eq!(evaluate(&headers, &from("news@example.com")).status, SenderStatus::NotChecked);
        // The same header as our server's (its name is the receiving host).
        let headers = h(&[
            (
                "Received",
                " from mail.example.com by mx.google.com with ESMTPS id x;\r\n Mon, 16 Nov 2026 01:00:00 -0800",
            ),
            ("Authentication-Results", ar),
        ]);
        let c = evaluate(&headers, &from("news@Example.com"));
        assert_eq!((c.status, c.reason), (SenderStatus::Verified, SenderReason::DmarcPass));
        assert_eq!((c.dkim, c.spf, c.signed_by.as_str()), (MethodResult::Pass, MethodResult::Pass, "example.com"));
    }

    #[test]
    fn outlook_form_with_no_authserv_id_is_not_ours() {
        let ar = "spf=pass (sender IP is 40.107.1.2) smtp.mailfrom=contoso.com; dkim=pass (signature was verified) header.d=contoso.com;dmarc=pass action=none header.from=contoso.com;compauth=pass reason=100";
        let headers = h(&[("Received", " from a by mx.local with SMTP"), ("Authentication-Results", ar)]);
        assert_eq!(evaluate(&headers, &from("x@contoso.com")).status, SenderStatus::NotChecked);
        // As the server's own, with a name in front, it is read well: the entries come after it.
        let entries = parse_entries(&format!("mx.local; {ar}"));
        let methods: Vec<&str> = entries.iter().map(|e| e.method.as_str()).collect();
        assert_eq!(methods, ["spf", "dkim", "dmarc", "compauth"]);
        assert_eq!(entries[1].prop("header.d"), Some("contoso.com"));
        let headers = h(&[
            ("Received", " from a by mx.local with SMTP"),
            ("Authentication-Results", &format!(" mx.local; {ar}")),
        ]);
        assert_eq!(evaluate(&headers, &from("x@contoso.com")).status, SenderStatus::Verified);
    }

    #[test]
    fn dkim_alone_for_the_from_domain_is_verified_and_for_another_it_is_not() {
        let ok = stalwart(
            " localhost; dkim=pass header.d=mail.example.org header.s=k;\r\n spf=softfail smtp.mailfrom=x@bulk.example",
        );
        let c = evaluate(&ok, &from("a@example.org"));
        assert_eq!((c.status, c.reason), (SenderStatus::Verified, SenderReason::DkimPass));
        assert_eq!(c.signed_by, "mail.example.org");
        let other =
            stalwart(" localhost; dkim=pass header.d=esp.example.net; spf=pass smtp.mailfrom=bounce@esp.example.net");
        let c = evaluate(&other, &from("a@example.org"));
        assert_eq!((c.status, c.reason), (SenderStatus::Unverified, SenderReason::NotAligned));
        let nothing = stalwart(" localhost; none");
        assert_eq!(evaluate(&nothing, &from("a@example.org")).reason, SenderReason::NoResults);
    }

    #[test]
    fn a_domain_with_no_records_is_unverified_not_failed() {
        // As Stalwart 1.1.0 wrote it for mail from a domain that has no SPF and no DMARC record.
        let ar = " localhost;\r\n\tspf=none (localhost: no SPF records found for postmaster@mail.nonexistent.invalid) smtp.helo=mail.nonexistent.invalid;\r\n\tspf=none (localhost: no SPF records found for someone@nonexistent.invalid) smtp.mailfrom=someone@nonexistent.invalid;\r\n\tiprev=pass policy.iprev=127.0.0.1;\r\n\tdmarc=none header.from=nonexistent.invalid policy.dmarc=none";
        let c = evaluate(&stalwart(ar), &from("someone@nonexistent.invalid"));
        assert_eq!((c.status, c.reason), (SenderStatus::Unverified, SenderReason::NoResults));
        assert_eq!((c.dmarc, c.spf, c.dkim), (MethodResult::Other, MethodResult::Other, MethodResult::NotRun));
    }

    #[test]
    fn a_comment_with_a_semicolon_and_a_failed_signature() {
        let ar = " localhost; dkim=fail (bad signature; body hash did not verify) header.d=example.org; spf=pass smtp.mailfrom=other.net";
        let c = evaluate(&stalwart(ar), &from("a@example.org"));
        assert_eq!((c.status, c.reason), (SenderStatus::Failed, SenderReason::DkimFail));
    }

    #[test]
    fn a_domain_is_not_aligned_with_a_bare_suffix() {
        assert!(aligned("mail.example.org", "example.org"));
        assert!(aligned("example.org", "EXAMPLE.org."));
        assert!(!aligned("evil.com", "com"));
        assert!(!aligned("paypal.com", "paypal.com.evil.net"));
        assert!(!aligned("a.com", ""));
    }

    #[test]
    fn no_from_address_is_unverified() {
        assert_eq!(evaluate(&stalwart(STALWART_FAIL), &[]).reason, SenderReason::NoSender);
    }
}
