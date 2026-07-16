//! Text extraction and search helpers.

use anyhow::{Result, anyhow};
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
    let mut matches = Vec::new();

    for (line_index, line) in text.lines().enumerate() {
        let (haystack, offsets) = folded_line_with_offsets(line);
        let mut search_start = 0;

        while let Some(offset) = haystack[search_start..].find(&needle) {
            let folded_start = search_start + offset;
            let folded_end = folded_start + needle.len();
            let start = offsets
                .get(folded_start)
                .map(|offset| offset.0)
                .ok_or_else(|| anyhow!("invalid folded PDF search offset"))?;
            let end = offsets
                .get(folded_end.saturating_sub(1))
                .map(|offset| offset.1)
                .ok_or_else(|| anyhow!("invalid folded PDF search end offset"))?;
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
            search_start = folded_end;
            if search_start >= haystack.len() {
                break;
            }
        }
    }

    Ok(matches)
}

fn folded_line_with_offsets(line: &str) -> (String, Vec<(usize, usize)>) {
    let mut folded = String::with_capacity(line.len());
    let mut offsets = Vec::with_capacity(line.len());
    for (start, character) in line.char_indices() {
        let end = start + character.len_utf8();
        for folded_character in character.to_lowercase() {
            let mut buffer = [0; 4];
            let encoded = folded_character.encode_utf8(&mut buffer);
            folded.push_str(encoded);
            offsets.extend(std::iter::repeat_n((start, end), encoded.len()));
        }
    }
    (folded, offsets)
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
        assert!(matches[0].snippet.chars().count() <= 170);
        assert!(search_text(0, "text", &"x".repeat(4_097)).is_err());
    }
}
