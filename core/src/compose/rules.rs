//! The rules of writing a message that need no network and no cache: subjects, who a reply
//! goes to, which identity answers, the quoted text, the forward header, checking addresses and
//! matching what the user types against known people. All of it is pure, so it is tested here.

use std::collections::HashSet;

use crate::model::{Address, Draft, Identity};
use crate::{Error, Result};

/// The signature goes after this line (note the trailing space: it is what mail programs
/// recognise).
const SIGNATURE_SEPARATOR: &str = "-- ";

/// Whether `subject` begins with one of the `prefixes` (letters only, compared without case)
/// followed by a colon.
fn has_prefix(subject: &str, prefixes: &[&str]) -> bool {
    let subject = subject.trim_start();
    prefixes.iter().any(|p| {
        subject.len() > p.len()
            && subject.as_bytes()[..p.len()].eq_ignore_ascii_case(p.as_bytes())
            && subject.as_bytes()[p.len()] == b':'
    })
}

fn prefixed(prefix: &str, subject: &str) -> String {
    format!("{prefix} {subject}").trim_end().to_string()
}

/// "Re: " is added once: "re:", "RE:" and the like are left as they are.
pub fn reply_subject(subject: &str) -> String {
    if has_prefix(subject, &["re"]) {
        subject.to_string()
    } else {
        prefixed("Re:", subject)
    }
}

/// "Fwd: " is added once: "Fw:", "FWD:" and the like are left as they are.
pub fn forward_subject(subject: &str) -> String {
    if has_prefix(subject, &["fwd", "fw"]) {
        subject.to_string()
    } else {
        prefixed("Fwd:", subject)
    }
}

/// The key two spellings of one address share.
pub fn address_key(email: &str) -> String {
    email.trim().to_lowercase()
}

/// The addresses of the user's identities, as [`address_key`]s.
pub fn own_keys(identities: &[Identity]) -> HashSet<String> {
    identities.iter().map(|i| address_key(&i.email)).collect()
}

/// The first of each address (without regard to case) among `list` that is not in `skip`; the
/// ones taken are added to `skip`, so a later list leaves them out. Entries without an address
/// are dropped.
fn take_new(list: &[Address], skip: &mut HashSet<String>) -> Vec<Address> {
    let mut out = Vec::new();
    for a in list {
        let key = address_key(&a.email);
        if !key.is_empty() && skip.insert(key) {
            out.push(a.clone());
        }
    }
    out
}

/// Who an answer goes to, and who is copied.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Recipients {
    pub to: Vec<Address>,
    pub cc: Vec<Address>,
}

/// The recipients of a reply. `Reply` goes to the Reply-To address, else the sender. `ReplyAll`
/// adds the source's To (to the To list) and Cc (to the Cc list); in a reply to everyone the
/// user's own addresses (`own`) are left out and every address appears once.
///
/// A plain reply to a message the user wrote himself would go to himself: it goes to the
/// people that message was sent to instead. A note to oneself has nobody else on it: then the
/// reply does go to the sender, since a reply with no recipient at all helps nobody.
pub fn reply_recipients(
    all: bool,
    reply_to: &[Address],
    from: &Address,
    to: &[Address],
    cc: &[Address],
    own: &HashSet<String>,
) -> Recipients {
    let sender = std::slice::from_ref(from);
    let primary = if reply_to.is_empty() { sender } else { reply_to };
    if !all {
        let mut seen = HashSet::new();
        let targets = take_new(primary, &mut seen);
        if targets.iter().all(|a| own.contains(&address_key(&a.email))) {
            let others = take_new(to, &mut own.clone());
            if !others.is_empty() {
                return Recipients { to: others, cc: Vec::new() };
            }
        }
        return Recipients { to: targets, cc: Vec::new() };
    }
    let mut seen = own.clone();
    let main = take_new(primary, &mut seen);
    let more = take_new(to, &mut seen);
    let cc = take_new(cc, &mut seen);
    if main.is_empty() && more.is_empty() && cc.is_empty() {
        // Everyone on it is the user: still better addressed than empty.
        return Recipients { to: take_new(primary, &mut HashSet::new()), cc };
    }
    Recipients { to: [main, more].concat(), cc }
}

/// The identity a reply is sent as: the first one the source was sent to (in the order of its
/// recipients), else the first identity.
pub fn pick_identity<'a>(identities: &'a [Identity], source_recipients: &[Address]) -> Option<&'a Identity> {
    let sent_to = source_recipients.iter().find_map(|r| {
        let key = address_key(&r.email);
        identities.iter().find(|i| address_key(&i.email) == key)
    });
    sent_to.or_else(|| identities.first())
}

/// `Name <address>`, or the address alone when there is no name. Line breaks cannot get in.
pub fn address_text(a: &Address) -> String {
    let flat = |s: &str| s.replace(['\r', '\n'], " ");
    if a.name.trim().is_empty() {
        flat(&a.email)
    } else {
        format!("{} <{}>", flat(a.name.trim()), flat(&a.email))
    }
}

/// The text of a reply: room to write above, the line that says whose words follow, then those
/// words with every line marked.
pub fn quoted_reply(from: &Address, text: &str) -> String {
    let attribution = format!("{} wrote:", address_text(from));
    let text = text.replace("\r\n", "\n");
    let text = text.trim_end_matches('\n');
    if text.is_empty() {
        return format!("\n\n{attribution}");
    }
    let quoted: Vec<String> = text.lines().map(|line| format!("> {line}")).collect();
    format!("\n\n{attribution}\n{}", quoted.join("\n"))
}

/// The text of a forward: room to write above, the header of the original, then its text.
pub fn forwarded_text(from: &Address, sent_at: i64, subject: &str, to: &[Address], text: &str) -> String {
    let mut out = String::from("\n\n---------- Forwarded message ----------\n");
    out.push_str(&format!("From: {}\n", address_text(from)));
    out.push_str(&format!("Date: {}\n", iso_utc(sent_at)));
    out.push_str(&format!("Subject: {}\n", subject.replace(['\r', '\n'], " ")));
    if !to.is_empty() {
        let list: Vec<String> = to.iter().map(address_text).collect();
        out.push_str(&format!("To: {}\n", list.join(", ")));
    }
    out.push('\n');
    out.push_str(text.replace("\r\n", "\n").trim_end_matches('\n'));
    out
}

/// The signature as it stands in a message: a blank line, the "-- " line, the signature.
/// `None` for no signature.
fn signature_block(signature: &str) -> Option<String> {
    let signature = signature.trim_end();
    (!signature.trim().is_empty()).then(|| format!("\n\n{SIGNATURE_SEPARATOR}\n{signature}"))
}

/// Adds the signature below the text, after the "-- " line. No signature, no change.
pub fn with_signature(body: &str, signature: &str) -> String {
    format!("{body}{}", signature_block(signature).unwrap_or_default())
}

/// The text of a reply with the signature above the quoted original, so what the user writes
/// goes above the signature and the quote follows it.
pub fn reply_with_signature(quoted: &str, signature: &str) -> String {
    format!("{}{quoted}", signature_block(signature).unwrap_or_default())
}

/// The body of a draft whose sender changed from the one with signature `old` to the one with
/// `new`: the old signature, if it stands there untouched, is replaced by the new one. `None`
/// when there is nothing to swap: the user wrote in the signature, or took it out. A body that
/// is still empty gets the new signature when the old sender had none.
pub fn swap_signature(body: &str, old: &str, new: &str) -> Option<String> {
    let new_block = signature_block(new).unwrap_or_default();
    let Some(old_block) = signature_block(old) else {
        return body.trim().is_empty().then_some(new_block).filter(|block| !block.is_empty());
    };
    // The whole block, and the signature ends there: not the start of a longer one. The last
    // one: in a forward the draft's own signature ends the body, after the forwarded text (which
    // can carry the same block); in a reply it stands before the quote, whose lines start "> ".
    let at = body
        .match_indices(&old_block)
        .map(|(at, _)| at)
        .filter(|at| {
            let rest = &body[at + old_block.len()..];
            rest.is_empty() || rest.starts_with('\n')
        })
        .last()?;
    Some(format!("{}{new_block}{}", &body[..at], &body[at + old_block.len()..]))
}

/// `2026-11-14T09:30:00Z`.
pub fn iso_utc(secs: i64) -> String {
    let days = secs.div_euclid(86_400);
    let rest = secs.rem_euclid(86_400);
    // Days since 1970-01-01 to a calendar date (Howard Hinnant's civil_from_days).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z.rem_euclid(146_097);
    let year_of_era = (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let mp = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z", rest / 3600, rest % 3600 / 60, rest % 60)
}

// ---- HTML to text ----

/// Tags whose end is a line break.
const BLOCKS: [&str; 14] =
    ["p", "div", "li", "ul", "ol", "tr", "table", "blockquote", "pre", "h1", "h2", "h3", "h4", "section"];

/// A plain-text rendering of an HTML message, to quote when it has no text version. Script and
/// style content is dropped, `<br>` and the end of a block become line breaks, other tags are
/// removed, the common entities are decoded, and runs of blank lines shrink to one.
pub fn html_to_text(html: &str) -> String {
    let bytes = html.as_bytes();
    let starts = |at: usize, pattern: &str| {
        bytes.len() >= at + pattern.len() && bytes[at..at + pattern.len()].eq_ignore_ascii_case(pattern.as_bytes())
    };
    let mut out = String::new();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'<' if starts(i, "<!--") => i = find_ci(bytes, i + 4, b"-->").map_or(bytes.len(), |at| at + 3),
            b'<' if bytes.get(i + 1).is_some_and(|b| b.is_ascii_alphabetic() || matches!(b, b'/' | b'!' | b'?')) => {
                let end = tag_end(bytes, i);
                let closed = bytes[end - 1] == b'>';
                let tag = &html[i + 1..if closed { end - 1 } else { end }];
                i = end;
                let closing = tag.starts_with('/');
                let name: String = tag
                    .trim_start_matches('/')
                    .chars()
                    .take_while(char::is_ascii_alphanumeric)
                    .collect::<String>()
                    .to_ascii_lowercase();
                if !closing && (name == "script" || name == "style") && !tag.ends_with('/') {
                    // Everything up to the end tag is code, not text.
                    i = find_ci(bytes, i, format!("</{name}").as_bytes()).map_or(bytes.len(), |at| tag_end(bytes, at));
                } else if name == "br" || (closing && BLOCKS.contains(&name.as_str())) {
                    line_break(&mut out);
                }
            }
            b'&' => match entity(&html[i..]) {
                Some((c, used)) => {
                    push_char(&mut out, c);
                    i += used;
                }
                None => {
                    push_char(&mut out, '&');
                    i += 1;
                }
            },
            _ => {
                let c = html[i..].chars().next().expect("inside the string");
                push_char(&mut out, c);
                i += c.len_utf8();
            }
        }
    }
    tidy(&out)
}

/// Where the first `needle` at or after `from` starts, compared without regard to ASCII case.
fn find_ci(bytes: &[u8], from: usize, needle: &[u8]) -> Option<usize> {
    let last = bytes.len().checked_sub(needle.len())?;
    (from..=last).find(|&at| bytes[at..at + needle.len()].eq_ignore_ascii_case(needle))
}

/// The index after the `>` that closes the tag starting at `start`; quoted values may hold a `>`.
fn tag_end(bytes: &[u8], start: usize) -> usize {
    let mut quote = None;
    for (at, &b) in bytes.iter().enumerate().skip(start + 1) {
        match (quote, b) {
            (None, b'"' | b'\'') => quote = Some(b),
            (Some(q), _) if b == q => quote = None,
            (None, b'>') => return at + 1,
            _ => {}
        }
    }
    bytes.len()
}

/// Text is whitespace-collapsed as a browser does: runs become one space, none at a line start.
fn push_char(out: &mut String, c: char) {
    if c.is_whitespace() {
        if !out.is_empty() && !out.ends_with([' ', '\n']) {
            out.push(' ');
        }
    } else {
        out.push(c);
    }
}

fn line_break(out: &mut String) {
    let kept = out.trim_end_matches(' ').len();
    out.truncate(kept);
    out.push('\n');
}

/// The character an entity at the start of `text` stands for, and how many bytes it takes.
fn entity(text: &str) -> Option<(char, usize)> {
    let end = text.char_indices().take(12).find(|&(_, c)| c == ';')?.0;
    let name = &text[1..end];
    let c = match name {
        "amp" => '&',
        "lt" => '<',
        "gt" => '>',
        "quot" => '"',
        "apos" => '\'',
        "nbsp" => ' ',
        "ndash" => '\u{2013}',
        "mdash" => '\u{2014}',
        "hellip" => '\u{2026}',
        _ => {
            let digits = name.strip_prefix('#')?;
            let number = match digits.strip_prefix(['x', 'X']) {
                Some(hex) => u32::from_str_radix(hex, 16).ok()?,
                None => digits.parse().ok()?,
            };
            char::from_u32(number)?
        }
    };
    Some((c, end + 1))
}

/// Trims every line, drops blank lines at both ends and keeps at most one blank line in a row.
fn tidy(text: &str) -> String {
    let mut lines: Vec<&str> = Vec::new();
    for line in text.split('\n').map(str::trim) {
        if line.is_empty() && lines.last().is_none_or(|l| l.is_empty()) {
            continue;
        }
        lines.push(line);
    }
    while lines.last().is_some_and(|l| l.is_empty()) {
        lines.pop();
    }
    lines.join("\n")
}

// ---- addresses ----

/// Exactly one `@`, something before it and something after it, no spaces or control characters.
pub fn valid_address(email: &str) -> bool {
    if email.chars().any(|c| c.is_whitespace() || c.is_control()) || email.matches('@').count() != 1 {
        return false;
    }
    email.split_once('@').is_some_and(|(local, domain)| !local.is_empty() && !domain.is_empty())
}

/// A draft can be sent when it has a recipient and every address looks like one.
pub fn check_recipients(draft: &Draft) -> Result<()> {
    let all: Vec<&Address> = draft.to.iter().chain(&draft.cc).chain(&draft.bcc).collect();
    if all.is_empty() {
        return Err(Error::invalid("add at least one recipient"));
    }
    match all.iter().find(|a| !valid_address(&a.email)) {
        Some(bad) => Err(Error::invalid(format!("'{}' is not a valid address", bad.email.trim()))),
        None => Ok(()),
    }
}

// ---- suggestions ----

/// How well what the user typed (`needle`, lower case) fits a person: 0 when the name or the
/// address begins with it, 1 when a later word of the name does, `None` when it does not fit.
pub fn match_rank(needle: &str, name: &str, email: &str) -> Option<u8> {
    let (name, email) = (name.to_lowercase(), email.to_lowercase());
    if name.starts_with(needle) || email.starts_with(needle) {
        return Some(0);
    }
    let by_word = name.split(|c: char| !c.is_alphanumeric()).any(|word| !word.is_empty() && word.starts_with(needle));
    by_word.then_some(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn addr(name: &str, email: &str) -> Address {
        Address { name: name.into(), email: email.into() }
    }

    fn identity(id: &str, email: &str) -> Identity {
        Identity { id: id.into(), name: String::new(), email: email.into(), signature: String::new() }
    }

    fn emails(list: &[Address]) -> Vec<&str> {
        list.iter().map(|a| a.email.as_str()).collect()
    }

    #[test]
    fn re_is_added_once_whatever_its_case() {
        assert_eq!(reply_subject("Lunch"), "Re: Lunch");
        for already in ["Re: Lunch", "RE: Lunch", "re: Lunch", "  Re: Lunch", "Re:Lunch"] {
            assert_eq!(reply_subject(already), already);
        }
        // Not a prefix: a word that only starts with the letters.
        assert_eq!(reply_subject("Reminder"), "Re: Reminder");
        assert_eq!(reply_subject("Re"), "Re: Re");
        assert_eq!(reply_subject(""), "Re:");
        assert_eq!(reply_subject("Fwd: x"), "Re: Fwd: x");
    }

    #[test]
    fn fwd_is_added_once_and_fw_and_fwd_count() {
        assert_eq!(forward_subject("Lunch"), "Fwd: Lunch");
        for already in ["Fwd: Lunch", "FWD: Lunch", "fw: Lunch", "FW: Lunch", "Fw:Lunch"] {
            assert_eq!(forward_subject(already), already);
        }
        assert_eq!(forward_subject("Forward plan"), "Fwd: Forward plan");
        assert_eq!(forward_subject("Re: x"), "Fwd: Re: x");
        assert_eq!(forward_subject(""), "Fwd:");
    }

    #[test]
    fn reply_goes_to_reply_to_else_the_sender() {
        let own = own_keys(&[identity("i", "me@x.org")]);
        let from = addr("Ann", "ann@x.org");
        let got = reply_recipients(false, &[], &from, &[addr("", "me@x.org"), addr("", "bo@x.org")], &[], &own);
        assert_eq!(got, Recipients { to: vec![from.clone()], cc: Vec::new() });
        let list = addr("List", "list@x.org");
        let got = reply_recipients(false, std::slice::from_ref(&list), &from, &[], &[], &own);
        assert_eq!(got.to, vec![list]);
    }

    #[test]
    fn a_reply_to_ones_own_message_goes_to_its_recipients() {
        let own = own_keys(&[identity("i", "me@x.org")]);
        let from = addr("Me", "ME@x.org");
        let to = [addr("Bo", "bo@x.org"), addr("Me", "me@x.org"), addr("Bo again", "BO@x.org")];
        let got = reply_recipients(false, &[], &from, &to, &[], &own);
        assert_eq!(emails(&got.to), ["bo@x.org"]);
    }

    #[test]
    fn a_reply_to_a_note_to_oneself_still_has_a_recipient() {
        let me = addr("Me", "me@x.org");
        let own: HashSet<String> = [address_key("me@x.org")].into_iter().collect();
        for all in [false, true] {
            let r = reply_recipients(all, &[], &me, std::slice::from_ref(&me), &[], &own);
            assert_eq!(r.to, vec![me.clone()], "all = {all}");
            assert!(r.cc.is_empty());
        }
    }

    #[test]
    fn reply_all_adds_to_and_cc_without_own_addresses_each_address_once() {
        let own = own_keys(&[identity("1", "me@x.org"), identity("2", "Alias@x.org")]);
        let from = addr("Ann", "ann@x.org");
        let to = [addr("Me", "ME@x.org"), addr("Bo", "bo@x.org"), addr("Ann", "ANN@x.org")];
        let cc = [addr("", "alias@x.org"), addr("Cy", "cy@x.org"), addr("Bo", "bo@x.org"), addr("Di", "di@x.org")];
        let got = reply_recipients(true, &[], &from, &to, &cc, &own);
        assert_eq!(emails(&got.to), ["ann@x.org", "bo@x.org"]);
        assert_eq!(emails(&got.cc), ["cy@x.org", "di@x.org"]);
        // The name of the first mention is kept.
        assert_eq!(got.to[0].name, "Ann");
    }

    #[test]
    fn reply_all_to_ones_own_message_goes_to_the_original_recipients() {
        let own = own_keys(&[identity("1", "me@x.org")]);
        let got = reply_recipients(
            true,
            &[],
            &addr("Me", "me@x.org"),
            &[addr("Bo", "bo@x.org")],
            &[addr("Cy", "cy@x.org")],
            &own,
        );
        assert_eq!((emails(&got.to), emails(&got.cc)), (vec!["bo@x.org"], vec!["cy@x.org"]));
    }

    #[test]
    fn the_identity_is_the_one_the_source_was_sent_to_else_the_first() {
        let ids = [identity("1", "me@x.org"), identity("2", "work@x.org")];
        let pick = |to: &[Address]| pick_identity(&ids, to).map(|i| i.id.clone());
        assert_eq!(pick(&[addr("", "other@x.org"), addr("", "WORK@x.org")]), Some("2".into()));
        assert_eq!(pick(&[addr("", "other@x.org")]), Some("1".into()));
        assert_eq!(pick(&[]), Some("1".into()));
        assert_eq!(pick_identity(&[], &[addr("", "a@x.org")]), None);
        // The first recipient that is an identity wins, not the first identity.
        assert_eq!(pick(&[addr("", "work@x.org"), addr("", "me@x.org")]), Some("2".into()));
    }

    #[test]
    fn a_reply_quotes_every_line() {
        let text = quoted_reply(&addr("Ann", "ann@x.org"), "Hi\r\n\r\nSee you\n\n");
        assert_eq!(text, "\n\nAnn <ann@x.org> wrote:\n> Hi\n> \n> See you");
        assert_eq!(quoted_reply(&addr("", "ann@x.org"), "x"), "\n\nann@x.org wrote:\n> x");
        assert_eq!(quoted_reply(&addr("Ann", "ann@x.org"), ""), "\n\nAnn <ann@x.org> wrote:");
        // A name cannot add a line of its own.
        assert!(!quoted_reply(&addr("A\nB", "a@x.org"), "x").contains("A\nB"));
    }

    #[test]
    fn a_forward_has_the_header_block_then_the_text() {
        let text = forwarded_text(
            &addr("Ann", "ann@x.org"),
            1_767_323_045,
            "Plans",
            &[addr("Bo", "bo@x.org"), addr("", "cy@x.org")],
            "Hello\n",
        );
        assert_eq!(
            text,
            "\n\n---------- Forwarded message ----------\nFrom: Ann <ann@x.org>\nDate: 2026-01-02T03:04:05Z\n\
             Subject: Plans\nTo: Bo <bo@x.org>, cy@x.org\n\nHello"
        );
        assert!(!forwarded_text(&addr("", "a@x.org"), 0, "s", &[], "t").contains("To:"));
    }

    #[test]
    fn the_signature_follows_the_separator_line() {
        assert_eq!(with_signature("", "Ann\nAcme"), "\n\n-- \nAnn\nAcme");
        assert_eq!(with_signature("text", "Ann\n"), "text\n\n-- \nAnn");
        assert_eq!(with_signature("text", ""), "text");
        assert_eq!(with_signature("text", " \n"), "text");
    }

    #[test]
    fn a_reply_has_the_signature_above_the_quote() {
        let quote = "\n\nAnn wrote:\n> Hi";
        assert_eq!(reply_with_signature(quote, "Me\nAcme"), "\n\n-- \nMe\nAcme\n\nAnn wrote:\n> Hi");
        assert_eq!(reply_with_signature(quote, " "), quote);
    }

    #[test]
    fn a_changed_sender_swaps_a_signature_nobody_touched() {
        let body = with_signature("Hello", "Old\nCo");
        assert_eq!(swap_signature(&body, "Old\nCo", "New"), Some("Hello\n\n-- \nNew".to_string()));
        // A reply: the quote after the signature stays.
        let reply = reply_with_signature("\n\nAnn wrote:\n> Hi", "Old");
        assert_eq!(swap_signature(&reply, "Old", "New"), Some("\n\n-- \nNew\n\nAnn wrote:\n> Hi".to_string()));
        // The new sender has none: it goes away.
        assert_eq!(swap_signature(&body, "Old\nCo", ""), Some("Hello".to_string()));
        // Edited, or only the start of a longer one: left alone.
        assert_eq!(swap_signature("Hello\n\n-- \nOld\nCo and more", "Old\nCo", "New"), None);
        assert_eq!(swap_signature("Hello\n\n-- \nOlder", "Old", "New"), None);
        assert_eq!(swap_signature("Hello", "Old", "New"), None);
        // No old signature: an empty body gets the new one, a written one does not.
        assert_eq!(swap_signature("", "", "New"), Some("\n\n-- \nNew".to_string()));
        assert_eq!(swap_signature("Hello", "", "New"), None);
        assert_eq!(swap_signature("", "", ""), None);
    }

    #[test]
    fn the_signature_of_a_forward_is_swapped_at_the_end_not_inside_the_forwarded_text() {
        let body = "Hi\n\n-- \nOld\n\n-- \nOld";
        assert_eq!(swap_signature(body, "Old", "New").as_deref(), Some("Hi\n\n-- \nOld\n\n-- \nNew"));
        // A reply: the signature stands before the quote.
        let reply = "\n\n-- \nOld\n\nOn Monday Bo wrote:\n> hi";
        assert_eq!(swap_signature(reply, "Old", "New").as_deref(), Some("\n\n-- \nNew\n\nOn Monday Bo wrote:\n> hi"));
    }

    #[test]
    fn dates_are_written_as_utc() {
        assert_eq!(iso_utc(0), "1970-01-01T00:00:00Z");
        assert_eq!(iso_utc(951_782_400), "2000-02-29T00:00:00Z");
        assert_eq!(iso_utc(1_767_323_045), "2026-01-02T03:04:05Z");
        assert_eq!(iso_utc(-1), "1969-12-31T23:59:59Z");
    }

    #[test]
    fn html_becomes_plain_text() {
        let html = "<html><head><style>p { color: red }</style><script>alert('x > y')</script></head>\n\
            <body><p>Hello&nbsp;<b>world</b> &amp; &lt;friends&gt;</p>\n<p>Second<br>line</p><br><br><br>\n\
            <div>Tom&#39;s &#x41;</div><!-- hidden --><a href=\"x?a>b\">link</a></body></html>";
        assert_eq!(html_to_text(html), "Hello world & <friends>\nSecond\nline\n\nTom's A\nlink");
    }

    #[test]
    fn html_edge_cases_do_not_lose_or_break_text() {
        assert_eq!(html_to_text(""), "");
        assert_eq!(html_to_text("a < b && c"), "a < b && c");
        assert_eq!(html_to_text("<p>unclosed <b>tag"), "unclosed tag");
        assert_eq!(html_to_text("<SCRIPT>x</SCRIPT>kept<Style>y"), "kept");
        assert_eq!(html_to_text("caf\u{e9} &unknown; &#xZZ; &"), "caf\u{e9} &unknown; &#xZZ; &");
        assert_eq!(html_to_text("<li>one</li><li>two</li>"), "one\ntwo");
        assert_eq!(html_to_text("a<br/>b<br />c"), "a\nb\nc");
    }

    #[test]
    fn addresses_need_one_at_sign_and_both_sides() {
        for ok in ["a@b", "ann@example.org", "a.b+c@d.e.f"] {
            assert!(valid_address(ok), "{ok}");
        }
        for bad in ["", "a", "@b", "a@", "a@@b", "a@b@c", "a b@c", "a@b c", " a@b", "a@b\n", "a@b\u{0}"] {
            assert!(!valid_address(bad), "{bad:?}");
        }
    }

    #[test]
    fn a_draft_needs_a_recipient_and_good_addresses() {
        let mut d = Draft {
            id: "d".into(),
            identity_id: "i".into(),
            to: Vec::new(),
            cc: Vec::new(),
            bcc: Vec::new(),
            subject: String::new(),
            body: String::new(),
            attachments: Vec::new(),
            source_message_id: None,
            is_forward: false,
            server_id: None,
            send_error: None,
            updated_at: 0,
        };
        assert!(matches!(check_recipients(&d), Err(Error::Invalid { .. })));
        d.bcc = vec![addr("", "hidden@x.org")];
        assert!(check_recipients(&d).is_ok());
        d.cc = vec![addr("", "nope")];
        let message = check_recipients(&d).unwrap_err().to_string();
        assert!(message.contains("nope"), "{message}");
    }

    #[test]
    fn matches_begin_a_name_a_word_of_it_or_an_address() {
        assert_eq!(match_rank("ann", "Ann Keller", "x@y"), Some(0));
        assert_eq!(match_rank("kel", "Ann Keller", "x@y"), Some(1));
        assert_eq!(match_rank("ann", "", "ANN@x.org"), Some(0));
        assert_eq!(match_rank("ller", "Ann Keller", "x@y"), None);
        assert_eq!(match_rank("x.org", "Ann", "ann@x.org"), None);
        assert_eq!(match_rank("\u{e7}a", "\u{c7}a\u{11f}lar Demir", "c@y"), Some(0));
        assert_eq!(match_rank("de", "\u{c7}a\u{11f}lar Demir", "c@y"), Some(1));
    }
}
