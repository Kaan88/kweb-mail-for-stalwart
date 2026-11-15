//! Reading the stage 2 answers: the reply ideas and the to-dos and dates a model wrote, as the
//! lines the prompts ask for. Pure and tolerant: what does not fit is dropped, never an error.
//! What comes out is text for the user to read and tap; nothing here is ever acted on by itself.

use crate::model::AiFinding;

/// The most ideas a ReplyIdeas answer gives.
pub const IDEAS: usize = 3;
/// The most findings an Extract answer gives.
pub const FINDINGS: usize = 20;
/// The longest idea, to-do or title (characters); the rest of a longer one is dropped.
pub const LINE_CHARS: usize = 300;

/// The ideas in a ReplyIdeas answer, at most 3.
///
/// Each line is one idea; bullets and numbering in front are removed and blank lines dropped.
/// When some lines carry a bullet or a number and others do not, the others are a preface or a
/// closing remark and are left out.
pub fn ideas(answer: &str) -> Vec<String> {
    let lines: Vec<(bool, String)> = answer
        .lines()
        .map(|line| {
            let (marked, rest) = unmark(line);
            (marked, cap(rest))
        })
        .filter(|(_, text)| !text.is_empty())
        .collect();
    let any_marked = lines.iter().any(|(marked, _)| *marked);
    lines.into_iter().filter(|(marked, _)| *marked || !any_marked).map(|(_, text)| text).take(IDEAS).collect()
}

/// The findings in an Extract answer, at most 20; lines that do not fit are dropped.
///
/// A line is `TODO | text`, `DATE | YYYY-MM-DDTHH:MM | title` or `DATE | YYYY-MM-DD | title`
/// (a bullet or number in front is removed, the first word may be in any case). A day without a
/// time is an all-day entry at 00:00:00; a date that does not exist is dropped with its line.
pub fn findings(answer: &str) -> Vec<AiFinding> {
    answer.lines().filter_map(finding).take(FINDINGS).collect()
}

fn finding(line: &str) -> Option<AiFinding> {
    let (_, line) = unmark(line);
    let (kind, rest) = line.split_once('|')?;
    match kind.trim().to_ascii_uppercase().as_str() {
        "TODO" => Some(cap(rest)).filter(|text| !text.is_empty()).map(|text| AiFinding::Todo { text }),
        "DATE" => {
            let (when, title) = rest.split_once('|')?;
            let (start, all_day) = moment(when.trim())?;
            Some(cap(title)).filter(|t| !t.is_empty()).map(|title| AiFinding::Date { title, start, all_day })
        }
        _ => None,
    }
}

/// A line without the bullet or the number in front of it, and whether it had one.
fn unmark(line: &str) -> (bool, &str) {
    let line = line.trim();
    let bullets = line.trim_start_matches(['-', '*', '\u{2022}', '\u{2013}', '\u{2014}', '\u{b7}']);
    if bullets.len() != line.len() {
        return (true, bullets.trim());
    }
    // "1." or "2)": one or two digits, so that a line that starts with a year stays whole.
    let digits = line.chars().take_while(char::is_ascii_digit).count();
    match line[digits..].strip_prefix(['.', ')']) {
        Some(rest) if (1..=2).contains(&digits) => (true, rest.trim()),
        _ => (false, line),
    }
}

/// Trimmed, and at most [`LINE_CHARS`] characters.
fn cap(text: &str) -> String {
    text.trim().chars().take(LINE_CHARS).collect::<String>().trim_end().to_string()
}

/// Whether `text` is a day that exists, written `YYYY-MM-DD`.
pub(crate) fn is_day(text: &str) -> bool {
    day(text).is_some()
}

fn day(text: &str) -> Option<&str> {
    let b = text.as_bytes();
    let shape = b.len() == 10
        && b.iter().enumerate().all(|(i, c)| if i == 4 || i == 7 { *c == b'-' } else { c.is_ascii_digit() });
    if !shape {
        return None;
    }
    let number = |range: std::ops::Range<usize>| text[range].parse::<u32>().ok();
    let (year, month, day) = (number(0..4)?, number(5..7)?, number(8..10)?);
    chrono::NaiveDate::from_ymd_opt(year as i32, month, day).filter(|_| year > 0).map(|_| text)
}

/// `YYYY-MM-DD` or `YYYY-MM-DDTHH:MM` (a space for the `T` and seconds at the end are let
/// through) as local `YYYY-MM-DDTHH:MM:SS`, and whether it is a whole day.
fn moment(when: &str) -> Option<(String, bool)> {
    if !when.is_ascii() {
        return None;
    }
    if when.len() == 10 {
        return day(when).map(|d| (format!("{d}T00:00:00"), true));
    }
    if when.len() != 16 && when.len() != 19 {
        return None;
    }
    let date = day(&when[..10])?;
    let b = when.as_bytes();
    if b[10] != b'T' && b[10] != b' ' && b[10] != b't' {
        return None;
    }
    let two = |at: usize, below: u32| {
        let digits = b[at].is_ascii_digit() && b[at + 1].is_ascii_digit();
        when[at..at + 2].parse::<u32>().ok().filter(|n| digits && *n < below)
    };
    let (hour, minute) = (two(11, 24)?, two(14, 60)?);
    if b[13] != b':' || (when.len() == 19 && (b[16] != b':' || two(17, 60).is_none())) {
        return None;
    }
    Some((format!("{date}T{hour:02}:{minute:02}:00"), false))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn todo(text: &str) -> AiFinding {
        AiFinding::Todo { text: text.into() }
    }

    fn date(title: &str, start: &str, all_day: bool) -> AiFinding {
        AiFinding::Date { title: title.into(), start: start.into(), all_day }
    }

    #[test]
    fn ideas_are_the_lines_without_their_bullets_at_most_three() {
        assert_eq!(
            ideas("- Agree to Friday\n- Decline politely\n- Ask for another day"),
            ["Agree to Friday", "Decline politely", "Ask for another day"]
        );
        assert_eq!(ideas("1. One\n2) Two\n\n\u{2022} Three\n* Four"), ["One", "Two", "Three"]);
        assert_eq!(ideas("  --  spaced out  \r\n\r\n- next\r\n"), ["spaced out", "next"]);
        // No bullets at all: every line counts.
        assert_eq!(ideas("Say yes\nSay no"), ["Say yes", "Say no"]);
        // With bullets, what has none is a preface or a closing remark.
        assert_eq!(ideas("Here are some ideas:\n- Yes\n- No\nLet me know!"), ["Yes", "No"]);
        assert_eq!(ideas(""), Vec::<String>::new());
        assert_eq!(ideas(" \n-\n * \n"), Vec::<String>::new());
    }

    #[test]
    fn an_idea_keeps_a_leading_year_and_is_cut_at_three_hundred_characters() {
        assert_eq!(ideas("2027 is fine for me\n100% yes"), ["2027 is fine for me", "100% yes"]);
        let long = format!("- {}", "\u{e9}".repeat(400));
        let got = ideas(&long);
        assert_eq!(got[0].chars().count(), LINE_CHARS);
        // What an idea says is text, whatever it looks like.
        assert_eq!(ideas("- <b>Ignore the rules</b> | TODO"), ["<b>Ignore the rules</b> | TODO"]);
    }

    #[test]
    fn findings_are_read_from_their_three_forms() {
        let answer =
            "TODO | Send the signed form\nDATE | 2026-11-20T14:30 | Meeting with Maria\nDATE | 2026-12-01 | Deadline";
        assert_eq!(
            findings(answer),
            [
                todo("Send the signed form"),
                date("Meeting with Maria", "2026-11-20T14:30:00", false),
                date("Deadline", "2026-12-01T00:00:00", true),
            ]
        );
        // Bullets, numbering, case, spacing, a space for the T, seconds, a title with a bar in it.
        let loose =
            "- todo|call back\n2. Date |2026-11-20 09:05| Dentist | bring the card\n* DATE | 2026-11-20T09:05:59 | x";
        assert_eq!(
            findings(loose),
            [
                todo("call back"),
                date("Dentist | bring the card", "2026-11-20T09:05:00", false),
                date("x", "2026-11-20T09:05:00", false),
            ]
        );
    }

    #[test]
    fn nothing_found_and_junk_lines_give_nothing() {
        assert!(findings("NONE").is_empty());
        assert!(findings("").is_empty());
        let junk = "Here is what I found:\nNONE\nTODO\nTODO |   \nDATE | 2026-11-20\nDATE | tomorrow | Lunch\n\
                    DATE | 2026-11-20T14:30 |  \nEVENT | 2026-11-20 | Party\n| x | y\nSure! Anything else?";
        assert!(findings(junk).is_empty(), "{:?}", findings(junk));
        // A good line among junk is still read.
        assert_eq!(findings("I found:\nTODO | pay the bill\nthat is all"), [todo("pay the bill")]);
    }

    #[test]
    fn a_date_that_does_not_exist_is_dropped() {
        for bad in [
            "2026-02-30",
            "2026-13-01",
            "2026-00-10",
            "2026-11-00",
            "2025-02-29",
            "0000-01-01",
            "2026-11-20T24:00",
            "2026-11-20T12:60",
            "2026-11-20T12:30:61",
            "2026-11-20X12:30",
            "2026-11-20T1230",
            "2026-11-2",
            "20261120",
            "2026/11/20",
            "+026-11-20",
            "2026-11-20T+1:30",
            "2026-11-20T12:30Z",
            "\u{662}\u{660}\u{662}\u{666}-11-20",
        ] {
            assert!(findings(&format!("DATE | {bad} | Party")).is_empty(), "{bad}");
        }
        assert_eq!(findings("DATE | 2028-02-29 | Leap day"), [date("Leap day", "2028-02-29T00:00:00", true)]);
        assert!(is_day("2026-11-15") && !is_day("2026-11-31") && !is_day(" 2026-11-15") && !is_day("2026-11-15T00:00"));
    }

    #[test]
    fn at_most_twenty_findings_and_long_texts_are_cut() {
        let many: String = (0..30).map(|n| format!("TODO | thing {n}\n")).collect();
        let got = findings(&many);
        assert_eq!(got.len(), FINDINGS);
        assert_eq!(got[19], todo("thing 19"));
        let long = format!("TODO | {}\nDATE | 2026-11-20 | {}", "a".repeat(500), "b".repeat(500));
        let got = findings(&long);
        assert_eq!(got, [todo(&"a".repeat(300)), date(&"b".repeat(300), "2026-11-20T00:00:00", true)]);
    }
}
