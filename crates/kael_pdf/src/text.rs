//! Text extraction and search helpers.

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

pub(crate) fn search_text(page_index: usize, text: &str, query: &str) -> Vec<TextMatch> {
    if query.is_empty() {
        return Vec::new();
    }

    let needle = query.to_lowercase();
    let mut matches = Vec::new();

    for (line_index, line) in text.lines().enumerate() {
        let haystack = line.to_lowercase();
        let mut search_start = 0;

        while let Some(offset) = haystack[search_start..].find(&needle) {
            let start = search_start + offset;
            let end = start + needle.len();
            matches.push(TextMatch {
                page_index,
                line_index,
                start,
                end,
                snippet: line.to_string(),
            });
            search_start = end;
            if search_start >= haystack.len() {
                break;
            }
        }
    }

    matches
}