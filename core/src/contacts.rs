//! What the contact list does with a contact besides showing it: the order, what a search looks
//! at, and the search. Pure, so the cache and the tests share it.

use crate::model::{Contact, ContactRow};

/// A contact as the cache keeps it: the contact and what the list is built from.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub id: String,
    pub contact: Contact,
    pub row: ContactRow,
    /// What the list is sorted by.
    pub sort_key: String,
    /// Folded name, organization and mail addresses, one per line.
    pub search: String,
    /// The digits of each phone number, separated by spaces.
    pub digits: String,
}

/// What a search needs of a cached contact.
#[derive(Debug, Clone, PartialEq)]
pub struct Indexed {
    pub row: ContactRow,
    pub search: String,
    pub digits: String,
}

impl Entry {
    pub fn new(id: String, contact: Contact) -> Entry {
        let row = ContactRow {
            id: id.clone(),
            name: contact.name.clone(),
            email: contact.emails.first().map(|e| e.value.clone()).unwrap_or_default(),
            organization: contact.organization.clone(),
        };
        let words = [&contact.name, &contact.given_name, &contact.family_name, &contact.organization];
        let words = words.into_iter().cloned().chain(contact.emails.iter().map(|e| e.value.clone()));
        let search = words.map(|w| fold(&w)).collect::<Vec<_>>().join("\n");
        let digits = contact.phones.iter().map(|p| digits(&p.value)).collect::<Vec<_>>().join(" ");
        Entry { sort_key: fold(&contact.name), id, contact, row, search, digits }
    }
}

/// Lower case with the accents of Latin letters taken off, so that "Özdemir" is found by
/// "ozdemir" and sorts among the O's. It handles the Latin-1 and Latin Extended-A letters and
/// combining marks, and turns ß, æ, œ into ss, ae, oe. It does not transliterate (ä is a, not
/// ae), fold Greek or Cyrillic accents, or know any language's own order of letters (Swedish ö
/// after z): other scripts sort by code point.
pub fn fold(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars().flat_map(char::to_lowercase) {
        match c {
            // Combining marks: what an `İ` leaves behind when lower-cased, and decomposed accents.
            '\u{300}'..='\u{36f}' => {}
            'à'..='å' | 'ā' | 'ă' | 'ą' => out.push('a'),
            'ç' | 'ć' | 'ĉ' | 'ċ' | 'č' => out.push('c'),
            'ď' | 'đ' | 'ð' => out.push('d'),
            'è'..='ë' | 'ē' | 'ĕ' | 'ė' | 'ę' | 'ě' => out.push('e'),
            'ĝ' | 'ğ' | 'ġ' | 'ģ' => out.push('g'),
            'ĥ' | 'ħ' => out.push('h'),
            'ì'..='ï' | 'ĩ' | 'ī' | 'ĭ' | 'į' | 'ı' => out.push('i'),
            'ĵ' => out.push('j'),
            'ķ' => out.push('k'),
            'ĺ' | 'ļ' | 'ľ' | 'ŀ' | 'ł' => out.push('l'),
            'ñ' | 'ń' | 'ņ' | 'ň' => out.push('n'),
            'ò'..='ö' | 'ø' | 'ō' | 'ŏ' | 'ő' => out.push('o'),
            'ŕ' | 'ŗ' | 'ř' => out.push('r'),
            'ś' | 'ŝ' | 'ş' | 'š' => out.push('s'),
            'ţ' | 'ť' | 'ŧ' => out.push('t'),
            'ù'..='ü' | 'ũ' | 'ū' | 'ŭ' | 'ů' | 'ű' | 'ų' => out.push('u'),
            'ŵ' => out.push('w'),
            'ý' | 'ÿ' | 'ŷ' => out.push('y'),
            'ź' | 'ż' | 'ž' => out.push('z'),
            'ß' => out.push_str("ss"),
            'æ' => out.push_str("ae"),
            'œ' => out.push_str("oe"),
            other => out.push(other),
        }
    }
    out
}

/// The ASCII digits of a phone number, without the spaces and punctuation around them.
pub fn digits(text: &str) -> String {
    text.chars().filter(char::is_ascii_digit).collect()
}

/// The contacts that fit what the user typed, in the order given. Every word of the query must
/// be in the name, organization or one of the mail addresses (accents and case do not matter);
/// a query of only digits and phone punctuation also fits a phone number that contains those
/// digits. A blank query fits everything.
pub fn select(index: Vec<Indexed>, query: &str) -> Vec<ContactRow> {
    let terms: Vec<String> = fold(query).split_whitespace().map(str::to_string).collect();
    let phone = digits(query);
    let phone_like = !phone.is_empty() && query.chars().all(|c| c.is_ascii_digit() || " +-().,/".contains(c));
    let fits = |i: &Indexed| {
        terms.iter().all(|t| i.search.contains(t.as_str()))
            || (phone_like && i.digits.split(' ').any(|n| n.contains(&phone)))
    };
    index.into_iter().filter(fits).map(|i| i.row).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Labeled;

    fn contact(name: &str, emails: &[&str], phones: &[&str]) -> Contact {
        let labeled = |v: &&str| Labeled { label: String::new(), value: v.to_string() };
        Contact {
            address_book_id: "b".into(),
            name: name.into(),
            given_name: String::new(),
            family_name: String::new(),
            organization: "Acme".into(),
            emails: emails.iter().map(labeled).collect(),
            phones: phones.iter().map(labeled).collect(),
            addresses: Vec::new(),
            notes: "secret note".into(),
        }
    }

    fn indexed(entries: &[Entry]) -> Vec<Indexed> {
        entries
            .iter()
            .map(|e| Indexed { row: e.row.clone(), search: e.search.clone(), digits: e.digits.clone() })
            .collect()
    }

    fn found(entries: &[Entry], query: &str) -> Vec<String> {
        select(indexed(entries), query).into_iter().map(|r| r.id).collect()
    }

    #[test]
    fn folding_takes_accents_and_case_off_latin_letters_only() {
        assert_eq!(fold("Özdemir ÇAĞLAR Şişli"), "ozdemir caglar sisli");
        assert_eq!(fold("Åsa Łukasz Ørsted Dvořák"), "asa lukasz orsted dvorak");
        assert_eq!(fold("Straße Æsir Cœur"), "strasse aesir coeur");
        // Turkish dotted and dotless i, and an accent that is a separate character.
        assert_eq!(fold("İstanbul Iğdır ışık"), "istanbul igdir isik");
        assert_eq!(fold("Zoe\u{308}"), "zoe");
        // What it does not do: transliterate, or touch other scripts beyond lower-casing.
        assert_eq!(fold("Müller"), "muller");
        assert_eq!(fold("Дмитрий Ёж"), "дмитрий ёж");
        assert_eq!(fold("山田 太郎"), "山田 太郎");
    }

    #[test]
    fn an_entry_has_its_row_and_what_a_search_reads() {
        let mut c = contact("Zoë Özdemir", &["Zoe@X.test", "z2@x.test"], &["+49 (30) 1234-5", "0170 99"]);
        c.given_name = "Zoë".into();
        let e = Entry::new("c1".into(), c);
        assert_eq!(
            e.row,
            ContactRow {
                id: "c1".into(),
                name: "Zoë Özdemir".into(),
                email: "Zoe@X.test".into(),
                organization: "Acme".into()
            }
        );
        assert_eq!(e.sort_key, "zoe ozdemir");
        assert_eq!(e.search, "zoe ozdemir\nzoe\n\nacme\nzoe@x.test\nz2@x.test");
        assert_eq!(e.digits, "493012345 017099");
        // Notes and addresses are not searched.
        assert!(!e.search.contains("secret"));
        assert_eq!(Entry::new("c2".into(), contact("No Mail", &[], &[])).row.email, "");
    }

    #[test]
    fn the_query_must_hit_every_word_in_name_organization_or_mail() {
        let entries = [
            Entry::new("a".into(), contact("Anna Roth", &["anna@roth.test"], &[])),
            Entry::new("b".into(), contact("Özge Yılmaz", &["oy@x.test"], &[])),
            Entry::new("c".into(), contact("Tom", &["tom@acme.test"], &[])),
        ];
        assert_eq!(found(&entries, ""), ["a", "b", "c"]);
        assert_eq!(found(&entries, "   "), ["a", "b", "c"]);
        assert_eq!(found(&entries, "ROTH"), ["a"]);
        assert_eq!(found(&entries, "roth anna"), ["a"], "words in any order");
        assert_eq!(found(&entries, "anna tom"), Vec::<String>::new(), "every word must hit");
        assert_eq!(found(&entries, "ozge yilmaz"), ["b"], "accents do not matter");
        assert_eq!(found(&entries, "oy@x"), ["b"], "a mail address");
        assert_eq!(found(&entries, "acme"), ["a", "b", "c"], "the organization");
        assert_eq!(found(&entries, "zzz"), Vec::<String>::new());
    }

    #[test]
    fn a_phone_number_is_found_by_its_digits_however_it_is_written() {
        let entries = [
            Entry::new("a".into(), contact("Anna", &[], &["+49 (30) 1234-567"])),
            Entry::new("b".into(), contact("Bo", &[], &["0170 555 01", "030 99"])),
            Entry::new("c".into(), contact("Cy", &[], &[])),
        ];
        assert_eq!(found(&entries, "30 1234"), ["a"]);
        assert_eq!(found(&entries, "+49301234"), ["a"]);
        assert_eq!(found(&entries, "(0170) 555-01"), ["b"]);
        assert_eq!(found(&entries, "99"), ["b"]);
        // Digits do not run across two numbers.
        assert_eq!(found(&entries, "0155"), Vec::<String>::new());
        // A query with letters is a name search only.
        assert_eq!(found(&entries, "anna 30"), Vec::<String>::new());
    }
}
