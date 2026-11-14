//! Search snippets: the server's marked text turned into plain text and ranges.
//!
//! `SearchSnippet/get` gives the subject and a piece of the text as HTML-escaped text in which
//! `<mark>` surrounds what was found (`dev/notes/stalwart-mail-features.md`, section 4). The
//! host must not see markup, so it gets the text and where the marks are, counted in UTF-16
//! units as Kotlin counts them.

use crate::model::{MarkRange, Marked};

/// The longest entity looked at, `&#x10FFFF;`.
const ENTITY_MAX: usize = 10;

/// Turns the server's text into [`Marked`]: the entities are unescaped, `<mark>` and `</mark>`
/// become ranges, any other tag stays as the text it is. Marks that touch are one. With
/// `cut_trailing_space` the white space at the end of the text goes (the server leaves one
/// after the preview), and marks that reach into it are cut with it.
pub fn marked(escaped: &str, cut_trailing_space: bool) -> Marked {
    let mut text = String::new();
    let mut units: u32 = 0;
    let mut marks: Vec<MarkRange> = Vec::new();
    let mut open: Option<u32> = None;
    let mut rest = escaped;
    while let Some(c) = rest.chars().next() {
        if let Some(after) = tag(rest, "<mark>") {
            open.get_or_insert(units);
            rest = after;
        } else if let Some(after) = tag(rest, "</mark>") {
            if let Some(start) = open.take() {
                marks.push(MarkRange { start, end: units });
            }
            rest = after;
        } else if let Some((decoded, after)) = entity(rest) {
            text.push(decoded);
            units += decoded.len_utf16() as u32;
            rest = after;
        } else {
            text.push(c);
            units += c.len_utf16() as u32;
            rest = &rest[c.len_utf8()..];
        }
    }
    if let Some(start) = open {
        marks.push(MarkRange { start, end: units });
    }
    if cut_trailing_space {
        text.truncate(text.trim_end().len());
        units = text.encode_utf16().count() as u32;
        for m in &mut marks {
            m.end = m.end.min(units);
        }
    }
    Marked { text, marks: merged(marks) }
}

/// `rest` without the tag when it starts with it (any case).
fn tag<'a>(rest: &'a str, tag: &str) -> Option<&'a str> {
    let head = rest.get(..tag.len())?;
    head.eq_ignore_ascii_case(tag).then(|| &rest[tag.len()..])
}

/// The character an entity at the start of `rest` stands for, and what follows it.
fn entity(rest: &str) -> Option<(char, &str)> {
    let body = rest.strip_prefix('&')?;
    let end = body.char_indices().take(ENTITY_MAX + 1).find(|(_, c)| *c == ';')?.0;
    let (name, after) = (&body[..end], &body[end + 1..]);
    let decoded = match name {
        "lt" => '<',
        "gt" => '>',
        "amp" => '&',
        "quot" => '"',
        "apos" => '\'',
        _ => {
            let number = name.strip_prefix('#')?;
            let code = match number.strip_prefix(['x', 'X']) {
                Some(hex) => u32::from_str_radix(hex, 16).ok()?,
                None => number.parse().ok()?,
            };
            char::from_u32(code).filter(|c| *c != '\0')?
        }
    };
    Some((decoded, after))
}

/// Empty ranges dropped, ranges that touch or overlap joined.
fn merged(marks: Vec<MarkRange>) -> Vec<MarkRange> {
    let mut out: Vec<MarkRange> = Vec::new();
    for m in marks.into_iter().filter(|m| m.end > m.start) {
        match out.last_mut() {
            Some(last) if m.start <= last.end => last.end = last.end.max(m.end),
            _ => out.push(m),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(start: u32, end: u32) -> MarkRange {
        MarkRange { start, end }
    }

    #[test]
    fn marks_become_ranges_in_the_plain_text() {
        let m = marked("zt <mark>Invoice</mark> about <mark>Zebra</mark>", false);
        assert_eq!(m.text, "zt Invoice about Zebra");
        assert_eq!(m.marks, [r(3, 10), r(17, 22)]);
    }

    #[test]
    fn entities_are_unescaped_and_other_tags_are_text() {
        let m = marked("zt <mark>Fish</mark> &lt;b&gt;bold&lt;/b&gt; &amp; chips &#39;x&#x27; &quot;", false);
        assert_eq!(m.text, "zt Fish <b>bold</b> & chips 'x' \"");
        assert_eq!(m.marks, [r(3, 7)]);
        let m = marked("<b>x</b> <script>", false);
        assert_eq!(m.text, "<b>x</b> <script>");
        assert!(m.marks.is_empty());
        // Not an entity: left alone.
        assert_eq!(marked("fish &chips; &#xZZ; & &", false).text, "fish &chips; &#xZZ; & &");
    }

    #[test]
    fn ranges_count_utf16_units() {
        // The emoji is two units, the other letters one.
        let m = marked("😀 <mark>şeker</mark> é<mark>😀</mark>", false);
        assert_eq!(m.text, "😀 şeker é😀");
        assert_eq!(m.marks, [r(3, 8), r(10, 12)]);
    }

    #[test]
    fn touching_marks_are_one_and_empty_ones_go() {
        let m = marked("<mark>quarterly</mark> <mark>invoice</mark><mark>s</mark> <mark></mark>x", false);
        assert_eq!(m.text, "quarterly invoices x");
        assert_eq!(m.marks, [r(0, 9), r(10, 18)]);
        let m = marked("<mark>a</mark><mark>b</mark>", false);
        assert_eq!(m.marks, [r(0, 2)]);
    }

    #[test]
    fn the_stray_space_at_the_end_of_a_preview_is_cut() {
        let m = marked("Second paragraph &amp; <mark>hedgehog</mark> notes ", true);
        assert_eq!(m.text, "Second paragraph & hedgehog notes");
        assert_eq!(m.marks, [r(19, 27)]);
        let m = marked("the <mark>end </mark>", true);
        assert_eq!((m.text.as_str(), m.marks), ("the end", vec![r(4, 7)]));
        assert_eq!(marked("kept ", false).text, "kept ");
    }

    #[test]
    fn damaged_marks_do_no_harm() {
        let m = marked("a </mark>b <mark>c", false);
        assert_eq!(m.text, "a b c");
        assert_eq!(m.marks, [r(4, 5)]);
        let m = marked("<MARK>x</Mark>", false);
        assert_eq!((m.text.as_str(), m.marks), ("x", vec![r(0, 1)]));
        assert_eq!(marked("", true), Marked { text: String::new(), marks: Vec::new() });
    }
}
