//! The words and dates the attention rules look for in a mail's subject and preview. English and
//! Turkish. No model, no network: plain scanning over text the cache already has.

use std::collections::HashSet;

use chrono::{Datelike, Duration, NaiveDate, NaiveDateTime, NaiveTime, Weekday};

/// A date (and perhaps a time of day) found in a text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Found {
    pub date: NaiveDate,
    pub time: Option<NaiveTime>,
}

fn month_of(word: &str) -> Option<u32> {
    let w = word.trim_end_matches('.');
    Some(match w {
        "jan" | "january" | "ocak" | "oca" => 1,
        "feb" | "february" | "şubat" | "şub" | "subat" | "sub" => 2,
        "mar" | "march" | "mart" => 3,
        "apr" | "april" | "nisan" | "nis" => 4,
        "may" | "mayıs" | "mayis" => 5,
        "jun" | "june" | "haziran" | "haz" => 6,
        "jul" | "july" | "temmuz" | "tem" => 7,
        "aug" | "august" | "ağustos" | "agustos" | "ağu" | "agu" => 8,
        "sep" | "sept" | "september" | "eylül" | "eylul" | "eyl" => 9,
        "oct" | "october" | "ekim" | "eki" => 10,
        "nov" | "november" | "kasım" | "kasim" | "kas" => 11,
        "dec" | "december" | "aralık" | "aralik" | "ara" => 12,
        _ => return None,
    })
}

fn weekday_of(word: &str) -> Option<Weekday> {
    Some(match word.trim_end_matches('.') {
        "mon" | "monday" | "pazartesi" | "pzt" => Weekday::Mon,
        "tue" | "tues" | "tuesday" | "salı" | "sali" => Weekday::Tue,
        "wed" | "weds" | "wednesday" | "çarşamba" | "carsamba" => Weekday::Wed,
        "thu" | "thur" | "thurs" | "thursday" | "perşembe" | "persembe" => Weekday::Thu,
        "fri" | "friday" | "cuma" => Weekday::Fri,
        "sat" | "saturday" | "cumartesi" | "cmt" => Weekday::Sat,
        "sun" | "sunday" | "pazar" => Weekday::Sun,
        _ => return None,
    })
}

/// Words that make a bare weekday a plan ("on Friday", "before Friday") and not a passing mention.
fn leads_weekday(word: &str) -> bool {
    matches!(word, "on" | "next" | "this" | "by" | "before" | "until" | "till" | "bu" | "gelecek" | "önümüzdeki" | "en")
}

fn tokens(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|c: char| {
            c.is_whitespace() || matches!(c, ',' | ';' | '(' | ')' | '"' | '“' | '”' | '[' | ']' | '!' | '?')
        })
        .map(|t| t.trim_matches(|c: char| c == '\'' || c == '’' || c == ':').to_string())
        .filter(|t| !t.is_empty())
        .collect()
}

/// A day number with an optional ordinal ("20", "20th", "20'sinde").
fn day_number(token: &str) -> Option<u32> {
    let t = token.split(['\'', '’']).next().unwrap_or(token);
    let digits: String = t.chars().take_while(char::is_ascii_digit).collect();
    let rest = &t[digits.len()..];
    if digits.is_empty() || digits.len() > 2 || !matches!(rest, "" | "st" | "nd" | "rd" | "th" | ".") {
        return None;
    }
    digits.parse().ok().filter(|d| (1..=31).contains(d))
}

/// "19:30", "3pm", "3:30pm". "19.30" is not read: too close to a date.
fn clock(token: &str) -> Option<NaiveTime> {
    let t = token.trim_end_matches(['.', ',']);
    let (body, pm) = if let Some(b) = t.strip_suffix("pm") {
        (b, Some(true))
    } else if let Some(b) = t.strip_suffix("am") {
        (b, Some(false))
    } else {
        (t, None)
    };
    let (h, m) = match body.split_once(':') {
        Some((h, m)) if m.len() == 2 => (h.parse::<u32>().ok()?, m.parse::<u32>().ok()?),
        Some(_) => return None,
        None if pm.is_some() => (body.parse::<u32>().ok()?, 0),
        None => return None,
    };
    let h = match pm {
        Some(_) if !(1..=12).contains(&h) => return None,
        Some(true) if h < 12 => h + 12,
        Some(false) if h == 12 => 0,
        _ => h,
    };
    NaiveTime::from_hms_opt(h, m, 0)
}

/// "at 3", "saat 15": a bare hour after the word. 1 to 6 is taken as the afternoon.
fn spoken_hour(prev: &str, token: &str) -> Option<NaiveTime> {
    if !matches!(prev, "at" | "saat" | "@") {
        return None;
    }
    let t = token.split(['\'', '’']).next().unwrap_or(token);
    let h: u32 = t.parse().ok().filter(|h| *h <= 23)?;
    let h = if (1..=6).contains(&h) { h + 12 } else { h };
    NaiveTime::from_hms_opt(h, 0, 0)
}

/// The time of day in the tokens from `i` on (the next three at most).
fn time_after(toks: &[String], i: usize) -> Option<NaiveTime> {
    for k in i..(i + 3).min(toks.len()) {
        if let Some(t) = clock(&toks[k]) {
            return Some(t);
        }
        if k > 0 {
            if let Some(t) = spoken_hour(&toks[k - 1], &toks[k]) {
                return Some(t);
            }
        }
    }
    None
}

fn year_fix(today: NaiveDate, month: u32, day: u32, year: Option<i32>) -> Option<NaiveDate> {
    match year {
        Some(y) => NaiveDate::from_ymd_opt(y, month, day),
        None => {
            let d = NaiveDate::from_ymd_opt(today.year(), month, day)?;
            // A date well in the past is the same date next year ("5 Jan" read in December).
            if (today - d).num_days() > 183 {
                NaiveDate::from_ymd_opt(today.year() + 1, month, day)
            } else {
                Some(d)
            }
        }
    }
}

fn next_weekday(today: NaiveDate, wd: Weekday) -> NaiveDate {
    let ahead = (wd.num_days_from_monday() as i64 - today.weekday().num_days_from_monday() as i64).rem_euclid(7);
    today + Duration::days(ahead)
}

/// The first date or time in `text` that is still ahead of `now` (local wall-clock time), as
/// "Fri 19:30", "20 Nov", "tomorrow at 3", "20.11.", "2026-11-20", "Cuma 19:30", "20 Kasım",
/// "yarın saat 15" are written.
pub fn find_future_date(text: &str, now: NaiveDateTime) -> Option<Found> {
    let today = now.date();
    let toks = tokens(text);
    let mut best: Option<Found> = None;
    let mut keep = |f: Found| {
        let ahead = match f.time {
            Some(t) => f.date.and_time(t) > now,
            None => f.date >= today,
        };
        if ahead && best.is_none() {
            best = Some(f);
        }
    };
    for (i, tok) in toks.iter().enumerate() {
        let t = tok.as_str();
        // 2026-11-20, 2026-11-20t19:30
        let (head, tail) = t.split_once('t').unwrap_or((t, ""));
        if head.len() == 10 && head.as_bytes()[4] == b'-' && head.as_bytes()[7] == b'-' {
            let parts: Vec<&str> = head.split('-').collect();
            if let (Ok(y), Ok(m), Ok(d)) = (parts[0].parse(), parts[1].parse(), parts[2].parse()) {
                if let Some(date) = NaiveDate::from_ymd_opt(y, m, d) {
                    let time = clock(tail).or_else(|| time_after(&toks, i + 1));
                    keep(Found { date, time });
                    continue;
                }
            }
        }
        // 20.11. 20.11.2026 20/11 20/11/2026
        let sep = if t.contains('.') { '.' } else { '/' };
        let numeric = t.chars().all(|c| c.is_ascii_digit() || c == '.' || c == '/');
        if numeric && t.chars().next().is_some_and(|c| c.is_ascii_digit()) && t.contains(sep) {
            let parts: Vec<&str> = t.trim_end_matches('.').split(sep).collect();
            if (2..=3).contains(&parts.len()) {
                if let (Ok(d), Ok(m)) = (parts[0].parse::<u32>(), parts[1].parse::<u32>()) {
                    let year =
                        parts.get(2).and_then(|y| y.parse::<i32>().ok()).map(|y| if y < 100 { 2000 + y } else { y });
                    // "3.2" is a version; "20.11." and "20/11" are dates.
                    let date_like = parts.len() == 3 || t.ends_with('.') || sep == '/';
                    if (1..=31).contains(&d) && (1..=12).contains(&m) && date_like {
                        if let Some(date) = year_fix(today, m, d, year) {
                            keep(Found { date, time: time_after(&toks, i + 1) });
                            continue;
                        }
                    }
                }
            }
        }
        // 20 Nov, 20th November, 20 Kasım ; Nov 20, November 20th
        if let Some(d) = day_number(t) {
            if let Some(m) = toks.get(i + 1).and_then(|n| month_of(n)) {
                if let Some(date) = year_fix(today, m, d, None) {
                    keep(Found { date, time: time_after(&toks, i + 2) });
                    continue;
                }
            }
        }
        if let Some(m) = month_of(t) {
            if let Some(d) = toks.get(i + 1).and_then(|n| day_number(n)) {
                if let Some(date) = year_fix(today, m, d, None) {
                    keep(Found { date, time: time_after(&toks, i + 2) });
                    continue;
                }
            }
        }
        // tomorrow, today, yarın, bugün
        match t {
            "tomorrow" | "yarın" | "yarin" => {
                keep(Found { date: today + Duration::days(1), time: time_after(&toks, i + 1) });
                continue;
            }
            "today" | "bugün" | "bugun" => {
                if let Some(time) = time_after(&toks, i + 1) {
                    keep(Found { date: today, time: Some(time) });
                    continue;
                }
            }
            _ => {}
        }
        // Fri 19:30, on Friday, Cuma 19:30
        if let Some(wd) = weekday_of(t) {
            let time = time_after(&toks, i + 1);
            let planned = i > 0 && leads_weekday(&toks[i - 1]);
            if time.is_some() || planned {
                let mut date = next_weekday(today, wd);
                // Today's weekday with the time gone is next week's.
                if date == today && time.is_some_and(|tm| date.and_time(tm) <= now) {
                    date += Duration::days(7);
                }
                // A bare weekday that is today is not a plan for later.
                if time.is_some() || date > today {
                    keep(Found { date, time });
                }
            }
        }
    }
    best
}

fn words(text: &str) -> Vec<String> {
    text.to_lowercase().split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).map(str::to_string).collect()
}

/// A word that says something is owed or booked.
pub fn has_deadline_word(text: &str) -> bool {
    const EXACT: [&str; 14] = [
        "deadline",
        "deadlines",
        "due",
        "invoice",
        "invoices",
        "payment",
        "payments",
        "rsvp",
        "appointment",
        "booking",
        "reservation",
        "flight",
        "ticket",
        "tickets",
    ];
    // Turkish takes suffixes, so these are stems.
    const STEMS: [&str; 10] =
        ["fatura", "ödeme", "odeme", "randevu", "rezervasyon", "uçuş", "ucus", "vade", "teslim", "bilet"];
    let low = text.to_lowercase();
    if low.contains("son tarih") || low.contains("son gün") || low.contains("son gun") {
        return true;
    }
    words(&low).iter().any(|w| EXACT.contains(&w.as_str()) || STEMS.iter().any(|s| w.starts_with(s)))
}

/// A number with a currency next to it.
pub fn has_amount(text: &str) -> bool {
    let chars: Vec<char> = text.to_lowercase().chars().collect();
    let digit_at = |k: Option<usize>| k.and_then(|k| chars.get(k)).is_some_and(|c| c.is_ascii_digit());
    for (i, c) in chars.iter().enumerate() {
        if matches!(c, '€' | '$' | '£' | '₺') && (digit_at(i.checked_sub(1)) || digit_at(Some(i + 1))) {
            return true;
        }
        if c.is_ascii_digit() && (i == 0 || !chars[i - 1].is_ascii_digit()) {
            let mut j = i;
            while chars.get(j).is_some_and(|c| c.is_ascii_digit() || matches!(c, '.' | ',')) {
                j += 1;
            }
            if chars.get(j) == Some(&' ') {
                j += 1;
            }
            let code: String = chars.iter().skip(j).take(4).collect();
            let word = code.split(|c: char| !c.is_alphabetic()).next().unwrap_or("");
            if matches!(word, "tl" | "eur" | "usd" | "gbp" | "try" | "chf") {
                return true;
            }
            if chars.get(j).is_some_and(|c| matches!(c, '€' | '$' | '£' | '₺')) {
                return true;
            }
        }
    }
    false
}

/// Something asked of the user: a question mark, or the user's first name before "can you",
/// "could you", "please" (and the Turkish "lütfen", "yapar mısın").
pub fn asks_user(text: &str, first_names: &[String]) -> bool {
    if text.contains('?') {
        return true;
    }
    const ASKS: [&str; 8] =
        ["can you", "could you", "would you", "will you", "please", "lütfen", "yapar mısın", "yapabilir misin"];
    let low = text.to_lowercase();
    for name in first_names.iter().map(|n| n.to_lowercase()).filter(|n| n.chars().count() >= 2) {
        let mut from = 0;
        while let Some(p) = low[from..].find(&name) {
            let at = from + p + name.len();
            let rest = low[at..].trim_start_matches([',', ':', ' ', '-']);
            if ASKS.iter().any(|a| rest.starts_with(a)) {
                return true;
            }
            from = at;
        }
    }
    false
}

/// A mail the user sent that waits for an answer: a question mark, or a request to the reader
/// ("can you", "could you", "let me know" and the Turkish ones). A bare "please" is not enough:
/// "please find attached" asks nothing.
pub fn asks_reader(text: &str) -> bool {
    if text.contains('?') {
        return true;
    }
    const ASKS: [&str; 10] = [
        "can you",
        "could you",
        "would you",
        "will you",
        "let me know",
        "please confirm",
        "please reply",
        "haber ver",
        "yapar mısın",
        "yapabilir misin",
    ];
    let low = text.to_lowercase();
    ASKS.iter().any(|a| low.contains(a))
}

const STOP: [&str; 42] = [
    "with",
    "from",
    "that",
    "this",
    "have",
    "your",
    "about",
    "please",
    "hello",
    "thanks",
    "thank",
    "regarding",
    "will",
    "would",
    "could",
    "there",
    "their",
    "what",
    "when",
    "where",
    "which",
    "into",
    "over",
    "need",
    "meeting",
    "mail",
    "email",
    "again",
    "just",
    "some",
    "more",
    "için",
    "icin",
    "merhaba",
    "lütfen",
    "lutfen",
    "hakkında",
    "hakkinda",
    "gibi",
    "daha",
    "olan",
    "toplantı",
];

/// The words of a title or subject worth comparing: four letters or more, not filler.
pub fn significant_words(text: &str) -> HashSet<String> {
    words(text).into_iter().filter(|w| w.chars().count() >= 4 && !STOP.contains(&w.as_str())).collect()
}

/// How many words of `a` have a word of `b` that starts the same (one is the start of the other:
/// "sign" and "signed"). Both sets hold words of four letters or more.
pub fn shared_stems(a: &HashSet<String>, b: &HashSet<String>) -> usize {
    a.iter().filter(|x| b.iter().any(|y| x.starts_with(y.as_str()) || y.starts_with(x.as_str()))).count()
}

/// File names ("contract-v3.pdf") and numbers of three digits or more in a text.
pub fn file_names_and_numbers(text: &str) -> HashSet<String> {
    let mut out = HashSet::new();
    for tok in text.to_lowercase().split(|c: char| c.is_whitespace() || matches!(c, ',' | ';' | '(' | ')' | '"' | ':'))
    {
        let tok = tok.trim_matches(|c: char| !c.is_alphanumeric());
        if let Some((stem, ext)) = tok.rsplit_once('.') {
            if !stem.is_empty() && (2..=4).contains(&ext.len()) && ext.chars().all(|c| c.is_ascii_alphabetic()) {
                out.insert(tok.to_string());
            }
        }
        if tok.len() >= 3 && tok.chars().all(|c| c.is_ascii_digit()) {
            out.insert(tok.to_string());
        }
        // An identifier: letters, a hyphen and a digit ("contract-v3").
        if tok.contains('-') && tok.chars().any(|c| c.is_ascii_digit()) && tok.chars().any(char::is_alphabetic) {
            out.insert(tok.to_string());
        }
    }
    out
}

/// The text without a leading "Re:", "Fwd:" and the like.
pub fn bare_subject(subject: &str) -> &str {
    let mut s = subject.trim();
    loop {
        let low = s.to_lowercase();
        match ["re:", "fw:", "fwd:", "aw:", "yn:", "ynt:"].iter().find(|p| low.starts_with(**p)) {
            Some(p) => s = s[p.len()..].trim_start(),
            None => return s,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn now() -> NaiveDateTime {
        // Tuesday 17 November 2026, 09:40
        NaiveDate::from_ymd_opt(2026, 11, 17).unwrap().and_hms_opt(9, 40, 0).unwrap()
    }

    fn at(text: &str) -> Option<(String, Option<String>)> {
        find_future_date(text, now()).map(|f| (f.date.to_string(), f.time.map(|t| t.format("%H:%M").to_string())))
    }

    fn some(d: &str, t: Option<&str>) -> Option<(String, Option<String>)> {
        Some((d.to_string(), t.map(str::to_string)))
    }

    #[test]
    fn english_dates() {
        assert_eq!(at("Call Fri 19:30"), some("2026-11-20", Some("19:30")));
        assert_eq!(at("Invoice due 20 Nov"), some("2026-11-20", None));
        assert_eq!(at("see you tomorrow at 3"), some("2026-11-18", Some("15:00")));
        assert_eq!(at("Due 20.11."), some("2026-11-20", None));
        assert_eq!(at("Deadline 2026-11-20"), some("2026-11-20", None));
        assert_eq!(at("Deadline 2026-11-20T10:30"), some("2026-11-20", Some("10:30")));
        assert_eq!(at("November 30th, 5pm"), some("2026-11-30", Some("17:00")));
        assert_eq!(at("send it before Friday"), some("2026-11-20", None));
    }

    #[test]
    fn turkish_dates() {
        assert_eq!(at("Toplantı Cuma 19:30"), some("2026-11-20", Some("19:30")));
        assert_eq!(at("Fatura son tarih 30 Kasım"), some("2026-11-30", None));
        assert_eq!(at("yarın saat 15 görüşelim"), some("2026-11-18", Some("15:00")));
        assert_eq!(at("randevu 3 Aralık"), some("2026-12-03", None));
        assert_eq!(at("bugün saat 18"), some("2026-11-17", Some("18:00")));
    }

    #[test]
    fn the_past_and_passing_mentions_are_not_dates() {
        assert_eq!(at("it was on 10 Nov"), None);
        assert_eq!(at("today at 8:00"), None); // 08:00 is gone
        assert_eq!(at("I love Fridays"), None);
        assert_eq!(at("see you friday"), None); // a bare weekday with no lead word and no time
        assert_eq!(at("version 3.2"), None);
        assert_eq!(at("Invoice 1042"), None);
    }

    #[test]
    fn a_date_early_next_year() {
        let dec = NaiveDate::from_ymd_opt(2026, 12, 28).unwrap().and_hms_opt(9, 0, 0).unwrap();
        assert_eq!(find_future_date("5 Jan", dec).map(|f| f.date.to_string()), Some("2027-01-05".into()));
    }

    #[test]
    fn deadline_words_and_amounts() {
        assert!(has_deadline_word("Invoice 1042, due 30 Nov"));
        assert!(has_deadline_word("Faturanız hazır"));
        assert!(has_deadline_word("Son tarih yarın"));
        assert!(has_deadline_word("RSVP by Friday"));
        assert!(!has_deadline_word("Weekly notes"));
        assert!(!has_deadline_word("An individual outdue"));
        assert!(has_amount("pay 120,50 EUR"));
        assert!(has_amount("Total: €45"));
        assert!(has_amount("250 TL"));
        assert!(!has_amount("Release plan 2026"));
    }

    #[test]
    fn asking_the_user() {
        let names = vec!["Kaan".to_string()];
        assert!(asks_user("Can you send the menu choices?", &names));
        assert!(asks_user("Kaan, can you send the menu", &names));
        assert!(asks_user("Kaan lütfen gönder", &names));
        assert!(!asks_user("Thanks, please see attached", &names));
        assert!(!asks_user("The notes are attached.", &names));
        assert!(asks_reader("Could you send me the quote by Friday"));
        assert!(asks_reader("Toplantı saati uygun mu?"));
        assert!(asks_reader("Let me know when it is done"));
        assert!(!asks_reader("Please find the notes attached."));
        assert!(!asks_reader("probe"));
    }

    #[test]
    fn words_and_names() {
        let a = significant_words("Re: Contract-v3 signed copy");
        assert!(a.contains("contract") && a.contains("signed") && a.contains("copy"));
        assert!(file_names_and_numbers("see Contract-v3.pdf and invoice 0311").contains("contract-v3.pdf"));
        assert!(file_names_and_numbers("see Contract-v3.pdf and invoice 0311").contains("0311"));
        assert_eq!(bare_subject("Re: Fwd: Hi"), "Hi");
    }
}
