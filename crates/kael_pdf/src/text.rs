//! Text extraction and search helpers.

use std::collections::VecDeque;

use anyhow::Result;
use lopdf::Document;

/// A textual search match within a PDF page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextMatch {
    /// The zero-based page index containing the match.
    pub page_index: usize,
    /// The zero-based line index containing the match.
    pub line_index: usize,
    /// The byte offset where the match starts within the line.
    pub start: usize,
    /// The byte offset where the match ends within the line.
    pub end: usize,
    /// A short snippet containing the match.
    pub snippet: String,
}

pub(crate) fn extract_page_text(document: &Document, page_number: u32) -> Result<String> {
    let text = document.extract_text(&[page_number])?;
    Ok(text.replace('\r', ""))
}

pub(crate) fn search_text(page_index: usize, text: &str, query: &str) -> Result<Vec<TextMatch>> {
    const MAX_QUERY_BYTES: usize = 4 * 1024;
    const MAX_MATCHES: usize = 10_000;

    if query.is_empty() {
        return Ok(Vec::new());
    }
    anyhow::ensure!(
        query.len() <= MAX_QUERY_BYTES,
        "PDF search query exceeds the {MAX_QUERY_BYTES} byte limit"
    );

    let needle = query.to_lowercase();
    let prefix = match_prefixes(needle.as_bytes());
    let mut matches = Vec::new();

    for (line_index, line) in text.lines().enumerate() {
        let mut matcher = FoldedMatcher::new(needle.as_bytes(), &prefix);
        if line.is_ascii() && needle.is_ascii() {
            for (index, byte) in line.bytes().enumerate() {
                if let Some((start, end)) =
                    matcher.push(byte.to_ascii_lowercase(), index, index + 1)
                {
                    matches.push(TextMatch {
                        page_index,
                        line_index,
                        start,
                        end,
                        snippet: short_snippet(line, start, end),
                    });
                    if matches.len() >= MAX_MATCHES {
                        return Ok(matches);
                    }
                }
            }
        } else {
            for (start, character) in line.char_indices() {
                let end = start + character.len_utf8();
                for folded_character in character.to_lowercase() {
                    let mut buffer = [0; 4];
                    for byte in folded_character.encode_utf8(&mut buffer).bytes() {
                        if let Some((match_start, match_end)) = matcher.push(byte, start, end) {
                            matches.push(TextMatch {
                                page_index,
                                line_index,
                                start: match_start,
                                end: match_end,
                                snippet: short_snippet(line, match_start, match_end),
                            });
                            if matches.len() >= MAX_MATCHES {
                                return Ok(matches);
                            }
                        }
                    }
                }
            }
        }
    }

    Ok(matches)
}

fn match_prefixes(needle: &[u8]) -> Vec<usize> {
    let mut prefixes = vec![0; needle.len()];
    let mut matched = 0usize;
    for index in 1..needle.len() {
        while matched > 0 && needle[index] != needle[matched] {
            matched = prefixes[matched - 1];
        }
        if needle[index] == needle[matched] {
            matched += 1;
        }
        prefixes[index] = matched;
    }
    prefixes
}

struct FoldedMatcher<'a> {
    needle: &'a [u8],
    prefixes: &'a [usize],
    matched: usize,
    offsets: VecDeque<(usize, usize)>,
}

impl<'a> FoldedMatcher<'a> {
    fn new(needle: &'a [u8], prefixes: &'a [usize]) -> Self {
        Self {
            needle,
            prefixes,
            matched: 0,
            offsets: VecDeque::with_capacity(needle.len()),
        }
    }

    fn push(&mut self, byte: u8, start: usize, end: usize) -> Option<(usize, usize)> {
        self.offsets.push_back((start, end));
        if self.offsets.len() > self.needle.len() {
            self.offsets.pop_front();
        }

        while self.matched > 0 && byte != self.needle[self.matched] {
            self.matched = self.prefixes[self.matched - 1];
        }
        if byte == self.needle[self.matched] {
            self.matched += 1;
        }
        if self.matched != self.needle.len() {
            return None;
        }

        self.matched = 0;
        Some((self.offsets.front()?.0, self.offsets.back()?.1))
    }
}

fn short_snippet(line: &str, match_start: usize, match_end: usize) -> String {
    const CONTEXT_CHARACTERS: usize = 80;

    let start = line[..match_start]
        .char_indices()
        .rev()
        .nth(CONTEXT_CHARACTERS)
        .map_or(0, |(index, character)| index + character.len_utf8());
    let end = line[match_end..]
        .char_indices()
        .nth(CONTEXT_CHARACTERS)
        .map_or(line.len(), |(index, _)| match_end + index);
    let mut snippet = String::new();
    if start > 0 {
        snippet.push('…');
    }
    snippet.push_str(&line[start..end]);
    if end < line.len() {
        snippet.push('…');
    }
    snippet
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn case_folded_search_reports_original_utf8_offsets() {
        let matches = search_text(2, "A İSTANBUL line", "i\u{307}s").unwrap();
        assert_eq!(matches.len(), 1);
        assert_eq!(&"A İSTANBUL line"[matches[0].start..matches[0].end], "İS");
    }

    #[test]
    fn search_is_bounded_and_snippets_are_short() {
        let text = format!("{} needle {}", "a".repeat(1_000), "b".repeat(1_000));
        let matches = search_text(0, &text, "needle").unwrap();
        assert_eq!(matches.len(), 1);
        assert_eq!(&text[matches[0].start..matches[0].end], "needle");
        assert!(matches[0].snippet.chars().count() <= 170);
        assert!(search_text(0, "text", &"x".repeat(4_097)).is_err());
    }

    #[test]
    fn search_reports_non_overlapping_matches_without_line_sized_indexes() {
        let matches = search_text(0, "aaaa İSTANBUL", "aa").unwrap();
        assert_eq!(
            matches
                .iter()
                .map(|result| (result.start, result.end))
                .collect::<Vec<_>>(),
            vec![(0, 2), (2, 4)]
        );

        let unicode = search_text(0, "aaaa İSTANBUL", "i\u{307}s").unwrap();
        assert_eq!(&"aaaa İSTANBUL"[unicode[0].start..unicode[0].end], "İS");
    }
}
