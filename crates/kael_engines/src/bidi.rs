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
    match unicode_bidi::bidi_class(ch) {
        unicode_bidi::BidiClass::L => Some(Direction::Ltr),
        unicode_bidi::BidiClass::R | unicode_bidi::BidiClass::AL => Some(Direction::Rtl),
        _ => None,
    }
}

/// Whether `ch` has strong right-to-left directionality (Hebrew, Arabic, Syriac,
/// Thaana, N'Ko, and the Hebrew/Arabic presentation forms).
pub fn is_rtl_char(ch: char) -> bool {
    matches!(
        unicode_bidi::bidi_class(ch),
        unicode_bidi::BidiClass::R | unicode_bidi::BidiClass::AL
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

/// Reorder logical-order `runs` into visual (display) order against `base`, applying the
/// UAX #9 rule L2 at the run level: assign each run an embedding level (even for the base
/// parity, odd for the opposite), reverse the run sequence within each level from highest
/// down to the lowest odd level, and reverse the characters of each right-to-left run.
///
/// Exact for run-level reordering; it does not handle per-character weak-type levels.
pub fn reorder_visual(runs: &[DirectionalRun], base: Direction) -> Vec<DirectionalRun> {
    if runs.is_empty() {
        return Vec::new();
    }
    let base_level: u8 = if base == Direction::Ltr { 0 } else { 1 };
    let levels: Vec<u8> = runs
        .iter()
        .map(|run| {
            let run_parity = u8::from(run.direction == Direction::Rtl);
            if run_parity == base_level % 2 {
                base_level
            } else {
                base_level + 1
            }
        })
        .collect();

    let mut order: Vec<usize> = (0..runs.len()).collect();
    let max_level = levels.iter().copied().max().unwrap_or(0);
    let min_odd = levels
        .iter()
        .copied()
        .filter(|level| level % 2 == 1)
        .min()
        .unwrap_or(max_level + 1);

    let mut level = max_level;
    while level >= min_odd {
        let mut index = 0;
        while index < order.len() {
            if levels[order[index]] >= level {
                let start = index;
                while index < order.len() && levels[order[index]] >= level {
                    index += 1;
                }
                order[start..index].reverse();
            } else {
                index += 1;
            }
        }
        level -= 1;
    }

    order
        .into_iter()
        .map(|index| {
            let run = &runs[index];
            let text = if levels[index] % 2 == 1 {
                run.text.chars().rev().collect()
            } else {
                run.text.clone()
            };
            DirectionalRun {
                text,
                direction: run.direction,
            }
        })
        .collect()
}

/// A UAX #9 bidirectional character type (the subset used by the weak-type rules).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BidiClass {
    /// Strong left-to-right.
    L,
    /// Strong right-to-left.
    R,
    /// Strong right-to-left Arabic letter.
    Al,
    /// European number.
    En,
    /// European number separator (`+`, `-`).
    Es,
    /// European number terminator (`#`, `$`, `%`, currency).
    Et,
    /// Arabic number.
    An,
    /// Common number separator (`,`, `.`, `:`).
    Cs,
    /// Non-spacing mark.
    Nsm,
    /// Other neutral.
    On,
}

/// Classify a character into its [`BidiClass`] (common ranges; approximate, used as the
/// input to [`resolve_weak_types`]).
pub fn bidi_class(ch: char) -> BidiClass {
    match unicode_bidi::bidi_class(ch) {
        unicode_bidi::BidiClass::L => BidiClass::L,
        unicode_bidi::BidiClass::R => BidiClass::R,
        unicode_bidi::BidiClass::AL => BidiClass::Al,
        unicode_bidi::BidiClass::EN => BidiClass::En,
        unicode_bidi::BidiClass::ES => BidiClass::Es,
        unicode_bidi::BidiClass::ET => BidiClass::Et,
        unicode_bidi::BidiClass::AN => BidiClass::An,
        unicode_bidi::BidiClass::CS => BidiClass::Cs,
        unicode_bidi::BidiClass::NSM => BidiClass::Nsm,
        _ => BidiClass::On,
    }
}

/// Resolve weak types in a single-level character sequence per UAX #9 rules W1–W7.
///
/// Operates on the class sequence directly (so the rules are testable independent of
/// classification) using the paragraph `base` as the start-of-run strong type. Numbers
/// (EN/AN), separators (ES/CS), terminators (ET), and marks (NSM) are resolved; the
/// result feeds neutral resolution and reordering.
pub fn resolve_weak_types(classes: &[BidiClass], base: Direction) -> Vec<BidiClass> {
    let sor = if base == Direction::Rtl {
        BidiClass::R
    } else {
        BidiClass::L
    };
    let mut types = classes.to_vec();

    // W1: NSM takes the type of the previous character (start-of-run if first).
    for index in 0..types.len() {
        if types[index] == BidiClass::Nsm {
            types[index] = if index == 0 { sor } else { types[index - 1] };
        }
    }

    // W2: EN becomes AN when the last strong type is AL.
    let mut last_strong = sor;
    for class in &mut types {
        match *class {
            BidiClass::R | BidiClass::L | BidiClass::Al => last_strong = *class,
            BidiClass::En if last_strong == BidiClass::Al => *class = BidiClass::An,
            _ => {}
        }
    }

    // W3: AL becomes R.
    for class in &mut types {
        if *class == BidiClass::Al {
            *class = BidiClass::R;
        }
    }

    // W4: a single ES between two EN, or a single CS between two numbers of the same
    // kind, joins them.
    for index in 1..types.len().saturating_sub(1) {
        let (prev, next) = (types[index - 1], types[index + 1]);
        types[index] = match types[index] {
            BidiClass::Es if prev == BidiClass::En && next == BidiClass::En => BidiClass::En,
            BidiClass::Cs if prev == BidiClass::En && next == BidiClass::En => BidiClass::En,
            BidiClass::Cs if prev == BidiClass::An && next == BidiClass::An => BidiClass::An,
            other => other,
        };
    }

    // W5: a run of ET adjacent to EN becomes EN.
    let len = types.len();
    let mut index = 0;
    while index < len {
        if types[index] == BidiClass::Et {
            let start = index;
            while index < len && types[index] == BidiClass::Et {
                index += 1;
            }
            let touches_en = (start > 0 && types[start - 1] == BidiClass::En)
                || (index < len && types[index] == BidiClass::En);
            if touches_en {
                for class in &mut types[start..index] {
                    *class = BidiClass::En;
                }
            }
        } else {
            index += 1;
        }
    }

    // W6: remaining separators and terminators become neutral.
    for class in &mut types {
        if matches!(*class, BidiClass::Es | BidiClass::Et | BidiClass::Cs) {
            *class = BidiClass::On;
        }
    }

    // W7: EN becomes L when the last strong type is L.
    let mut last_strong = sor;
    for class in &mut types {
        match *class {
            BidiClass::R | BidiClass::L => last_strong = *class,
            BidiClass::En if last_strong == BidiClass::L => *class = BidiClass::L,
            _ => {}
        }
    }

    types
}

/// The Bidi_Mirroring_Glyph of `ch` for UAX#9 rule L4: in a right-to-left run, paired
/// punctuation is replaced by its mirror image (e.g. `(` ↔ `)`, `«` ↔ `»`). Characters with
/// no mirror are returned unchanged. Covers the common bracket, angle, guillemet, and
/// comparison pairs.
pub fn mirror_char(ch: char) -> char {
    match ch {
        '(' => ')',
        ')' => '(',
        '[' => ']',
        ']' => '[',
        '{' => '}',
        '}' => '{',
        '<' => '>',
        '>' => '<',
        '\u{00AB}' => '\u{00BB}', // « »
        '\u{00BB}' => '\u{00AB}',
        '\u{2039}' => '\u{203A}', // ‹ ›
        '\u{203A}' => '\u{2039}',
        '\u{2264}' => '\u{2265}', // ≤ ≥
        '\u{2265}' => '\u{2264}',
        '\u{2308}' => '\u{2309}', // ⌈ ⌉
        '\u{2309}' => '\u{2308}',
        '\u{230A}' => '\u{230B}', // ⌊ ⌋
        '\u{230B}' => '\u{230A}',
        '\u{27E8}' => '\u{27E9}', // ⟨ ⟩
        '\u{27E9}' => '\u{27E8}',
        other => other,
    }
}

/// Whether `ch` has a Bidi mirror (a distinct [`mirror_char`]).
pub fn is_mirrored(ch: char) -> bool {
    mirror_char(ch) != ch
}

/// Reorder each paragraph in `text` into visual order using the complete UAX #9
/// level resolution implemented by `unicode-bidi`.
///
/// This string helper applies rules through L2. It intentionally does not apply
/// combining-mark adjustment (L3), mirrored glyph selection (L4), shaping, or
/// cursor mapping; production text renderers should consume resolved levels and
/// perform those steps while shaping glyphs. Pure left-to-right text is returned
/// unchanged.
pub fn display_order(text: &str) -> String {
    let info = unicode_bidi::BidiInfo::new(text, None);
    let mut output = String::with_capacity(text.len());
    for paragraph in &info.paragraphs {
        output.push_str(&info.reorder_line(paragraph, paragraph.range.clone()));
    }
    output
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
        assert_eq!(strong_direction('\u{1E900}'), Some(Direction::Rtl)); // Adlam
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

    fn visual(text: &str, base: Direction) -> String {
        reorder_visual(&segment_runs(text, base), base)
            .iter()
            .map(|run| run.text.as_str())
            .collect()
    }

    #[test]
    fn reorder_pure_ltr_is_unchanged() {
        assert_eq!(visual("abc", Direction::Ltr), "abc");
    }

    #[test]
    fn reorder_reverses_single_rtl_run() {
        let expected: String = HEBREW.chars().rev().collect();
        assert_eq!(visual(HEBREW, Direction::Rtl), expected);
    }

    #[test]
    fn reorder_ltr_base_with_embedded_rtl() {
        let hebrew_rev: String = HEBREW.chars().rev().collect();
        assert_eq!(
            visual(&format!("abc{HEBREW}"), Direction::Ltr),
            format!("abc{hebrew_rev}")
        );
    }

    #[test]
    fn reorder_rtl_base_with_embedded_ltr() {
        // Logical Hebrew-aleph + "bc" in an RTL paragraph displays as "bc" then aleph.
        assert_eq!(visual("אbc", Direction::Rtl), "bcא");
    }

    #[test]
    fn reorder_empty_is_empty() {
        assert!(reorder_visual(&[], Direction::Ltr).is_empty());
    }

    use BidiClass::*;

    #[test]
    fn bidi_class_classifies_numbers_and_letters() {
        assert_eq!(bidi_class('5'), En);
        assert_eq!(bidi_class('\u{0665}'), An); // Arabic-Indic five
        assert_eq!(bidi_class('ا'), Al); // Arabic alef
        assert_eq!(bidi_class('א'), R); // Hebrew aleph
        assert_eq!(bidi_class('a'), L);
        assert_eq!(bidi_class('+'), Es);
        assert_eq!(bidi_class(','), Cs);
        assert_eq!(bidi_class('$'), Et);
    }

    #[test]
    fn w1_nsm_takes_previous_type() {
        assert_eq!(resolve_weak_types(&[L, Nsm], Direction::Ltr), vec![L, L]);
        // Leading NSM takes the start-of-run type (base).
        assert_eq!(resolve_weak_types(&[Nsm], Direction::Rtl), vec![R]);
    }

    #[test]
    fn w2_w3_arabic_number_and_letter_resolution() {
        // AL EN -> (W2) AL AN -> (W3) R AN.
        assert_eq!(resolve_weak_types(&[Al, En], Direction::Rtl), vec![R, An]);
        // A lone AL becomes R.
        assert_eq!(resolve_weak_types(&[Al], Direction::Rtl), vec![R]);
    }

    #[test]
    fn w4_single_separator_joins_numbers() {
        // Base RTL so W7 leaves EN intact, isolating W4.
        assert_eq!(
            resolve_weak_types(&[En, Es, En], Direction::Rtl),
            vec![En, En, En]
        );
        assert_eq!(
            resolve_weak_types(&[En, Cs, En], Direction::Rtl),
            vec![En, En, En]
        );
        assert_eq!(
            resolve_weak_types(&[An, Cs, An], Direction::Rtl),
            vec![An, An, An]
        );
        // Double separators are not joined (then W6 -> ON).
        assert_eq!(
            resolve_weak_types(&[En, Es, Es, En], Direction::Rtl),
            vec![En, On, On, En]
        );
    }

    #[test]
    fn w5_terminators_adjacent_to_numbers() {
        assert_eq!(resolve_weak_types(&[Et, En], Direction::Rtl), vec![En, En]);
        assert_eq!(
            resolve_weak_types(&[En, Et, Et], Direction::Rtl),
            vec![En, En, En]
        );
        // A terminator with no adjacent number falls through to W6 -> ON.
        assert_eq!(resolve_weak_types(&[Et], Direction::Rtl), vec![On]);
    }

    #[test]
    fn w7_european_number_after_left_becomes_left() {
        assert_eq!(resolve_weak_types(&[L, En], Direction::Ltr), vec![L, L]);
        // After a strong R the European number stays EN.
        assert_eq!(resolve_weak_types(&[R, En], Direction::Rtl), vec![R, En]);
    }

    #[test]
    fn mirror_char_swaps_paired_punctuation() {
        assert_eq!(mirror_char('('), ')');
        assert_eq!(mirror_char(')'), '(');
        assert_eq!(mirror_char('['), ']');
        assert_eq!(mirror_char('<'), '>');
        assert_eq!(mirror_char('\u{00AB}'), '\u{00BB}'); // « -> »
        assert_eq!(mirror_char('\u{2265}'), '\u{2264}'); // ≥ -> ≤
        // Mirroring is an involution.
        for ch in ['(', '[', '{', '<', '\u{00AB}', '\u{2039}', '\u{27E8}'] {
            assert_eq!(mirror_char(mirror_char(ch)), ch);
        }
    }

    #[test]
    fn non_mirrored_chars_are_unchanged() {
        assert_eq!(mirror_char('a'), 'a');
        assert_eq!(mirror_char('5'), '5');
        assert_eq!(mirror_char('\u{0627}'), '\u{0627}'); // Arabic alef has no mirror
        assert!(!is_mirrored('a'));
        assert!(is_mirrored('('));
    }

    #[test]
    fn display_order_leaves_ltr_unchanged() {
        assert_eq!(display_order("abc(def)"), "abc(def)");
        assert_eq!(display_order(""), "");
    }

    #[test]
    fn display_order_reverses_a_pure_rtl_run() {
        // A pure right-to-left paragraph reads reversed in left-to-right display order.
        let expected: String = HEBREW.chars().rev().collect();
        assert_eq!(display_order(HEBREW), expected);
    }

    #[test]
    fn display_order_leaves_mirroring_to_the_renderer() {
        let input = format!("({HEBREW})");
        let expected = format!("){}(", HEBREW.chars().rev().collect::<String>());
        assert_eq!(display_order(&input), expected);
    }
}
