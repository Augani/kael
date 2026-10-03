//! Immutable text-run metadata for native selection and text-range providers.

use super::{AccessibilityId, AccessibilityIdRange, AccessibilityRect};
use crate::BackgroundExecutor;
pub use accesskit::TextDirection as AccessibilityTextDirection;
use std::{ops::Range, sync::Arc};
use unicode_segmentation::UnicodeSegmentation;

/// Native suggested placement when revealing text. This never changes selection.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AccessibilityTextAlignment {
    /// Move only enough to expose the run.
    #[default]
    Nearest,
    /// Align vertically at the top edge.
    Top,
    /// Align vertically at the bottom edge.
    Bottom,
    /// Align horizontally at the left edge.
    Left,
    /// Align horizontally at the right edge.
    Right,
    /// Align at the top-left corner.
    TopLeft,
    /// Align at the bottom-right corner.
    BottomRight,
}
impl AccessibilityTextAlignment {
    pub(super) fn from_data(data: Option<accesskit::ActionData>) -> Self {
        match data {
            Some(accesskit::ActionData::ScrollHint(hint)) => match hint {
                accesskit::ScrollHint::TopEdge => Self::Top,
                accesskit::ScrollHint::BottomEdge => Self::Bottom,
                accesskit::ScrollHint::LeftEdge => Self::Left,
                accesskit::ScrollHint::RightEdge => Self::Right,
                accesskit::ScrollHint::TopLeft => Self::TopLeft,
                accesskit::ScrollHint::BottomRight => Self::BottomRight,
            },
            _ => Self::Nearest,
        }
    }
}

/// Actual layout of one immutable text run, in window logical pixels.
/// Arrays have one entry per selectable unit, including the hard line break.
#[derive(Clone, Debug, PartialEq)]
pub struct AccessibilityTextRunGeometry {
    /// Run identity from the exact immutable document.
    pub run_id: AccessibilityId,
    /// Actual shaped run rectangle in window coordinates.
    pub bounds: AccessibilityRect,
    /// Direction of the provided character coordinates.
    pub direction: accesskit::TextDirection,
    /// Relative unit positions along the declared direction.
    pub character_positions: Arc<[f32]>,
    /// Nonnegative advance widths for the same units.
    pub character_widths: Arc<[f32]>,
}

/// A bounded viewport overlay. Complete text remains in its immutable document;
/// offscreen runs have no invented geometry. Replace the overlay on layout or
/// scroll changes and retain its Arc across caret-only redraws.
#[derive(Debug)]
pub struct AccessibilityTextGeometry {
    document_id: AccessibilityId,
    runs: Vec<AccessibilityTextRunGeometry>,
}
impl PartialEq for AccessibilityTextGeometry {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::eq(self, other)
            || (self.document_id == other.document_id && self.runs == other.runs)
    }
}
impl AccessibilityTextGeometry {
    /// Validate actual finite geometry and exact unit counts before exporting.
    pub fn new(
        document: &AccessibilityTextDocument,
        mut runs: Vec<AccessibilityTextRunGeometry>,
    ) -> anyhow::Result<Arc<Self>> {
        runs.sort_unstable_by_key(|run| run.run_id.0);
        for (index, run) in runs.iter().enumerate() {
            let units = document
                .run(run.run_id)
                .ok_or_else(|| anyhow::anyhow!("text geometry belongs to a different document"))?
                .lengths
                .len();
            anyhow::ensure!(
                index == 0 || runs[index - 1].run_id != run.run_id,
                "duplicate text run geometry"
            );
            anyhow::ensure!(
                [
                    run.bounds.x,
                    run.bounds.y,
                    run.bounds.width,
                    run.bounds.height
                ]
                .iter()
                .all(|value| value.is_finite())
                    && run.bounds.width >= 0.0
                    && run.bounds.height >= 0.0
                    && run.character_positions.len() == units
                    && run.character_widths.len() == units
                    && run
                        .character_positions
                        .iter()
                        .all(|value| value.is_finite())
                    && run
                        .character_widths
                        .iter()
                        .all(|value| value.is_finite() && *value >= 0.0),
                "invalid text run geometry"
            );
        }
        Ok(Arc::new(Self {
            document_id: document.id(),
            runs,
        }))
    }
    /// Immutable origin of every overlaid run.
    pub fn document_id(&self) -> AccessibilityId {
        self.document_id
    }
    /// Currently laid-out runs, sorted by identity.
    pub fn runs(&self) -> &[AccessibilityTextRunGeometry] {
        &self.runs
    }
    pub(super) fn get(&self, id: AccessibilityId) -> Option<&AccessibilityTextRunGeometry> {
        self.runs
            .binary_search_by_key(&id.0, |run| run.run_id.0)
            .ok()
            .map(|index| &self.runs[index])
    }
}

/// A directed selection measured in UTF-8 bytes in a prepared document.
/// Both endpoints must be selectable character boundaries, including the end.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AccessibilityTextSelection {
    /// Fixed endpoint where selection started.
    pub anchor: usize,
    /// Active endpoint, or caret when equal to `anchor`.
    pub focus: usize,
}

/// Borrowed immutable run metadata; no label or per-unit storage is copied.
pub struct AccessibilityTextRun<'a> {
    /// Stable run identity within this revision.
    pub id: AccessibilityId,
    /// Absolute UTF-8 byte span, including an ending hard line break.
    pub bytes: Range<usize>,
    /// Zero-based hard-line number.
    pub hard_line: usize,
    /// UTF-8 byte length of each selectable unit.
    pub character_lengths: &'a [u8],
    /// Resolved Unicode embedding direction for this immutable run.
    pub direction: AccessibilityTextDirection,
}

#[derive(Debug)]
struct TextLine {
    bytes: Range<usize>,
    lengths: Vec<u8>,
    // One byte-offset checkpoint per 64 characters bounds endpoint mapping.
    checkpoints: Vec<usize>,
    hard_line: usize,
    word_starts: Vec<u8>,
    direction: AccessibilityTextDirection,
}
impl TextLine {
    fn character_index(&self, byte: usize) -> Option<usize> {
        let local = byte.checked_sub(self.bytes.start)?;
        if local > self.bytes.len() {
            return None;
        }
        let block = self
            .checkpoints
            .partition_point(|offset| *offset <= local)
            .saturating_sub(1);
        let mut offset = self.checkpoints[block];
        let mut index = block * 64;
        while offset < local {
            offset += usize::from(*self.lengths.get(index)?);
            index += 1;
        }
        (offset == local).then_some(index)
    }
    fn byte_offset(&self, index: usize) -> Option<usize> {
        if index > self.lengths.len() {
            return None;
        }
        let block = index / 64;
        let offset = *self.checkpoints.get(block)?;
        Some(
            self.bytes.start
                + offset
                + self.lengths[block * 64..index]
                    .iter()
                    .map(|length| usize::from(*length))
                    .sum::<usize>(),
        )
    }
}

/// Immutable complete text metadata shared across cursor-only redraws.
///
/// Hard lines are split into runs of at most 255 selectable units so native
/// word indices cannot wrap. Character lengths use UTF-8 scalar
/// boundaries (CRLF is one selectable line-break character); native UTF-16
/// ranges are converted by AccessKit. There is no per-character ID map. Endpoint
/// conversion uses a line binary search and at most 64 character lengths.
/// Prepare once per content revision, on a worker for large documents, and keep
/// the returned Arc while only the selection changes.
pub struct AccessibilityTextDocument {
    value: Arc<str>,
    lines: Vec<TextLine>,
    ids: AccessibilityIdRange,
    reclaimer: Option<BackgroundExecutor>,
}
impl std::fmt::Debug for AccessibilityTextDocument {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AccessibilityTextDocument")
            .field("id", &self.id())
            .field("bytes", &self.len_bytes())
            .field("lines", &self.lines.len())
            .finish()
    }
}
impl PartialEq for AccessibilityTextDocument {
    fn eq(&self, other: &Self) -> bool {
        self.ids == other.ids
    }
}
impl AccessibilityTextDocument {
    /// Prepare the complete document. Empty text still has one text run.
    pub fn new(value: impl Into<Arc<str>>) -> Arc<Self> {
        Self::build(value.into(), None)
    }

    /// Prepare on the caller's worker and reclaim large final-owned text/run
    /// storage on the executor when an outgoing UI snapshot releases it.
    pub fn with_reclaim_executor(
        value: impl Into<Arc<str>>,
        executor: &BackgroundExecutor,
    ) -> Arc<Self> {
        Self::build(value.into(), Some(executor.clone()))
    }
    fn build(value: Arc<str>, reclaimer: Option<BackgroundExecutor>) -> Arc<Self> {
        let mut lines = Vec::new();
        let mut start = 0;
        let slices = value
            .split_inclusive('\n')
            .chain(value.ends_with('\n').then_some(""));
        for (hard_line, slice) in slices.enumerate() {
            let words = Self::word_starts(slice);
            let bidi = unicode_bidi::BidiInfo::new(slice, None);
            // UAX #9 L1 resets trailing whitespace and paragraph separators to
            // the paragraph level, matching the native single-line layout.
            let levels = bidi
                .paragraphs
                .first()
                .map(|paragraph| bidi.reordered_levels(paragraph, paragraph.range.clone()))
                .unwrap_or_default();
            let mut local_start = 0;
            let mut units = 0;
            let mut level = levels
                .first()
                .copied()
                .unwrap_or(unicode_bidi::Level::ltr());
            let mut characters = slice.char_indices().peekable();
            while let Some((offset, character)) = characters.next() {
                let next_level = levels[offset];
                if units == 255 || next_level != level {
                    lines.push(Self::line(
                        &slice[local_start..offset],
                        start + local_start,
                        hard_line,
                        &words,
                        local_start,
                        level.is_rtl(),
                    ));
                    local_start = offset;
                    units = 0;
                    level = next_level;
                }
                if character == '\r' && characters.peek().is_some_and(|(_, next)| *next == '\n') {
                    characters.next();
                }
                units += 1;
            }
            lines.push(Self::line(
                &slice[local_start..],
                start + local_start,
                hard_line,
                &words,
                local_start,
                level.is_rtl(),
            ));
            start += slice.len();
        }
        if lines.is_empty() {
            lines.push(Self::line("", 0, 0, &[], 0, false));
        }
        let ids = AccessibilityId::reserve_range(lines.len() as u64 + 1)
            .expect("accessibility identifier space exhausted");
        Arc::new(Self {
            value,
            lines,
            ids,
            reclaimer,
        })
    }
    fn line(
        value: &str,
        start: usize,
        hard_line: usize,
        words: &[usize],
        local_start: usize,
        right_to_left: bool,
    ) -> TextLine {
        let mut lengths = Vec::new();
        let mut checkpoints = vec![0];
        let mut offset = 0;
        let mut characters = value.chars().peekable();
        let mut word_starts = Vec::new();
        while let Some(character) = characters.next() {
            if words.binary_search(&(local_start + offset)).is_ok() {
                word_starts.push(lengths.len() as u8);
            }
            let length = if character == '\r' && characters.peek() == Some(&'\n') {
                characters.next();
                2
            } else {
                character.len_utf8() as u8
            };
            lengths.push(length);
            offset += usize::from(length);
            if lengths.len() % 64 == 0 {
                checkpoints.push(offset);
            }
        }
        TextLine {
            bytes: start..start + value.len(),
            lengths,
            checkpoints,
            hard_line,
            word_starts,
            direction: if right_to_left {
                AccessibilityTextDirection::RightToLeft
            } else {
                AccessibilityTextDirection::LeftToRight
            },
        }
    }
    // The editor's word policy: alphanumeric/underscore graphemes form words,
    // punctuation is one grapheme, trailing whitespace belongs to its word.
    fn word_starts(value: &str) -> Vec<usize> {
        if value.is_ascii() {
            return Self::ascii_word_starts(value.as_bytes());
        }
        Self::unicode_word_starts(value)
    }

    fn ascii_word_starts(bytes: &[u8]) -> Vec<usize> {
        let mut result = Vec::new();
        let mut offset = 0;
        while offset < bytes.len() {
            result.push(offset);
            let first = bytes[offset];
            // CRLF is the only multi-byte grapheme in an ASCII string.
            offset += if first == b'\r' && bytes.get(offset + 1) == Some(&b'\n') {
                2
            } else {
                1
            };
            if first.is_ascii_alphanumeric() || first == b'_' {
                while bytes
                    .get(offset)
                    .is_some_and(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
                {
                    offset += 1;
                }
            }
            // char::is_whitespace includes vertical tab; u8's ASCII predicate
            // does not. Preserve the existing Unicode word policy exactly.
            while bytes
                .get(offset)
                .is_some_and(|byte| matches!(*byte, b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c))
            {
                offset += 1;
            }
        }
        result
    }

    fn unicode_word_starts(value: &str) -> Vec<usize> {
        let mut result = Vec::new();
        let mut graphemes = value.grapheme_indices(true).peekable();
        while let Some((offset, text)) = graphemes.next() {
            result.push(offset);
            if text
                .chars()
                .next()
                .is_some_and(|ch| ch.is_alphanumeric() || ch == '_')
            {
                while graphemes.peek().is_some_and(|(_, text)| {
                    text.chars()
                        .next()
                        .is_some_and(|ch| ch.is_alphanumeric() || ch == '_')
                }) {
                    graphemes.next();
                }
            }
            while graphemes
                .peek()
                .is_some_and(|(_, text)| text.chars().all(char::is_whitespace))
            {
                graphemes.next();
            }
        }
        result
    }
    /// Unique document revision identity, independent of selection.
    pub fn id(&self) -> AccessibilityId {
        self.ids.get(0).unwrap()
    }
    /// Complete prepared text, without cloning it.
    pub fn text(&self) -> &str {
        &self.value
    }
    /// UTF-8 byte length, including hard line breaks.
    pub fn len_bytes(&self) -> usize {
        self.value.len()
    }
    /// Whether the prepared document is empty.
    pub fn is_empty(&self) -> bool {
        self.value.is_empty()
    }
    pub(super) fn is_multiline(&self) -> bool {
        self.lines.last().is_some_and(|run| run.hard_line > 0)
    }
    pub(super) fn run_ids(&self) -> impl Iterator<Item = accesskit::NodeId> + '_ {
        // A retained transparent container keeps caret-only root child storage
        // constant size even when the document contains 100,000 lines.
        std::iter::once(accesskit::NodeId(self.id().0))
    }
    pub(super) fn export_runs<'a>(
        &'a self,
        geometry: Option<&'a AccessibilityTextGeometry>,
    ) -> impl Iterator<Item = (accesskit::NodeId, accesskit::Node)> + 'a {
        let mut container = accesskit::Node::new(accesskit::Role::GenericContainer);
        container.set_children(
            (0..self.lines.len())
                .map(|index| accesskit::NodeId(self.ids.get(index as u64 + 1).unwrap().0))
                .collect::<Vec<_>>(),
        );
        std::iter::once((accesskit::NodeId(self.id().0), container)).chain(
            (0..self.lines.len()).map(move |index| {
                self.export_run(
                    index,
                    geometry
                        .filter(|geometry| geometry.document_id == self.id())
                        .and_then(|geometry| geometry.get(self.ids.get(index as u64 + 1).unwrap())),
                )
            }),
        )
    }
    fn run_index(&self, id: AccessibilityId) -> Option<usize> {
        let index = usize::try_from(id.0.checked_sub(self.id().0)?.checked_sub(1)?).ok()?;
        (index < self.lines.len()).then_some(index)
    }
    fn run(&self, id: AccessibilityId) -> Option<&TextLine> {
        self.lines.get(self.run_index(id)?)
    }
    /// Byte span of an exported run in this exact document revision.
    pub fn run_bytes(&self, id: AccessibilityId) -> Option<Range<usize>> {
        Some(self.run(id)?.bytes.clone())
    }
    /// Visit only runs intersecting a byte range, with binary-search setup.
    /// An empty range visits the run owning that caret, including document EOF.
    pub fn runs_for_bytes(
        &self,
        bytes: Range<usize>,
    ) -> impl Iterator<Item = AccessibilityTextRun<'_>> {
        let valid = bytes.start <= bytes.end
            && self.contains_selection(AccessibilityTextSelection {
                anchor: bytes.start,
                focus: bytes.end,
            });
        let first = self
            .lines
            .partition_point(|line| line.bytes.start <= bytes.start)
            .saturating_sub(1);
        self.lines
            .iter()
            .enumerate()
            .skip(first)
            .take_while(move |(index, line)| {
                valid && (*index == first || line.bytes.start < bytes.end)
            })
            .map(|(index, line)| AccessibilityTextRun {
                id: self.ids.get(index as u64 + 1).unwrap(),
                bytes: line.bytes.clone(),
                hard_line: line.hard_line,
                character_lengths: &line.lengths,
                direction: line.direction,
            })
    }
    fn export_run(
        &self,
        index: usize,
        geometry: Option<&AccessibilityTextRunGeometry>,
    ) -> (accesskit::NodeId, accesskit::Node) {
        let line = &self.lines[index];
        let id = self.ids.get(index as u64 + 1).unwrap();
        let mut node = accesskit::Node::new(accesskit::Role::TextRun);
        node.set_value(&self.value[line.bytes.clone()]);
        node.set_character_lengths(line.lengths.clone());
        node.set_word_starts(line.word_starts.clone());
        node.set_text_direction(line.direction);
        if index > 0 && self.lines[index - 1].hard_line == line.hard_line {
            node.set_previous_on_line(accesskit::NodeId(self.ids.get(index as u64).unwrap().0));
        }
        if self
            .lines
            .get(index + 1)
            .is_some_and(|next| next.hard_line == line.hard_line)
        {
            node.set_next_on_line(accesskit::NodeId(self.ids.get(index as u64 + 2).unwrap().0));
        }
        if let Some(geometry) = geometry {
            node.set_bounds(geometry.bounds.to_accesskit());
            node.set_text_direction(geometry.direction);
            node.set_character_positions(geometry.character_positions.to_vec());
            node.set_character_widths(geometry.character_widths.to_vec());
        }
        (accesskit::NodeId(id.0), node)
    }
    pub(super) fn export_geometry_after(
        &self,
        current: Option<&AccessibilityTextGeometry>,
        previous: Option<&AccessibilityTextGeometry>,
    ) -> Vec<(accesskit::NodeId, accesskit::Node)> {
        let current = current.filter(|geometry| geometry.document_id == self.id());
        let previous = previous.filter(|geometry| geometry.document_id == self.id());
        if current == previous {
            return Vec::new();
        }
        let mut ids = current
            .into_iter()
            .flat_map(|geometry| geometry.runs.iter().map(|run| run.run_id))
            .chain(
                previous
                    .into_iter()
                    .flat_map(|geometry| geometry.runs.iter().map(|run| run.run_id)),
            )
            .collect::<Vec<_>>();
        ids.sort_unstable_by_key(|id| id.0);
        ids.dedup();
        ids.into_iter()
            .filter_map(|id| {
                let new = current.and_then(|geometry| geometry.get(id));
                let old = previous.and_then(|geometry| geometry.get(id));
                (new != old).then(|| self.export_run(self.run_index(id).unwrap(), new))
            })
            .collect()
    }
    fn position(&self, byte: usize) -> Option<accesskit::TextPosition> {
        if byte > self.value.len() {
            return None;
        }
        let index = self
            .lines
            .partition_point(|line| line.bytes.start <= byte)
            .saturating_sub(1);
        Some(accesskit::TextPosition {
            node: accesskit::NodeId(self.ids.get(index as u64 + 1)?.0),
            character_index: self.lines[index].character_index(byte)?,
        })
    }
    fn byte_offset(&self, position: accesskit::TextPosition) -> Option<usize> {
        let index = position.node.0.checked_sub(self.id().0)?.checked_sub(1)?;
        self.lines
            .get(usize::try_from(index).ok()?)?
            .byte_offset(position.character_index)
    }
    /// Check a directed byte selection against selectable character boundaries.
    pub fn contains_selection(&self, selection: AccessibilityTextSelection) -> bool {
        self.position(selection.anchor).is_some() && self.position(selection.focus).is_some()
    }
    pub(super) fn export_selection(
        &self,
        selection: AccessibilityTextSelection,
    ) -> Option<accesskit::TextSelection> {
        Some(accesskit::TextSelection {
            anchor: self.position(selection.anchor)?,
            focus: self.position(selection.focus)?,
        })
    }
    pub(super) fn import_selection(
        &self,
        selection: accesskit::TextSelection,
    ) -> Option<AccessibilityTextSelection> {
        Some(AccessibilityTextSelection {
            anchor: self.byte_offset(selection.anchor)?,
            focus: self.byte_offset(selection.focus)?,
        })
    }
}
impl Drop for AccessibilityTextDocument {
    fn drop(&mut self) {
        if let Some(executor) = self.reclaimer.take() {
            let value = std::mem::take(&mut self.value);
            let lines = std::mem::take(&mut self.lines);
            executor
                .spawn(async move {
                    drop((value, lines));
                })
                .detach();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        AccessibilityAction, AccessibilityActionPayload, AccessibilityActionRequest,
        AccessibilityAttributes, AccessibilityRole, AccessibilityState, AccessibilityTree,
    };

    #[test]
    fn ascii_words_match_unicode_grapheme_policy_for_every_byte_pair() {
        for first in 0..=127 {
            for second in 0..=127 {
                let bytes = [first, second];
                let value = std::str::from_utf8(&bytes).unwrap();
                assert_eq!(
                    AccessibilityTextDocument::word_starts(value),
                    AccessibilityTextDocument::unicode_word_starts(value),
                    "ASCII bytes {bytes:?}"
                );
            }
        }
    }

    #[test]
    fn ascii_words_preserve_crlf_controls_and_long_mixed_sequences() {
        let mut inputs = vec![
            String::new(),
            "a\r\nb\t_c\u{0b}\u{0c}! \r\n".to_string(),
            "\r\n\r\n\u{0b}\u{0c}\t 0_a: punctuation...\n".to_string(),
        ];
        let mut seed = 0x4173_625b_u32;
        for length in 0..512 {
            let bytes = (0..length)
                .map(|_| {
                    seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                    (seed >> 24) as u8 & 0x7f
                })
                .collect();
            inputs.push(String::from_utf8(bytes).unwrap());
        }
        for value in inputs {
            assert_eq!(
                AccessibilityTextDocument::word_starts(&value),
                AccessibilityTextDocument::unicode_word_starts(&value),
                "word policy for {value:?}"
            );
        }
        for value in ["A🙂e\u{301}\r\n第二行\n", "אבג 123 café\t", "क्\u{200d}ष"] {
            assert_eq!(
                AccessibilityTextDocument::word_starts(value),
                AccessibilityTextDocument::unicode_word_starts(value)
            );
        }
    }

    #[test]
    fn unicode_directed_endpoints_roundtrip_and_reject_foreign_or_nonboundary_positions() {
        let document = AccessibilityTextDocument::new("A🙂e\u{301}\r\n第二行\n");
        let value = document.text();
        for anchor in 0..=value.len() {
            for focus in [0, 1, value.len()] {
                let selection = AccessibilityTextSelection { anchor, focus };
                let expected = value.is_char_boundary(anchor) && !value[..anchor].ends_with('\r');
                assert_eq!(document.contains_selection(selection), expected);
                if let Some(native) = document.export_selection(selection) {
                    assert_eq!(document.import_selection(native), Some(selection));
                }
            }
        }
        let mut selection = document
            .export_selection(AccessibilityTextSelection {
                anchor: value.len(),
                focus: 1,
            })
            .unwrap();
        let other = AccessibilityTextDocument::new(value);
        assert!(other.import_selection(selection).is_none());
        selection.focus.character_index = usize::MAX;
        assert!(document.import_selection(selection).is_none());
        assert!(!document.contains_selection(AccessibilityTextSelection {
            anchor: value.len() + 1,
            focus: 0
        }));
    }

    #[test]
    fn empty_documents_and_long_line_checkpoints_preserve_exact_boundaries() {
        let empty = AccessibilityTextDocument::new("");
        let caret = AccessibilityTextSelection {
            anchor: 0,
            focus: 0,
        };
        assert_eq!(
            empty.import_selection(empty.export_selection(caret).unwrap()),
            Some(caret)
        );
        let document = AccessibilityTextDocument::new("🙂".repeat(10_000));
        for character in [0, 63, 64, 65, 127, 128, 10_000] {
            let selection = AccessibilityTextSelection {
                anchor: 40_000,
                focus: character * 4,
            };
            assert_eq!(
                document.import_selection(document.export_selection(selection).unwrap()),
                Some(selection)
            );
        }
        assert!(!document.contains_selection(AccessibilityTextSelection {
            anchor: 255,
            focus: 0
        }));
    }

    #[test]
    fn text_selection_normalization_checks_document_actions_and_unicode_bytes() {
        let document = AccessibilityTextDocument::new("A🙂\n第二行");
        let mut node = AccessibilityAttributes::new(AccessibilityRole::TextInput)
            .text_document(
                document.clone(),
                AccessibilityTextSelection {
                    anchor: 0,
                    focus: 0,
                },
            )
            .actions(vec![AccessibilityAction::SetTextSelection])
            .to_node(AccessibilityId::new());
        let raw = document
            .export_selection(AccessibilityTextSelection {
                anchor: document.len_bytes(),
                focus: 1,
            })
            .unwrap();
        let normalized = |node: &crate::AccessibilityNode, selection| {
            AccessibilityActionRequest::from_accesskit_for_node_with_data(
                node.id,
                node,
                accesskit::Action::SetTextSelection,
                Some(accesskit::ActionData::SetTextSelection(selection)),
            )
        };
        assert_eq!(
            normalized(&node, raw).unwrap().payload,
            Some(AccessibilityActionPayload::TextSelection {
                document_id: document.id(),
                anchor: document.len_bytes(),
                focus: 1
            })
        );
        node.states |= AccessibilityState::DISABLED;
        assert!(normalized(&node, raw).is_none());
        node.states = AccessibilityState::HIDDEN;
        assert!(normalized(&node, raw).is_none());
        node.states = AccessibilityState::READ_ONLY;
        assert!(
            normalized(&node, raw).is_some(),
            "read-only text still permits its advertised selection action"
        );
        node.states = AccessibilityState::NONE;
        node.text_document = Some(AccessibilityTextDocument::new(document.text()));
        assert!(
            normalized(&node, raw).is_none(),
            "old revision text positions are rejected"
        );
        assert!(
            AccessibilityActionRequest::from_accesskit_with_data(
                node.id,
                accesskit::Action::SetTextSelection,
                Some(accesskit::ActionData::SetTextSelection(raw))
            )
            .is_none()
        );
    }

    #[test]
    fn synthetic_run_reveal_resolves_current_owner_without_per_run_nodes() {
        let document = AccessibilityTextDocument::new("row 日本🙂\n".repeat(100_000));
        let run = document
            .runs_for_bytes(document.len_bytes()..document.len_bytes())
            .next()
            .unwrap();
        let owner = AccessibilityAttributes::new(AccessibilityRole::TextInput)
            .text_document(
                document.clone(),
                AccessibilityTextSelection {
                    anchor: 0,
                    focus: 0,
                },
            )
            .actions(vec![
                AccessibilityAction::ScrollToVisible,
                AccessibilityAction::CopyText,
                AccessibilityAction::CutText,
                AccessibilityAction::ReplaceSelectedText,
            ])
            .to_node(AccessibilityId::new());
        let mut tree = AccessibilityTree::new(owner);
        assert_eq!(
            tree.nodes.len(),
            1,
            "text runs do not become common NodeMap entries"
        );
        for _ in 0..1000 {
            let request = tree
                .normalize_accesskit_action(
                    run.id,
                    accesskit::Action::ScrollIntoView,
                    Some(accesskit::ActionData::ScrollHint(
                        accesskit::ScrollHint::BottomEdge,
                    )),
                )
                .unwrap();
            assert_eq!(request.node_id, tree.root);
            assert_eq!(
                request.payload,
                Some(AccessibilityActionPayload::TextReveal {
                    document_id: document.id(),
                    start: document.len_bytes(),
                    end: document.len_bytes(),
                    alignment: AccessibilityTextAlignment::Bottom
                })
            );
        }
        let raw = document
            .export_selection(AccessibilityTextSelection {
                anchor: 0,
                focus: 3,
            })
            .unwrap();
        let copy = tree
            .normalize_text_clipboard(tree.root, raw, AccessibilityAction::CopyText)
            .unwrap();
        assert!(
            tree.normalize_text_replacement(tree.root, raw, "🙂".into())
                .is_some()
        );
        tree.get_mut(tree.root).unwrap().states = AccessibilityState::READ_ONLY;
        assert!(
            tree.normalize_text_clipboard(tree.root, raw, AccessibilityAction::CopyText)
                .is_some()
        );
        assert!(
            tree.normalize_text_clipboard(tree.root, raw, AccessibilityAction::CutText)
                .is_none()
        );
        assert!(
            tree.normalize_text_replacement(tree.root, raw, "🙂".into())
                .is_none()
        );
        tree.get_mut(tree.root).unwrap().states = AccessibilityState::DISABLED;
        assert!(
            tree.normalize_accesskit_action(run.id, accesskit::Action::ScrollIntoView, None)
                .is_none()
        );
        tree.get_mut(tree.root).unwrap().states = AccessibilityState::NONE;
        tree.get_mut(tree.root).unwrap().text_document =
            Some(AccessibilityTextDocument::new("replacement"));
        assert!(
            tree.normalize_accesskit_action(run.id, accesskit::Action::ScrollIntoView, None)
                .is_none()
        );
        assert!(
            tree.validate_action_request(copy).is_none(),
            "queued origin is checked again"
        );
    }

    #[test]
    fn long_line_word_navigation_crosses_bounded_runs_without_wrapping_indices() {
        let value = format!("{} . café🙂\n", "a".repeat(300));
        let document = AccessibilityTextDocument::new(value.clone());
        let owner = AccessibilityAttributes::new(AccessibilityRole::TextInput)
            .text_document(
                document.clone(),
                AccessibilityTextSelection {
                    anchor: 0,
                    focus: 0,
                },
            )
            .to_node(AccessibilityId::new());
        let tree = AccessibilityTree::new(owner);
        let update = tree.to_accesskit_tree_update(None, None);
        assert!(
            update
                .nodes
                .iter()
                .filter(|(_, node)| node.role() == accesskit::Role::TextRun)
                .all(|(_, node)| node.character_lengths().len() <= 255)
        );
        let consumer = accesskit_consumer::Tree::new(update, true);
        let root = consumer.state().root();
        assert_eq!(root.document_range().text(), value);
        let position = root.text_position_from_global_usv_index(255).unwrap();
        assert_eq!(position.backward_to_word_start().to_global_usv_index(), 0);
        assert_eq!(position.forward_to_word_start().to_global_usv_index(), 301);
        assert_eq!(root.line_range_from_index(0).unwrap().text(), value);
    }

    #[test]
    fn directional_runs_keep_unicode_selection_words_and_native_rtl_hit_testing() {
        let value = "abc אבג xyz\n";
        let document = AccessibilityTextDocument::new(value);
        let runs = document.runs_for_bytes(0..value.len()).collect::<Vec<_>>();
        assert!(
            runs.iter()
                .any(|run| run.direction == AccessibilityTextDirection::RightToLeft)
        );
        assert!(runs.iter().all(|run| run.character_lengths.len() <= 255));
        assert_eq!(
            runs.iter()
                .map(|run| &value[run.bytes.clone()])
                .collect::<String>(),
            value
        );
        let rtl = runs
            .iter()
            .find(|run| run.direction == AccessibilityTextDirection::RightToLeft)
            .unwrap();
        assert_eq!(&value[rtl.bytes.clone()], "אבג");
        let geometry = AccessibilityTextGeometry::new(
            &document,
            vec![AccessibilityTextRunGeometry {
                run_id: rtl.id,
                bounds: AccessibilityRect::new(100.0, 200.0, 30.0, 14.0),
                direction: rtl.direction,
                character_positions: vec![0.0, 10.0, 20.0].into(),
                character_widths: vec![10.0; 3].into(),
            }],
        )
        .unwrap();
        let owner = AccessibilityAttributes::new(AccessibilityRole::TextInput)
            .text_document(
                document.clone(),
                AccessibilityTextSelection {
                    anchor: rtl.bytes.start,
                    focus: rtl.bytes.start + 'א'.len_utf8(),
                },
            )
            .text_geometry(geometry)
            .to_node(AccessibilityId::new());
        let tree = AccessibilityTree::new(owner);
        let consumer =
            accesskit_consumer::Tree::new(tree.to_accesskit_tree_update(None, None), true);
        let root = consumer.state().root();
        assert_eq!(root.document_range().text(), value);
        assert_eq!(root.line_range_from_index(0).unwrap().text(), value);
        assert_eq!(root.text_selection().unwrap().text(), "א");
        assert_eq!(
            root.text_selection().unwrap().bounding_boxes(),
            vec![accesskit::Rect {
                x0: 120.0,
                x1: 130.0,
                y0: 200.0,
                y1: 214.0,
            }]
        );
        assert_eq!(
            root.text_position_at_point(accesskit::Point::new(125.0, 205.0))
                .to_global_usv_index(),
            4
        );
        let mut current = tree.clone();
        current.get_mut(current.root).unwrap().text_selection = Some(AccessibilityTextSelection {
            anchor: value.len(),
            focus: 0,
        });
        let update = current.to_accesskit_tree_update_after(Some(&tree), None, None);
        assert_eq!(
            update.nodes.len(),
            1,
            "directional runs stay immutable during caret changes"
        );
    }

    #[test]
    fn viewport_geometry_exports_once_reuses_caret_and_clears_departed_runs() {
        let document = AccessibilityTextDocument::new("ab🙂\nZ");
        let first = document.runs_for_bytes(0..1).next().unwrap();
        let geometry = AccessibilityTextGeometry::new(
            &document,
            vec![AccessibilityTextRunGeometry {
                run_id: first.id,
                bounds: AccessibilityRect::new(100.0, 200.0, 30.0, 14.0),
                direction: AccessibilityTextDirection::LeftToRight,
                character_positions: vec![0.0, 10.0, 20.0, 30.0].into(),
                character_widths: vec![10.0, 10.0, 10.0, 0.0].into(),
            }],
        )
        .unwrap();
        let owner = AccessibilityAttributes::new(AccessibilityRole::TextInput)
            .text_document(
                document.clone(),
                AccessibilityTextSelection {
                    anchor: 1,
                    focus: 6,
                },
            )
            .text_geometry(geometry.clone())
            .to_node(AccessibilityId::new());
        let previous = AccessibilityTree::new(owner);
        let update = previous.to_accesskit_tree_update(None, None);
        let ids = update
            .nodes
            .iter()
            .map(|(id, _)| *id)
            .collect::<std::collections::HashSet<_>>();
        assert_eq!(
            ids.len(),
            update.nodes.len(),
            "full update has no duplicated overlaid run"
        );
        let mut consumer = accesskit_consumer::Tree::new(update, true);
        let root = consumer.state().root();
        assert_eq!(
            root.text_selection().unwrap().bounding_boxes(),
            vec![accesskit::Rect {
                x0: 110.0,
                y0: 200.0,
                x1: 130.0,
                y1: 214.0
            }]
        );
        assert_eq!(
            root.text_position_at_point(accesskit::Point::new(112.0, 205.0))
                .to_global_usv_index(),
            1
        );
        let mut current = previous.clone();
        current.get_mut(current.root).unwrap().text_selection = Some(AccessibilityTextSelection {
            anchor: 0,
            focus: 1,
        });
        let update = current.to_accesskit_tree_update_after(Some(&previous), None, None);
        assert_eq!(
            update.nodes.len(),
            1,
            "caret-only export keeps geometry records"
        );
        consumer.update_and_process_changes(update, &mut Changes);
        let mut scrolled = current.clone();
        scrolled.get_mut(scrolled.root).unwrap().text_geometry = None;
        let update = scrolled.to_accesskit_tree_update_after(Some(&current), None, None);
        assert_eq!(
            update.nodes.len(),
            2,
            "owner plus bounded departing run reset"
        );
        consumer.update_and_process_changes(update, &mut Changes);
        assert!(
            consumer
                .state()
                .root()
                .text_selection()
                .unwrap()
                .bounding_boxes()
                .is_empty()
        );
        assert_eq!(
            consumer.state().root().document_range().text(),
            document.text()
        );
    }

    struct Changes;
    impl accesskit_consumer::TreeChangeHandler for Changes {
        fn node_added(&mut self, _: &accesskit_consumer::Node<'_>) {}
        fn node_updated(
            &mut self,
            _: &accesskit_consumer::Node<'_>,
            _: &accesskit_consumer::Node<'_>,
        ) {
        }
        fn focus_moved(
            &mut self,
            _: Option<&accesskit_consumer::Node<'_>>,
            _: Option<&accesskit_consumer::Node<'_>>,
        ) {
        }
        fn node_removed(&mut self, _: &accesskit_consumer::Node<'_>) {}
    }

    #[test]
    fn hundred_thousand_lines_export_once_and_caret_delta_has_constant_child_storage() {
        let value = "A🙂e\u{301}\n".repeat(100_000);
        let document = AccessibilityTextDocument::new(value.as_str());
        let root = AccessibilityAttributes::new(AccessibilityRole::TextInput)
            .text_document(
                document.clone(),
                AccessibilityTextSelection {
                    anchor: value.len(),
                    focus: 1,
                },
            )
            .actions(vec![AccessibilityAction::SetTextSelection])
            .to_node(AccessibilityId::new());
        let previous = AccessibilityTree::new(root);
        let update = previous.to_accesskit_tree_update(None, None);
        assert_eq!(update.nodes.len(), 100_003); // control, container and 100001 lines
        assert_eq!(update.nodes[0].1.children().len(), 1);
        let mut consumer = accesskit_consumer::Tree::new(update, true);
        {
            let state = consumer.state();
            let root = state.root();
            assert!(root.supports_text_ranges());
            assert_eq!(root.document_range().text(), value);
            assert_eq!(root.text_selection().unwrap().text(), &value[1..]);
        }
        let mut current = previous.clone();
        current.nodes.get_mut(&current.root).unwrap().text_selection =
            Some(AccessibilityTextSelection {
                anchor: 1,
                focus: 5,
            });
        let update = current.to_accesskit_tree_update_after(Some(&previous), None, None);
        assert_eq!(
            update.nodes.len(),
            1,
            "caret redraw must retain every line record"
        );
        assert_eq!(update.nodes[0].1.children().len(), 1);
        consumer.update_and_process_changes(update, &mut Changes);
        assert_eq!(
            consumer.state().root().text_selection().unwrap().text(),
            "🙂"
        );
        let mut replacement = current.clone();
        let root = replacement.nodes.get_mut(&replacement.root).unwrap();
        root.text_document = Some(AccessibilityTextDocument::new("new🙂"));
        root.text_selection = Some(AccessibilityTextSelection {
            anchor: 3,
            focus: 7,
        });
        consumer.update_and_process_changes(
            replacement.to_accesskit_tree_update_after(Some(&current), None, None),
            &mut Changes,
        );
        assert_eq!(consumer.state().root().document_range().text(), "new🙂");
        assert_eq!(
            consumer.state().root().text_selection().unwrap().text(),
            "🙂"
        );
    }
}
