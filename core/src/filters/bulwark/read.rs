//! Reading a script the way Bulwark's store does, without parsing Sieve.
//!
//! The rules come from the JSON in the metadata comment. Everything after the part the app
//! writes from those rules is the external section (rules other tools made), kept as text. The
//! only Sieve this reads is the `require` line's list of strings. The part the app writes is
//! found by generating it again: when the script starts with exactly that, the rest is
//! external; when it does not (another Bulwark version wrote it), the external header marks the
//! rest, and without the header the script is opaque: shown, never rewritten.

use super::{
    generate::{generate, js_trim, Options, EXTERNAL_HEADER},
    model::{Metadata, METADATA_BEGIN, METADATA_END},
};

#[derive(Debug, Clone, PartialEq)]
pub enum Reading {
    /// Blank: no rules and nothing to keep.
    Empty,
    /// No metadata comment: written by something else. Shown, never rewritten.
    Foreign,
    /// A metadata comment that cannot be read, or a script whose rule part is not what the
    /// comment says and has nothing to mark where the external part begins.
    Opaque,
    Rules(Box<Parsed>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Parsed {
    pub meta: Metadata,
    /// The `require` tokens the script had that nothing of ours accounts for (`include` is
    /// ours, unless the external part uses it).
    pub external_requires: Vec<String>,
    /// The external section as text, trimmed.
    pub external: String,
    /// The rule part of the script is not what this code writes from the metadata (another
    /// version wrote it): saving writes it as this code does.
    pub differs: bool,
}

/// The strings of the `require` statement at the start of `s` (after blank space), and the text
/// after its `;`. `Ok(None)` when the text does not start with one.
fn require_statement(s: &str) -> Result<Option<(Vec<String>, &str)>, ()> {
    let t = s.trim_start();
    let Some(after) = t.strip_prefix("require") else { return Ok(None) };
    if after.chars().next().is_some_and(|c| c.is_ascii_alphanumeric() || c == '_') {
        return Ok(None);
    }
    let mut tokens = Vec::new();
    let mut chars = after.char_indices().peekable();
    let mut in_list = false;
    while let Some((i, c)) = chars.next() {
        match c {
            c if c.is_whitespace() => {}
            '[' if !in_list => in_list = true,
            ']' if in_list => in_list = false,
            ',' if in_list => {}
            '"' => {
                let mut token = String::new();
                loop {
                    match chars.next() {
                        Some((_, '"')) => break,
                        Some((_, '\\')) => token.push(chars.next().ok_or(())?.1),
                        Some((_, c)) => token.push(c),
                        None => return Err(()),
                    }
                }
                tokens.push(token);
            }
            ';' if !in_list => return Ok(Some((tokens, &after[i + 1..]))),
            _ => return Err(()),
        }
    }
    Err(())
}

fn mentions_include(text: &str) -> bool {
    let bytes = text.as_bytes();
    text.match_indices("include").any(|(i, w)| {
        let word = |b: u8| b.is_ascii_alphanumeric() || b == b'_';
        (i == 0 || !word(bytes[i - 1])) && bytes.get(i + w.len()).is_none_or(|b| !word(*b))
    })
}

/// What a script says. `extensions` is what the server lists (the same list that decided
/// what the script's author wrote).
pub fn read(raw: &str, extensions: &[String]) -> Reading {
    let text = raw.replace("\r\n", "\n");
    if text.trim().is_empty() {
        return Reading::Empty;
    }
    let Some(begin) = text.find(METADATA_BEGIN) else { return Reading::Foreign };
    if !text[..begin].trim().is_empty() {
        return Reading::Opaque;
    }
    let json_from = begin + METADATA_BEGIN.len();
    let Some(end) = text[json_from..].find(METADATA_END).map(|e| e + json_from) else { return Reading::Opaque };
    let Some(meta) = Metadata::from_json(&text[json_from..end]) else { return Reading::Opaque };
    let after_metadata = &text[end + METADATA_END.len()..];
    let (tokens, body_from) = match require_statement(after_metadata) {
        Ok(Some((tokens, rest))) => (tokens, text.len() - rest.len()),
        Ok(None) => (Vec::new(), end + METADATA_END.len()),
        Err(()) => return Reading::Opaque,
    };

    let expected = generate(
        &meta,
        "",
        &Options { external_requires: &tokens, include_vacation: meta.include_vacation, extensions },
    );
    let expected = expected.strip_suffix('\n').unwrap_or(&expected);
    let (external_text, differs) = match text.strip_prefix(expected) {
        Some(rest) if rest.is_empty() || rest.starts_with('\n') => {
            let rest = js_trim(rest);
            (js_trim(rest.strip_prefix(EXTERNAL_HEADER).unwrap_or(rest)).to_string(), false)
        }
        _ => {
            // Another version wrote the rule part. The header, on a line of its own, marks
            // where the external part begins.
            let from = text[body_from..].match_indices(EXTERNAL_HEADER).map(|(i, _)| i + body_from).find(|&i| {
                let line_start = text[..i].rfind('\n').map_or(0, |n| n + 1);
                text[line_start..i].trim().is_empty()
                    && text[i + EXTERNAL_HEADER.len()..].split('\n').next().is_some_and(|l| l.trim().is_empty())
            });
            match from {
                Some(i) => (js_trim(&text[i + EXTERNAL_HEADER.len()..]).to_string(), true),
                None => return Reading::Opaque,
            }
        }
    };
    let keep_include = mentions_include(&external_text);
    let external_requires = tokens.into_iter().filter(|t| t != "include" || keep_include).collect();
    Reading::Rules(Box::new(Parsed { meta, external_requires, external: external_text, differs }))
}

#[cfg(test)]
mod tests;
