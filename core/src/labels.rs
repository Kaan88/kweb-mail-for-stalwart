//! Labels: what the user calls them, and the keywords they are on the server.
//!
//! A label's keyword is its name when the name is plain (lower-case ASCII letters, digits, `-`
//! and `_`), so that other mail programs show the same label. Any other name becomes `kweb-`
//! and the name's UTF-8 bytes in lower-case hex: the server takes only ASCII keywords (and
//! lowers their case), at most 128 bytes. The server cannot list the keywords in use, so the
//! list of labels is kept by the app and completed from the keywords of the mail it has seen.

use std::collections::HashSet;

use crate::{model::Label, Error, Result};

/// Where the list of labels is kept, as JSON, in the account's key/value table.
pub const LABELS_KEY: &str = "labels";
/// Keywords of labels the user deleted: mail may still carry them, but they are not offered
/// again. As JSON.
pub const HIDDEN_KEY: &str = "labels_hidden";

/// The longest name, in characters.
pub const NAME_MAX_CHARS: usize = 60;
/// The longest keyword the server takes, in bytes.
const KEYWORD_MAX_BYTES: usize = 128;
const PREFIX: &str = "kweb-";
/// The most bytes of a name that is hex-encoded: two hex digits each, behind the prefix.
const ENCODED_NAME_MAX_BYTES: usize = (KEYWORD_MAX_BYTES - PREFIX.len()) / 2;

/// The name as it is kept: trimmed, one line, not blank, at most [`NAME_MAX_CHARS`] characters.
pub fn check_name(name: &str) -> Result<String> {
    let name = name.trim();
    if name.is_empty() {
        return Err(Error::invalid("A label needs a name."));
    }
    if name.chars().any(char::is_control) {
        return Err(Error::invalid("A label name is one line."));
    }
    if name.chars().count() > NAME_MAX_CHARS {
        return Err(Error::invalid(format!("A label name has at most {NAME_MAX_CHARS} characters.")));
    }
    // A name that is hex-encoded must fit the keyword whole, or two names would share one.
    if !is_plain(name) && name.len() > ENCODED_NAME_MAX_BYTES {
        return Err(Error::invalid(format!(
            "A label name takes at most {ENCODED_NAME_MAX_BYTES} bytes (fewer characters if it has accents or symbols)."
        )));
    }
    Ok(name.to_string())
}

fn is_plain(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with(PREFIX)
        && !is_system(name)
        && name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
}

/// The keyword of a label with this name.
pub fn keyword_for(name: &str) -> String {
    if is_plain(name) {
        return name.to_string();
    }
    // Whole characters only, so that the name decodes again; two bytes of hex per byte.
    let room = ENCODED_NAME_MAX_BYTES;
    let mut bytes = 0;
    let mut hex = String::new();
    for c in name.chars() {
        bytes += c.len_utf8();
        if bytes > room {
            break;
        }
        let mut buf = [0u8; 4];
        for b in c.encode_utf8(&mut buf).bytes() {
            hex.push_str(&format!("{b:02x}"));
        }
    }
    format!("{PREFIX}{hex}")
}

/// The name a keyword stands for: the decoded name of a `kweb-` keyword, else the keyword itself.
pub fn name_for(keyword: &str) -> String {
    keyword.strip_prefix(PREFIX).and_then(decode_hex).unwrap_or_else(|| keyword.to_string())
}

fn decode_hex(hex: &str) -> Option<String> {
    if hex.is_empty() || !hex.len().is_multiple_of(2) || !hex.is_ascii() {
        return None;
    }
    let bytes: Option<Vec<u8>> =
        (0..hex.len()).step_by(2).map(|i| u8::from_str_radix(&hex[i..i + 2], 16).ok()).collect();
    String::from_utf8(bytes?).ok().filter(|name| !name.trim().is_empty() && !name.chars().any(char::is_control))
}

/// A keyword the server or a mail program uses for its own purposes: `$...`, and the IMAP flags
/// (and the junk marks) without a `$`. Never offered as a label.
pub fn is_system(keyword: &str) -> bool {
    keyword.starts_with('$')
        || keyword.starts_with('\\')
        || matches!(
            keyword,
            "seen" | "answered" | "flagged" | "deleted" | "draft" | "recent" | "junk" | "nonjunk" | "notjunk"
        )
}

/// The labels the user has: the kept list, then the keywords found on cached mail that are
/// neither kept, nor hidden, nor a system keyword, all by name (case ignored).
pub fn merge(kept: Vec<Label>, hidden: &HashSet<String>, found: &[String]) -> Vec<Label> {
    let mut seen = HashSet::new();
    let mut out: Vec<Label> = kept.into_iter().filter(|l| seen.insert(l.id.clone())).collect();
    let more: Vec<Label> = found
        .iter()
        .filter(|k| !is_system(k) && !hidden.contains(*k) && seen.insert((*k).clone()))
        .map(|k| Label { id: k.clone(), name: name_for(k) })
        .collect();
    out.extend(more);
    out.sort_by_key(|l| (l.name.to_lowercase(), l.id.clone()));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn label(id: &str, name: &str) -> Label {
        Label { id: id.into(), name: name.into() }
    }

    #[test]
    fn a_plain_name_is_its_own_keyword() {
        assert_eq!(keyword_for("work"), "work");
        assert_eq!(keyword_for("to-do_2"), "to-do_2");
    }

    #[test]
    fn any_other_name_is_hex_behind_a_prefix_and_comes_back() {
        assert_eq!(keyword_for("Work"), "kweb-576f726b");
        for name in ["Mom and Dad", "İş", "日本語", "a😀", "work/home"] {
            let keyword = keyword_for(name);
            assert!(keyword.starts_with("kweb-") && keyword.is_ascii() && keyword.len() <= 128, "{keyword}");
            assert_eq!(keyword, keyword.to_ascii_lowercase());
            assert_eq!(name_for(&keyword), name);
        }
        // Not mistaken for a made keyword.
        assert_eq!(keyword_for("kweb-6162"), "kweb-6b7765622d36313632");
        assert_eq!(name_for(&keyword_for("kweb-6162")), "kweb-6162");
    }

    #[test]
    fn a_long_name_is_cut_at_a_whole_character_to_fit() {
        let name = "é".repeat(60);
        let keyword = keyword_for(&name);
        assert!(keyword.len() <= 128);
        let back = name_for(&keyword);
        assert!(!back.is_empty() && name.starts_with(&back) && back.chars().all(|c| c == 'é'));
    }

    #[test]
    fn a_system_keyword_as_a_name_is_hex_encoded() {
        assert_eq!(keyword_for("junk"), "kweb-6a756e6b");
        for k in ["seen", "answered", "flagged", "deleted", "draft", "recent", "nonjunk", "notjunk"] {
            let keyword = keyword_for(k);
            assert!(keyword.starts_with("kweb-") && !is_system(&keyword), "{keyword}");
            assert_eq!(name_for(&keyword), k);
        }
    }

    #[test]
    fn a_name_that_would_be_cut_is_refused() {
        assert!(check_name(&"é".repeat(30)).is_ok());
        assert!(check_name(&"é".repeat(31)).is_err());
        assert!(check_name(&"x".repeat(60)).is_ok());
    }

    #[test]
    fn a_keyword_that_is_not_made_by_the_app_is_its_own_name() {
        assert_eq!(name_for("project-x"), "project-x");
        assert_eq!(name_for("kweb-zz"), "kweb-zz");
        assert_eq!(name_for("kweb-"), "kweb-");
        assert_eq!(name_for("kweb-ff"), "kweb-ff");
    }

    #[test]
    fn names_are_checked() {
        assert_eq!(check_name("  Work  ").unwrap(), "Work");
        assert!(check_name("   ").is_err() && check_name("").is_err());
        assert!(check_name("a\nb").is_err() && check_name("a\tb").is_err());
        assert!(check_name(&"x".repeat(60)).is_ok());
        assert!(check_name(&"x".repeat(61)).is_err());
        assert!(check_name(&"é".repeat(30)).is_ok());
    }

    #[test]
    fn system_keywords_are_not_labels() {
        for k in ["$seen", "$label1", "\\seen", "seen", "answered", "draft", "junk", "nonjunk"] {
            assert!(is_system(k), "{k}");
        }
        assert!(!is_system("work") && !is_system("kweb-576f726b"));
    }

    #[test]
    fn found_keywords_complete_the_kept_list() {
        let kept = vec![label("kweb-576f726b", "Work"), label("home", "Home")];
        let hidden: HashSet<String> = ["old".to_string()].into();
        let found: Vec<String> =
            ["$seen", "home", "old", "zeta", "kweb-c4b0c59f", "alpha", "junk"].iter().map(|s| s.to_string()).collect();
        let all = merge(kept, &hidden, &found);
        let ids: Vec<&str> = all.iter().map(|l| l.id.as_str()).collect();
        assert_eq!(ids, ["alpha", "home", "kweb-c4b0c59f", "kweb-576f726b", "zeta"]);
        assert_eq!(all[2].name, "İş");
    }
}
