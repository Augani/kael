//! Simplified UAX#14 line breaking for fixed-width text — wrapping the title/caption layer.
//!
//! [`wrap_text`] greedily wraps text to a maximum character width, breaking at spaces,
//! honoring `\n` as mandatory breaks, and hard-splitting a word that is longer than the
//! width. It assumes monospace cells (one column per `char`), which is what the broadcast
//! caption / lower-third text path uses; proportional shaping is a separate concern.

/// Greedily wrap `text` into lines of at most `max_width` characters. Words are broken only
/// at spaces (runs of spaces collapse); `\n` forces a line break (blank lines are preserved);
/// a single word longer than `max_width` is hard-split. A `max_width` of 0 only splits on
/// `\n`.
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
        let mut line = String::new();
        let mut line_len = 0usize;
        for mut word in paragraph.split(' ').filter(|word| !word.is_empty()) {
            // Hard-split a word that cannot fit on a line at all.
            while word.chars().count() > max_width {
                if line_len > 0 {
                    lines.push(std::mem::take(&mut line));
                    line_len = 0;
                }
                let split_at = word
                    .char_indices()
                    .nth(max_width)
                    .map_or(word.len(), |(index, _)| index);
                lines.push(word[..split_at].to_string());
                word = &word[split_at..];
            }

            let word_len = word.chars().count();
            if line_len == 0 {
                line = word.to_string();
                line_len = word_len;
            } else if line_len
                .checked_add(1)
                .and_then(|length| length.checked_add(word_len))
                .is_some_and(|length| length <= max_width)
            {
                line.push(' ');
                line.push_str(word);
                line_len += 1 + word_len;
            } else {
                lines.push(std::mem::take(&mut line));
                line = word.to_string();
                line_len = word_len;
            }
        }
        lines.push(line);
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wraps_at_spaces_within_the_width() {
        assert_eq!(
            wrap_text("the quick brown fox", 10),
            vec!["the quick".to_string(), "brown fox".to_string()]
        );
    }

    #[test]
    fn honors_mandatory_newlines_and_blank_lines() {
        assert_eq!(
            wrap_text("a\nb", 10),
            vec!["a".to_string(), "b".to_string()]
        );
        // A blank paragraph between newlines is preserved.
        assert_eq!(
            wrap_text("a\n\nb", 10),
            vec!["a".to_string(), String::new(), "b".to_string()]
        );
    }

    #[test]
    fn hard_splits_a_word_longer_than_the_width() {
        assert_eq!(
            wrap_text("abcdefghij", 4),
            vec!["abcd".to_string(), "efgh".to_string(), "ij".to_string()]
        );
        // A long word mid-line flushes the current line first.
        assert_eq!(
            wrap_text("hi abcdefghij", 4),
            vec![
                "hi".to_string(),
                "abcd".to_string(),
                "efgh".to_string(),
                "ij".to_string()
            ]
        );
    }

    #[test]
    fn zero_width_splits_only_on_newlines() {
        assert_eq!(
            wrap_text("a b\nc", 0),
            vec!["a b".to_string(), "c".to_string()]
        );
    }

    #[test]
    fn collapses_runs_of_spaces() {
        assert_eq!(wrap_text("a   b", 10), vec!["a b".to_string()]);
    }

    #[test]
    fn windows_newlines_do_not_leave_carriage_returns() {
        assert_eq!(wrap_text("a\r\nb", 10), vec!["a", "b"]);
    }
}
