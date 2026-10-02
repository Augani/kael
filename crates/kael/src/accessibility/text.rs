//! Immutable text-run metadata for native selection and text-range providers.

use super::{AccessibilityId, AccessibilityIdRange};
use crate::BackgroundExecutor;
use std::{ops::Range, sync::Arc};

/// A directed selection measured in UTF-8 bytes in a prepared document.
/// Both endpoints must be selectable character boundaries, including the end.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AccessibilityTextSelection {
    /// Fixed endpoint where selection started.
    pub anchor: usize,
    /// Active endpoint, or caret when equal to `anchor`.
    pub focus: usize,
}

#[derive(Debug)]
struct TextLine {
    bytes: Range<usize>,
    lengths: Vec<u8>,
    // One byte-offset checkpoint per 64 characters bounds endpoint mapping.
    checkpoints: Vec<usize>,
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
/// Each hard line is an AccessKit text run. Character lengths use UTF-8 scalar
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
        for slice in slices {
            lines.push(Self::line(slice, start));
            start += slice.len();
        }
        if lines.is_empty() {
            lines.push(Self::line("", 0));
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
    fn line(value: &str, start: usize) -> TextLine {
        let mut lengths = Vec::new();
        let mut checkpoints = vec![0];
        let mut offset = 0;
        let mut characters = value.chars().peekable();
        while let Some(character) = characters.next() {
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
        }
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
        self.lines.len() > 1
    }
    pub(super) fn run_ids(&self) -> impl Iterator<Item = accesskit::NodeId> + '_ {
        // A retained transparent container keeps caret-only root child storage
        // constant size even when the document contains 100,000 lines.
        std::iter::once(accesskit::NodeId(self.id().0))
    }
    pub(super) fn export_runs(
        &self,
    ) -> impl Iterator<Item = (accesskit::NodeId, accesskit::Node)> + '_ {
        let mut container = accesskit::Node::new(accesskit::Role::GenericContainer);
        container.set_children(
            (0..self.lines.len())
                .map(|index| accesskit::NodeId(self.ids.get(index as u64 + 1).unwrap().0))
                .collect::<Vec<_>>(),
        );
        std::iter::once((accesskit::NodeId(self.id().0), container)).chain(
            self.lines.iter().enumerate().map(|(index, line)| {
                let mut node = accesskit::Node::new(accesskit::Role::TextRun);
                node.set_value(&self.value[line.bytes.clone()]);
                node.set_character_lengths(line.lengths.clone());
                (
                    accesskit::NodeId(self.ids.get(index as u64 + 1).unwrap().0),
                    node,
                )
            }),
        )
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
