//! Unicode-aware line wrapping for fixed-width text.
//!
//! [`wrap_text`] uses UAX #14 line-break opportunities and counts extended
//! grapheme clusters, so combining sequences and emoji stay intact. It still
//! assumes each grapheme occupies one cell; proportional layout should use the
//! renderer's shaped glyph advances instead.

use unicode_linebreak::linebreaks;
use unicode_segmentation::UnicodeSegmentation;

/// Greedily wrap `text` into lines of at most `max_width` grapheme clusters.
///
/// UAX #14 opportunities are preferred, `\n` forces a line break, blank lines
/// are preserved, and a segment with no legal opportunity is hard-split at a
/// grapheme boundary. Runs of ASCII spaces collapse for compatibility with the
/// original fixed-cell helper. A `max_width` of zero only splits on newlines.
pub fn wrap_text(text: &str, max_width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    for paragraph in text
        .split('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line))
    {
        if max_width == 0 {
            lines.push(paragraph.to_string());
            continue;
        }

        let normalized = collapse_ascii_spaces(paragraph);
        wrap_paragraph(&normalized, max_width, &mut lines);
    }
    lines
}

fn wrap_paragraph(paragraph: &str, max_width: usize, lines: &mut Vec<String>) {
    if paragraph.is_empty() {
        lines.push(String::new());
        return;
    }

    let mut remaining = paragraph;
    loop {
        let hard_end = remaining
            .grapheme_indices(true)
            .nth(max_width)
            .map_or(remaining.len(), |(index, _)| index);
        if hard_end == remaining.len() {
            lines.push(remaining.to_string());
            return;
        }

        let break_at = linebreaks(remaining)
            .map(|(index, _)| index)
            .take_while(|&index| index <= hard_end)
            .filter(|&index| index > 0)
            .last()
            .unwrap_or(hard_end);
        let (line, tail) = remaining.split_at(break_at);
        lines.push(line.trim_end_matches(' ').to_string());
        remaining = tail.trim_start_matches(' ');
        if remaining.is_empty() {
            return;
        }
    }
}

fn collapse_ascii_spaces(text: &str) -> String {
    let mut normalized = String::with_capacity(text.len());
    let mut after_space = true;
    for character in text.chars() {
        if character == ' ' {
            if !after_space {
                normalized.push(' ');
            }
            after_space = true;
        } else {
            normalized.push(character);
            after_space = false;
        }
    }
    if normalized.ends_with(' ') {
        normalized.pop();
    }
    normalized
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wraps_at_spaces_within_the_width() {
        assert_eq!(
            wrap_text("the quick brown fox", 10),
            ["the quick", "brown fox"]
        );
    }

    #[test]
    fn honors_mandatory_newlines_and_blank_lines() {
        assert_eq!(wrap_text("a\nb", 10), ["a", "b"]);
        assert_eq!(wrap_text("a\n\nb", 10), ["a", "", "b"]);
    }

    #[test]
    fn hard_splits_a_word_longer_than_the_width() {
        assert_eq!(wrap_text("abcdefghij", 4), ["abcd", "efgh", "ij"]);
        assert_eq!(wrap_text("hi abcdefghij", 4), ["hi", "abcd", "efgh", "ij"]);
    }

    #[test]
    fn uses_unicode_breaks_without_splitting_graphemes() {
        assert_eq!(wrap_text("日本語です", 3), ["日本語", "です"]);
        assert_eq!(wrap_text("a\u{301}b", 1), ["a\u{301}", "b"]);
        assert_eq!(wrap_text("👨‍👩‍👧‍👦x", 1), ["👨‍👩‍👧‍👦", "x"]);
    }

    #[test]
    fn zero_width_splits_only_on_newlines() {
        assert_eq!(wrap_text("a b\nc", 0), ["a b", "c"]);
    }

    #[test]
    fn collapses_runs_of_ascii_spaces() {
        assert_eq!(wrap_text("  a   b  ", 10), ["a b"]);
    }

    #[test]
    fn windows_newlines_do_not_leave_carriage_returns() {
        assert_eq!(wrap_text("a\r\nb", 10), ["a", "b"]);
    }
}
