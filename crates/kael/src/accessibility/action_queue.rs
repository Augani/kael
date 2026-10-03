//! Bounded native action ownership shared by every platform adapter.

use accesskit::{ActionData, ActionRequest};

/// Raw native identities survive until current-model resolution. Native atomic
/// edits can also enqueue one already-normalized immutable-document request.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum PendingAccessibilityAction {
    Raw(ActionRequest),
    #[cfg_attr(
        not(any(target_os = "linux", target_os = "freebsd", test)),
        allow(dead_code)
    )]
    Normalized(super::AccessibilityActionRequest),
}
impl PendingAccessibilityAction {
    pub(crate) fn normalize(
        self,
        tree: &super::AccessibilityTree,
    ) -> Option<super::AccessibilityActionRequest> {
        match self {
            Self::Raw(request) if request.target_tree == accesskit::TreeId::ROOT => tree
                .normalize_accesskit_action(
                    super::AccessibilityId(request.target_node.0),
                    request.action,
                    request.data,
                ),
            Self::Raw(_) => None,
            Self::Normalized(request) => tree.validate_action_request(request),
        }
    }
}

#[derive(Default)]
pub(crate) struct PendingActionQueue {
    requests: Vec<PendingAccessibilityAction>,
    text_bytes: usize,
    dropped: usize,
}

impl PendingActionQueue {
    pub(crate) const MAX_REQUESTS: usize = 1_024;
    pub(crate) const MAX_TEXT_BYTES: usize = 16 * 1024 * 1024;

    pub(crate) fn len(&self) -> usize {
        self.requests.len()
    }
    pub(crate) fn is_empty(&self) -> bool {
        self.requests.is_empty()
    }
    pub(crate) fn dropped(&self) -> usize {
        self.dropped
    }
    #[cfg(test)]
    pub(crate) fn text_bytes(&self) -> usize {
        self.text_bytes
    }

    /// Preserve FIFO and repeated actions. Count retained string bytes before
    /// admission. A rejected request increments a saturating batch counter.
    pub(crate) fn push(&mut self, request: ActionRequest) -> bool {
        let bytes = match request.data.as_ref() {
            Some(ActionData::Value(value)) => value.len(),
            _ => 0,
        };
        self.push_event(PendingAccessibilityAction::Raw(request), bytes)
    }

    #[cfg_attr(
        not(any(target_os = "linux", target_os = "freebsd", test)),
        allow(dead_code)
    )]
    pub(crate) fn push_normalized(&mut self, request: super::AccessibilityActionRequest) -> bool {
        let bytes = request
            .payload
            .as_ref()
            .map_or(0, super::AccessibilityActionPayload::value_len_bytes);
        self.push_event(PendingAccessibilityAction::Normalized(request), bytes)
    }

    fn push_event(&mut self, request: PendingAccessibilityAction, bytes: usize) -> bool {
        if self.requests.len() >= Self::MAX_REQUESTS
            || bytes > Self::MAX_TEXT_BYTES - self.text_bytes
        {
            self.dropped = self.dropped.saturating_add(1);
            return false;
        }
        self.text_bytes += bytes;
        self.requests.push(request);
        true
    }

    /// Move accepted requests out for processing outside the queue lock, and
    /// reset pending-byte/drop accounting for the next batch.
    pub(crate) fn take(&mut self) -> Vec<PendingAccessibilityAction> {
        self.text_bytes = 0;
        self.dropped = 0;
        std::mem::take(&mut self.requests)
    }

    /// Call outside platform locks only when a rejected push has `dropped()==1`.
    /// The message contains no target IDs, values or other application content.
    pub(crate) fn report_overflow() {
        log::warn!(
            "native accessibility action queue admission limit reached; excess requests in this batch are dropped (1024 requests or 16 MiB text)"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use accesskit::{Action, NodeId, TreeId};

    fn request(index: usize, data: Option<ActionData>) -> ActionRequest {
        ActionRequest {
            action: match index % 3 {
                0 => Action::Click,
                1 => Action::Increment,
                _ => Action::Decrement,
            },
            target_tree: TreeId::ROOT,
            target_node: NodeId((index % 2) as u64),
            data,
        }
    }

    #[test]
    fn flood_retains_fifo_repeated_actions_and_recovers_after_drain() {
        let mut queue = PendingActionQueue::default();
        let expected = (0..PendingActionQueue::MAX_REQUESTS)
            .map(|index| request(index, None))
            .collect::<Vec<_>>();
        for request in &expected {
            assert!(queue.push(request.clone()));
        }
        for index in 0..500 {
            assert!(!queue.push(request(index, None)));
        }
        assert_eq!(queue.len(), PendingActionQueue::MAX_REQUESTS);
        assert_eq!(queue.dropped(), 500);
        assert_eq!(
            queue.take(),
            expected
                .into_iter()
                .map(PendingAccessibilityAction::Raw)
                .collect::<Vec<_>>()
        );
        assert!(queue.is_empty());
        assert_eq!(queue.dropped(), 0);
        assert!(queue.push(request(0, None)));
    }

    #[test]
    fn aggregate_utf8_payload_budget_is_checked_before_retention() {
        let mut queue = PendingActionQueue::default();
        let chunk = "🙂".repeat(PendingActionQueue::MAX_TEXT_BYTES / 8);
        for index in 0..2 {
            assert!(queue.push(request(
                index,
                Some(ActionData::Value(chunk.clone().into()))
            )));
        }
        assert_eq!(queue.text_bytes(), PendingActionQueue::MAX_TEXT_BYTES);
        assert!(!queue.push(request(2, Some(ActionData::Value("🙂".into())))));
        assert_eq!(queue.dropped(), 1);
        assert!(
            queue.push(request(2, None)),
            "scalar requests fit even at the text budget"
        );
        let accepted = queue.take();
        assert_eq!(accepted.len(), 3);
        assert_eq!(queue.text_bytes(), 0);
        assert_eq!(queue.dropped(), 0);
        assert!(queue.push(request(0, Some(ActionData::Value(chunk.into())))));
    }

    #[test]
    fn raw_and_atomic_edit_requests_share_fifo_and_utf8_byte_admission() {
        use super::super::{
            AccessibilityAction, AccessibilityActionPayload, AccessibilityActionRequest,
            AccessibilityId,
        };
        let mut queue = PendingActionQueue::default();
        let edit = AccessibilityActionRequest::with_payload(
            AccessibilityId::new(),
            AccessibilityAction::ReplaceSelectedText,
            AccessibilityActionPayload::TextReplacement {
                document_id: AccessibilityId::new(),
                start: 0,
                end: 0,
                value: "🙂".repeat(PendingActionQueue::MAX_TEXT_BYTES / 4),
            },
        );
        assert!(queue.push(request(0, None)));
        assert!(queue.push_normalized(edit.clone()));
        assert_eq!(queue.text_bytes(), PendingActionQueue::MAX_TEXT_BYTES);
        assert!(!queue.push(request(1, Some(ActionData::Value("x".into())))));
        assert!(queue.push(request(2, None)));
        assert_eq!(
            queue.take(),
            vec![
                PendingAccessibilityAction::Raw(request(0, None)),
                PendingAccessibilityAction::Normalized(edit),
                PendingAccessibilityAction::Raw(request(2, None))
            ]
        );
        assert_eq!(queue.text_bytes(), 0);
        assert_eq!(queue.dropped(), 0);
    }

    #[test]
    fn oversized_single_value_is_rejected_without_any_pending_work() {
        let mut queue = PendingActionQueue::default();
        assert!(!queue.push(request(
            0,
            Some(ActionData::Value(
                "x".repeat(PendingActionQueue::MAX_TEXT_BYTES + 1).into()
            ))
        )));
        assert!(queue.is_empty());
        assert_eq!(queue.text_bytes(), 0);
        assert_eq!(queue.dropped(), 1);
        assert!(queue.push(request(1, None)));
        assert_eq!(
            queue.take(),
            [PendingAccessibilityAction::Raw(request(1, None))]
        );
        assert_eq!(queue.dropped(), 0);
    }
}
