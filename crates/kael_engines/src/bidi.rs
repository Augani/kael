//! Unicode bidirectional text foundation (subset of UAX #9).
//!
//! This resolves the *direction* of text — the part needed before glyph shaping:
//! classify each character's strong direction, pick the paragraph base direction
//! (UAX #9 rules P2/P3, exact), and segment a string into directional runs using N1-style
//! neutral resolution. Full weak-type resolution (W1–W7), explicit isolates/embeddings,
//! and glyph shaping are out of scope here.

/// A text direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    /// Left-to-right.
    Ltr,
    /// Right-to-left.
    Rtl,
}

/// The strong direction of `ch`, or `None` for neutral/weak characters (digits,
/// punctuation, whitespace, symbols).
pub fn strong_direction(ch: char) -> Option<Direction> {
    if is_rtl_char(ch) {
        Some(Direction::Rtl)
    } else if ch.is_alphabetic() {
        Some(Direction::Ltr)
    } else {
        None
    }
}

/// Whether `ch` has strong right-to-left directionality (Hebrew, Arabic, Syriac,
/// Thaana, N'Ko, and the Hebrew/Arabic presentation forms).
pub fn is_rtl_char(ch: char) -> bool {
    let code = ch as u32;
    matches!(code,
        0x0590..=0x05FF // Hebrew
        | 0x0600..=0x06FF // Arabic
        | 0x0700..=0x074F // Syriac
        | 0x0750..=0x077F // Arabic Supplement
        | 0x0780..=0x07BF // Thaana
        | 0x07C0..=0x07FF // N'Ko
        | 0x08A0..=0x08FF // Arabic Extended-A
        | 0xFB1D..=0xFB4F // Hebrew presentation forms
        | 0xFB50..=0xFDFF // Arabic presentation forms-A
        | 0xFE70..=0xFEFF // Arabic presentation forms-B
    )
}

/// The paragraph base direction (UAX #9 P2/P3): the direction of the first strong
/// character, defaulting to left-to-right when there is none.
pub fn base_direction(text: &str) -> Direction {
    text.chars()
        .find_map(strong_direction)
        .unwrap_or(Direction::Ltr)
}

/// A maximal slice of text resolved to a single direction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectionalRun {
    /// The run's text, in logical order.
    pub text: String,
    /// The resolved direction of the run.
    pub direction: Direction,
}

/// Segment `text` into directional runs against the `base` direction.
///
/// Strong characters take their own direction; a neutral run takes the direction of the
/// strong characters on both sides when they agree (UAX #9 rule N1), otherwise the base
/// direction. Returns runs in logical order.
pub fn segment_runs(text: &str, base: Direction) -> Vec<DirectionalRun> {
    let chars: Vec<char> = text.chars().collect();
    if chars.is_empty() {
        return Vec::new();
    }
    let strong: Vec<Option<Direction>> = chars.iter().map(|&ch| strong_direction(ch)).collect();

    let mut runs: Vec<DirectionalRun> = Vec::new();
    for (index, &ch) in chars.iter().enumerate() {
        let direction = strong[index].unwrap_or_else(|| {
            let previous = strong[..index].iter().rev().find_map(|entry| *entry);
            let next = strong[index + 1..].iter().find_map(|entry| *entry);
            match (previous, next) {
                (Some(left), Some(right)) if left == right => left,
                _ => base,
            }
        });
        match runs.last_mut() {
            Some(run) if run.direction == direction => run.text.push(ch),
            _ => runs.push(DirectionalRun {
                text: ch.to_string(),
                direction,
            }),
        }
    }
    runs
}

#[cfg(test)]
mod tests {
    use super::*;

    // Hebrew "shalom" and Arabic "salam".
    const HEBREW: &str = "שלום";
    const ARABIC: &str = "سلام";

    #[test]
    fn classifies_strong_directions() {
        assert_eq!(strong_direction('a'), Some(Direction::Ltr));
        assert_eq!(strong_direction('Ж'), Some(Direction::Ltr)); // Cyrillic
        assert_eq!(strong_direction('語'), Some(Direction::Ltr)); // CJK is L
        assert_eq!(strong_direction('א'), Some(Direction::Rtl)); // Hebrew aleph
        assert_eq!(strong_direction('ا'), Some(Direction::Rtl)); // Arabic alef
        assert_eq!(strong_direction('5'), None);
        assert_eq!(strong_direction(' '), None);
        assert_eq!(strong_direction('!'), None);
    }

    #[test]
    fn base_direction_uses_first_strong_character() {
        assert_eq!(base_direction("hello"), Direction::Ltr);
        assert_eq!(base_direction(HEBREW), Direction::Rtl);
        // Leading neutrals are skipped to the first strong character.
        assert_eq!(base_direction("123 hello"), Direction::Ltr);
        assert_eq!(base_direction("  \"שלום\""), Direction::Rtl);
        // No strong character -> default LTR (P3).
        assert_eq!(base_direction("123 !!!"), Direction::Ltr);
        assert_eq!(base_direction(""), Direction::Ltr);
    }

    #[test]
    fn segments_pure_runs() {
        let runs = segment_runs("abc", Direction::Ltr);
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].direction, Direction::Ltr);
        assert_eq!(runs[0].text, "abc");

        let runs = segment_runs(HEBREW, Direction::Rtl);
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].direction, Direction::Rtl);
    }

    #[test]
    fn n1_keeps_neutrals_between_matching_strongs() {
        // "a 1 b": the space+digit between two L characters resolve to L (one run).
        let runs = segment_runs("a 1 b", Direction::Rtl);
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].direction, Direction::Ltr);
        assert_eq!(runs[0].text, "a 1 b");
    }

    #[test]
    fn mixed_script_splits_into_runs() {
        // Latin then Hebrew, base LTR: the separating space (between L and R) takes the
        // base (LTR), so it stays with the Latin run.
        let input = format!("abc {HEBREW}");
        let runs = segment_runs(&input, Direction::Ltr);
        assert_eq!(runs.len(), 2);
        assert_eq!(runs[0].direction, Direction::Ltr);
        assert_eq!(runs[0].text, "abc ");
        assert_eq!(runs[1].direction, Direction::Rtl);
        assert_eq!(runs[1].text, HEBREW);
    }

    #[test]
    fn arabic_is_right_to_left() {
        assert_eq!(base_direction(ARABIC), Direction::Rtl);
        let runs = segment_runs(ARABIC, Direction::Rtl);
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].direction, Direction::Rtl);
    }

    #[test]
    fn empty_text_has_no_runs() {
        assert!(segment_runs("", Direction::Ltr).is_empty());
    }
}
