//! "Catch me up" (M12 stage 6): one run over the unread Inbox mail that came since the last time, in
//! two kinds of lines. What it sends is the sender, the subject, the date and the first lines (the
//! preview the cache keeps) of at most [`MOST_MAILS`] mails: no body, no attachment. The answer is
//! data read strictly: a line that does not fit is dropped, a number that is not one of the mails
//! sent is dropped, and no change is ever made by it.
//!
//! The grouping by sender and the link to each mail are made here from the cache, never taken from
//! the answer: the model only says, per numbered mail, whether it asks for something and in a
//! sentence what it is about.

use super::{
    prompts::{self, escape_cut, escape_head, flatten},
    request::{done, Prepared},
};
use crate::{compose::rules, model::MessageHeader, Error, Result};

/// The most mails one catch-up sends.
pub const MOST_MAILS: usize = 30;
/// Without an earlier catch-up the run goes back this far (seconds).
pub const FIRST_LOOK_BACK: i64 = 24 * 3600;

const FROM_CHARS: usize = 120;
const SUBJECT_CHARS: usize = 200;
const PREVIEW_CHARS: usize = 300;
/// The longest sentence kept per mail (characters).
const LINE_CHARS: usize = 200;

/// The mails a run sends and how many more were left out.
#[derive(Debug, Clone, PartialEq)]
pub struct Chosen {
    /// Newest first.
    pub mails: Vec<MessageHeader>,
    /// Unread mail since `since` beyond the limit.
    pub more: usize,
}

/// The unread mail that arrived after `since` (Unix seconds), newest first, at most [`MOST_MAILS`].
/// `inbox` is the Inbox's messages, newest first as the cache gives them.
pub fn choose(inbox: &[MessageHeader], since: i64) -> Chosen {
    let all: Vec<&MessageHeader> = inbox.iter().filter(|m| m.is_unread && m.received_at > since).collect();
    let more = all.len().saturating_sub(MOST_MAILS);
    Chosen { mails: all.into_iter().take(MOST_MAILS).cloned().collect(), more }
}

/// The request: the mails numbered from 1, in the order given, between `<mail>` tags.
pub fn build(mails: &[MessageHeader], today: &str, time_zone: &str) -> Result<Prepared> {
    if mails.is_empty() {
        return Err(Error::invalid("there is no unread mail since the last catch-up"));
    }
    let mut cut = false;
    let mut user = format!("{}\n\n<mail>\n", prompts::catchup_line(today, time_zone));
    for (i, m) in mails.iter().enumerate() {
        let from =
            rules::address_text(&crate::model::Address { name: m.from_name.clone(), email: m.from_email.clone() });
        let (from, c1) = escape_head(&flatten(&from), FROM_CHARS);
        let (subject, c2) = escape_head(&flatten(&m.subject), SUBJECT_CHARS);
        let (preview, c3) = escape_cut(flatten(&m.preview).trim(), PREVIEW_CHARS);
        cut |= c1 | c2 | c3;
        user.push_str(&format!(
            "<message>\n<number>{}</number>\n<from>{from}</from>\n<date>{}</date>\n<subject>{subject}</subject>\n<start>\n{preview}\n</start>\n</message>\n",
            i + 1,
            rules::iso_utc(m.received_at),
        ));
    }
    user.push_str("</mail>");
    Ok(done(prompts::catchup_system(), user, mails.len(), cut))
}

/// One line of the answer, for the mail with this number (counted from 1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    pub number: usize,
    pub needs_answer: bool,
    pub text: String,
}

/// The lines of an answer for `count` mails: `ITEM | number | ANSWER or INFO | sentence`. A number
/// that is not one of the mails, a second line for the same number, a kind that is neither word and
/// an empty sentence are dropped. An answer with no usable line is `Error::Temporary`.
pub fn parse(answer: &str, count: usize) -> Result<Vec<Line>> {
    let mut out: Vec<Line> = Vec::new();
    for line in answer.lines().map(str::trim).filter(|l| !l.is_empty()) {
        let line = line.trim_start_matches(['-', '*', '\u{2022}']).trim();
        let mut parts = line.splitn(4, '|').map(str::trim);
        if !parts.next().is_some_and(|w| w.eq_ignore_ascii_case("ITEM")) {
            continue;
        }
        let Some(number) = parts.next().and_then(|n| n.trim_matches(['#', '.']).parse::<usize>().ok()) else {
            continue;
        };
        let needs_answer = match parts.next().map(str::to_ascii_uppercase).as_deref() {
            Some("ANSWER") => true,
            Some("INFO") => false,
            _ => continue,
        };
        let text: String = flatten(parts.next().unwrap_or(""))
            .replace("&lt;", "<")
            .replace("&gt;", ">")
            .trim()
            .chars()
            .take(LINE_CHARS)
            .collect();
        if text.is_empty() || number == 0 || number > count || out.iter().any(|l| l.number == number) {
            continue;
        }
        out.push(Line { number, needs_answer, text });
    }
    match out.is_empty() {
        true => Err(Error::temporary("the answer could not be read")),
        false => Ok(out),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mail(id: &str, from: &str, unread: bool, at: i64) -> MessageHeader {
        MessageHeader {
            id: id.into(),
            thread_id: format!("t{id}"),
            mailbox_ids: vec!["inbox".into()],
            from_name: String::new(),
            from_email: from.into(),
            to: Vec::new(),
            subject: format!("Subject {id}"),
            preview: "first lines <b>of</b> it".into(),
            received_at: at,
            is_unread: unread,
            is_flagged: false,
            has_attachment: false,
            size: 1,
        }
    }

    #[test]
    fn only_unread_mail_after_the_last_time_is_chosen_newest_first_and_the_rest_is_counted() {
        let inbox: Vec<MessageHeader> = (0..40).map(|i| mail(&format!("m{i}"), "a@x.test", true, 1000 - i)).collect();
        let chosen = choose(&inbox, 100);
        assert_eq!(chosen.mails.len(), MOST_MAILS);
        assert_eq!(chosen.more, 10);
        assert_eq!(chosen.mails[0].id, "m0");
        let mixed = vec![
            mail("read", "a@x.test", false, 500),
            mail("old", "a@x.test", true, 50),
            mail("new", "a@x.test", true, 500),
        ];
        let chosen = choose(&mixed, 100);
        assert_eq!(chosen.mails.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(), ["new"]);
        assert_eq!(chosen.more, 0);
    }

    #[test]
    fn the_request_holds_the_sender_subject_and_first_lines_only_and_escapes_them() {
        let prepared =
            build(&[mail("1", "a@x.test", true, 1000), mail("2", "b@x.test", true, 900)], "2026-11-16, Monday", "UTC")
                .unwrap();
        assert_eq!(prepared.messages, 2);
        assert!(prepared.user.contains("<number>2</number>"));
        assert!(prepared.user.contains("first lines &lt;b&gt;of&lt;/b&gt; it"), "{}", prepared.user);
        assert!(prepared.user.contains("a@x.test") && prepared.user.contains("Subject 2"));
        assert!(!prepared.user.contains("<b>"));
        assert!(prepared.system.contains("DATA"), "the rules about data are in front");
        assert!(build(&[], "2026-11-16", "UTC").is_err());
    }

    #[test]
    fn lines_are_read_strictly() {
        let answer = "ITEM | 1 | ANSWER | Asks for the figures by Friday\n\
                      - ITEM | 2 | info | Newsletter about \u{2026}\n\
                      ITEM | 2 | ANSWER | a second line for 2 is dropped\n\
                      ITEM | 9 | INFO | not one of the mails\n\
                      ITEM | 3 | MAYBE | wrong kind\n\
                      ITEM | 3 | INFO |\n\
                      ITEM | x | INFO | no number\n\
                      Some preface the model should not have written";
        let lines = parse(answer, 3).unwrap();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0], Line { number: 1, needs_answer: true, text: "Asks for the figures by Friday".into() });
        assert!(!lines[1].needs_answer && lines[1].number == 2);
        assert!(matches!(parse("nothing usable", 3), Err(Error::Temporary { .. })));
        assert!(matches!(parse("ITEM | 4 | INFO | out of range", 3), Err(Error::Temporary { .. })));
    }

    #[test]
    fn a_long_sentence_is_cut_and_escaped_brackets_come_back() {
        let long = "x".repeat(500);
        let lines = parse(&format!("ITEM | 1 | INFO | a &lt;b&gt; {long}"), 1).unwrap();
        assert!(lines[0].text.starts_with("a <b> x") && lines[0].text.chars().count() == LINE_CHARS);
    }
}
