//! Reading a streamed answer: server-sent events cut into `data` payloads, and the filter that
//! keeps a reasoning model's `<think>` block away from the user.

use crate::{Error, Result};

/// A line, or the `data` lines of one event together, longer than this is not a stream we
/// understand.
const MAX_LINE: usize = 4 * 1024 * 1024;
/// More than this much text hidden in `<think>` blocks is a model that does not stop.
pub const MAX_HIDDEN: usize = 500_000;

/// Cuts bytes, in whatever pieces the network delivers them, into the `data` of each event.
/// Lines end in `\n`, `\r\n` or `\r`; an empty line ends an event; comments and fields other
/// than `data` are ignored.
#[derive(Default)]
pub struct SseParser {
    line: Vec<u8>,
    data: Vec<String>,
    /// Bytes in `data`.
    data_len: usize,
    after_cr: bool,
}

impl SseParser {
    /// The events that `bytes` completed.
    pub fn feed(&mut self, bytes: &[u8]) -> Result<Vec<String>> {
        let mut events = Vec::new();
        for &b in bytes {
            if std::mem::take(&mut self.after_cr) && b == b'\n' {
                continue;
            }
            match b {
                b'\r' => {
                    self.after_cr = true;
                    self.end_line(&mut events)?;
                }
                b'\n' => self.end_line(&mut events)?,
                _ => {
                    self.line.push(b);
                    if self.line.len() > MAX_LINE {
                        return Err(Error::temporary("the service sent something too large to read"));
                    }
                }
            }
        }
        Ok(events)
    }

    /// The stream ended: an event that was not closed by an empty line still counts.
    pub fn finish(&mut self) -> Result<Option<String>> {
        let mut events = Vec::new();
        if !self.line.is_empty() {
            self.end_line(&mut events)?;
        }
        self.end_line(&mut events)?;
        Ok(events.pop())
    }

    fn end_line(&mut self, events: &mut Vec<String>) -> Result<()> {
        let line = std::mem::take(&mut self.line);
        if line.is_empty() {
            if !self.data.is_empty() {
                events.push(std::mem::take(&mut self.data).join("\n"));
                self.data_len = 0;
            }
            return Ok(());
        }
        let line = String::from_utf8_lossy(&line);
        if let Some(rest) = line.strip_prefix("data:") {
            self.data_len += rest.len();
            if self.data_len > MAX_LINE {
                return Err(Error::temporary("the service sent something too large to read"));
            }
            self.data.push(rest.strip_prefix(' ').unwrap_or(rest).to_string());
        }
        Ok(())
    }
}

const OPEN: &str = "<think>";
const CLOSE: &str = "</think>";

/// Drops a leading `<think>…</think>` block from a stream of text pieces, also when a tag is
/// split between two pieces. Only a block that comes before any visible text counts: after the
/// first word, `<think>` is the model's own text (a mail may talk about the tag, and must not be
/// able to swallow the rest of the answer). What is inside is never shown, even when the closing
/// tag never comes.
#[derive(Default)]
pub struct ThinkFilter {
    pending: String,
    inside: bool,
    /// Visible text has begun; nothing is looked for any more.
    started: bool,
    /// An opening tag was seen.
    opened: bool,
    /// Characters held back inside blocks so far.
    pub hidden: usize,
}

impl ThinkFilter {
    /// What of `piece` may be shown now. A possible start of a tag is held back until the next
    /// piece says whether it is one.
    pub fn push(&mut self, piece: &str) -> String {
        self.pending.push_str(piece);
        loop {
            if self.started {
                return std::mem::take(&mut self.pending);
            }
            if self.inside {
                if let Some(at) = self.pending.find(CLOSE) {
                    self.hidden += at;
                    self.pending.drain(..at + CLOSE.len());
                    self.inside = false;
                    continue;
                }
                let ready = self.pending.len() - partial_tag_at_end(&self.pending, CLOSE);
                self.hidden += ready;
                self.pending.drain(..ready);
                return String::new();
            }
            let rest = self.pending.trim_start();
            if rest.is_empty() {
                self.pending.clear();
                return String::new();
            }
            if rest.starts_with(OPEN) {
                let cut = self.pending.len() - rest.len() + OPEN.len();
                self.pending.drain(..cut);
                (self.inside, self.opened) = (true, true);
            } else if rest.len() < OPEN.len() && OPEN.starts_with(rest) {
                return String::new(); // may still turn out to be the tag
            } else {
                self.started = true;
            }
        }
    }

    /// The stream ended: what was held back is text after all, unless it is inside a block.
    pub fn finish(&mut self) -> String {
        let rest = std::mem::take(&mut self.pending);
        if self.inside {
            String::new()
        } else {
            rest
        }
    }

    /// Some servers (a chat template that already wrote the opening tag) stream the reasoning
    /// and then `</think>` with no opening tag. When none was seen, and no `<think>` comes before
    /// the first closing tag (so the answer is not just talking about the tags), what comes
    /// after that tag is the answer.
    pub fn after_orphan_close<'a>(&self, answer: &'a str) -> &'a str {
        match answer.split_once(CLOSE) {
            Some((before, after)) if !self.opened && !before.contains(OPEN) && !after.trim().is_empty() => after.trim(),
            _ => answer,
        }
    }
}

/// The length of the longest end of `text` that is the start of `tag` (a tag is ASCII).
fn partial_tag_at_end(text: &str, tag: &str) -> usize {
    (1..tag.len())
        .rev()
        .find(|&n| text.len() >= n && text.is_char_boundary(text.len() - n) && tag.starts_with(&text[text.len() - n..]))
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(pieces: &[&[u8]]) -> Vec<String> {
        let mut parser = SseParser::default();
        let mut out = Vec::new();
        for p in pieces {
            out.extend(parser.feed(p).unwrap());
        }
        out.extend(parser.finish().unwrap());
        out
    }

    #[test]
    fn events_are_found_however_the_bytes_are_cut() {
        let whole = b"data: one\n\ndata: two\ndata: lines\n\n: comment\nevent: x\ndata:three\n\n";
        let expect = ["one", "two\nlines", "three"];
        assert_eq!(parse(&[whole]), expect);
        let bytes: Vec<&[u8]> = whole.chunks(1).collect();
        assert_eq!(parse(&bytes), expect);
        assert_eq!(parse(&[b"data: a\r\n\r\ndata: b\r\rdata: c\r\n\r\n"]), ["a", "b", "c"]);
    }

    #[test]
    fn a_character_cut_in_the_middle_survives() {
        let text = "data: h\u{e9}llo \u{1f600}\n\n".as_bytes();
        for cut in 1..text.len() {
            assert_eq!(parse(&[&text[..cut], &text[cut..]]), ["h\u{e9}llo \u{1f600}"], "cut at {cut}");
        }
    }

    #[test]
    fn an_event_not_closed_at_the_end_still_counts() {
        assert_eq!(parse(&[b"data: last"]), ["last"]);
        assert_eq!(parse(&[b"data: a\n\ndata: b\n"]), ["a", "b"]);
        assert!(parse(&[b": only a comment\n\n"]).is_empty());
    }

    #[test]
    fn an_event_of_endless_data_lines_is_refused() {
        let mut parser = SseParser::default();
        let line = format!("data: {}\n", "a".repeat(1000 * 1000));
        let result = (0..5).try_for_each(|_| parser.feed(line.as_bytes()).map(drop));
        assert!(matches!(result, Err(Error::Temporary { .. })));
        // Events that end properly start counting afresh.
        let mut parser = SseParser::default();
        for _ in 0..10 {
            parser.feed(format!("{line}\n").as_bytes()).unwrap();
        }
    }

    #[test]
    fn an_endless_line_is_refused() {
        let mut parser = SseParser::default();
        let chunk = vec![b'a'; 1024 * 1024];
        let result = (0..5).try_for_each(|_| parser.feed(&chunk).map(drop));
        assert!(matches!(result, Err(Error::Temporary { .. })));
    }

    fn filtered(pieces: &[&str]) -> String {
        let mut f = ThinkFilter::default();
        let mut out: String = pieces.iter().map(|p| f.push(p)).collect();
        out.push_str(&f.finish());
        out
    }

    #[test]
    fn a_think_block_before_the_answer_is_dropped_whole_or_split_anywhere() {
        let text = "\n <think>secret</think>\n\nb c";
        assert_eq!(filtered(&[text]).trim(), "b c");
        let chars: Vec<String> = text.chars().map(String::from).collect();
        let pieces: Vec<&str> = chars.iter().map(String::as_str).collect();
        assert_eq!(filtered(&pieces).trim(), "b c");
        for cut in 1..text.len() {
            assert_eq!(filtered(&[&text[..cut], &text[cut..]]).trim(), "b c", "cut at {cut}");
        }
        // Two blocks in a row, with only blank space between.
        assert_eq!(filtered(&["<think>x</think> <think>y</think>one"]), "one");
    }

    #[test]
    fn a_block_that_never_closes_stays_hidden_and_a_lookalike_is_text() {
        assert_eq!(filtered(&["<think>never ", "closed"]), "");
        assert_eq!(filtered(&["<thin", "g> 1"]), "<thing> 1");
        assert_eq!(filtered(&["<thi"]), "<thi");
        assert_eq!(filtered(&["<", "\u{e9}"]), "<\u{e9}");
    }

    #[test]
    fn the_tag_in_the_answer_itself_is_text() {
        // After the first word the tag is what the model wrote, not reasoning.
        assert_eq!(
            filtered(&["Wrap it in <think> tags", " and </think> too."]),
            "Wrap it in <think> tags and </think> too."
        );
        assert_eq!(filtered(&["one<think>y</think> two"]), "one<think>y</think> two");
    }

    #[test]
    fn hidden_text_is_counted_and_a_missing_opening_tag_is_dealt_with_at_the_end() {
        let mut f = ThinkFilter::default();
        f.push("<think>12345");
        f.push("678</think>");
        assert_eq!(f.hidden, 8);
        assert_eq!(f.after_orphan_close("a </think> b"), "a </think> b", "an opened block means it is text");
        assert_eq!(ThinkFilter::default().after_orphan_close("reasoning</think>\nAnswer"), "Answer");
        assert_eq!(ThinkFilter::default().after_orphan_close("done</think>"), "done</think>");
        let talk = "Use <think> to open and </think> to close.";
        assert_eq!(ThinkFilter::default().after_orphan_close(talk), talk);
    }
}
