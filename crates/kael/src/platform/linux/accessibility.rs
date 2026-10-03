//! AT-SPI2 accessibility support for the Linux backend via AccessKit.
//!
//! Each window owns an [`AtSpiAccessibleRoot`] that wraps an
//! [`accesskit_unix::Adapter`]. The adapter speaks the AT-SPI2 D-Bus protocol
//! (`org.a11y.atspi.*`) to expose GPUI elements to screen readers such as Orca.
//!
//! ## Async runtime
//!
//! `accesskit_unix` runs its own async executor. With the default `async-io`
//! feature (which this crate relies on), the adapter spawns and owns a
//! background thread for its zbus connection the first time an adapter is
//! created, so it does not need to be driven by kael's foreground/background
//! executors. The activation, action, and deactivation handlers are therefore
//! invoked from that adapter-owned thread, which is why the shared state they
//! touch is held behind `Arc<Mutex<_>>`.

use std::cell::RefCell;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

use accesskit::{ActionHandler, ActionRequest, ActivationHandler, DeactivationHandler, TreeUpdate};
use accesskit_unix::{Adapter, TextEditHandler, TextEditOperation, TextEditRequest};
use futures::{StreamExt, channel::mpsc};

use crate::{PermissionStatus, accessibility::PendingActionQueue};

const TOOLKIT_NAME: &str = "Kael";
const TOOLKIT_VERSION: &str = env!("CARGO_PKG_VERSION");

type SharedUpdate = Arc<Mutex<Option<crate::AccessibilityTree>>>;
type PendingActions = Arc<Mutex<PendingActionQueue>>;
type ActionWake = Arc<Mutex<Option<mpsc::Sender<()>>>>;

struct InitialTreeHandler {
    latest: SharedUpdate,
}

impl ActivationHandler for InitialTreeHandler {
    fn request_initial_tree(&mut self) -> Option<TreeUpdate> {
        self.latest.lock().ok().and_then(|guard| {
            guard.as_ref().map(|tree| {
                tree.to_accesskit_tree_update(Some(TOOLKIT_NAME), Some(TOOLKIT_VERSION))
            })
        })
    }
}

struct CollectingActionHandler {
    latest: SharedUpdate,
    pending: PendingActions,
    wake: ActionWake,
    alive: Arc<AtomicBool>,
}

impl ActionHandler for CollectingActionHandler {
    fn do_action(&mut self, request: ActionRequest) {
        let (needs_wake, first_overflow) = if let Ok(mut pending) = self.pending.lock() {
            if !self.alive.load(Ordering::Acquire) {
                return;
            }
            debug_assert!(pending.len() <= PendingActionQueue::MAX_REQUESTS);
            let was_empty = pending.is_empty();
            let accepted = pending.push(request);
            (accepted && was_empty, !accepted && pending.dropped() == 1)
        } else {
            (false, false)
        };
        if first_overflow {
            PendingActionQueue::report_overflow();
        }
        // AccessKit invokes this handler on its D-Bus worker. Sending a
        // coalesced signal wakes a foreground task; no Rc or platform window
        // borrow crosses that thread boundary.
        if needs_wake {
            if let Ok(mut wake) = self.wake.lock() {
                if let Some(sender) = wake.as_mut() {
                    let _ = sender.try_send(());
                }
            }
        }
    }
}

impl TextEditHandler for CollectingActionHandler {
    fn edit_text(&mut self, request: TextEditRequest) -> bool {
        if request.target_tree != accesskit::TreeId::ROOT || !self.alive.load(Ordering::Acquire) {
            return false;
        }
        let normalized = self.latest.lock().ok().and_then(|latest| {
            let tree = latest.as_ref()?;
            let owner = crate::AccessibilityId(request.target_node.0);
            match request.operation {
                TextEditOperation::Replace(value) => {
                    tree.normalize_text_replacement(owner, request.selection, value)
                }
                operation => tree.normalize_text_clipboard(
                    owner,
                    request.selection,
                    match operation {
                        TextEditOperation::Copy => crate::AccessibilityAction::CopyText,
                        TextEditOperation::Cut => crate::AccessibilityAction::CutText,
                        TextEditOperation::Paste => crate::AccessibilityAction::PasteText,
                        _ => unreachable!(),
                    },
                ),
            }
        });
        let Some(normalized) = normalized else {
            return false;
        };
        let (accepted, needs_wake, first_overflow) = if let Ok(mut pending) = self.pending.lock() {
            if !self.alive.load(Ordering::Acquire) {
                return false;
            }
            let was_empty = pending.is_empty();
            let accepted = pending.push_normalized(normalized);
            (
                accepted,
                accepted && was_empty,
                !accepted && pending.dropped() == 1,
            )
        } else {
            (false, false, false)
        };
        if first_overflow {
            PendingActionQueue::report_overflow();
        }
        if needs_wake
            && let Ok(mut wake) = self.wake.lock()
            && let Some(sender) = wake.as_mut()
        {
            let _ = sender.try_send(());
        }
        accepted
    }
}

struct NoopDeactivationHandler;

impl DeactivationHandler for NoopDeactivationHandler {
    fn deactivate_accessibility(&mut self) {}
}

/// The root AT-SPI2 accessible object for a GPUI window, backed by AccessKit.
///
/// Wraps an [`accesskit_unix::Adapter`] and feeds it [`TreeUpdate`]s built from
/// the shared [`crate::AccessibilityTree`].
pub struct AtSpiAccessibleRoot {
    adapter: RefCell<Adapter>,
    latest: SharedUpdate,
    pending_actions: PendingActions,
    action_wake: ActionWake,
    action_wake_task: RefCell<Option<crate::Task<()>>>,
    alive: Arc<AtomicBool>,
}

impl AtSpiAccessibleRoot {
    pub fn new() -> Self {
        let latest: SharedUpdate = Arc::new(Mutex::new(None));
        let pending_actions: PendingActions = Arc::new(Mutex::new(PendingActionQueue::default()));
        let action_wake: ActionWake = Arc::new(Mutex::new(None));
        let alive = Arc::new(AtomicBool::new(true));
        let adapter = Adapter::new_with_text_handler(
            InitialTreeHandler {
                latest: latest.clone(),
            },
            CollectingActionHandler {
                latest: latest.clone(),
                pending: pending_actions.clone(),
                wake: action_wake.clone(),
                alive: alive.clone(),
            },
            NoopDeactivationHandler,
        );
        Self {
            adapter: RefCell::new(adapter),
            latest,
            pending_actions,
            action_wake,
            action_wake_task: RefCell::new(None),
            alive,
        }
    }

    /// Route AT-SPI actions to this window's foreground executor. The stored
    /// task is cancelled with the accessible root, so it cannot retain a
    /// closed window. `wake` should capture weak callback/window references.
    pub fn set_action_wake(
        &self,
        executor: &crate::ForegroundExecutor,
        mut wake: impl FnMut() + 'static,
    ) {
        let (mut sender, mut receiver) = mpsc::channel(1);
        if self
            .pending_actions
            .lock()
            .is_ok_and(|pending| !pending.is_empty())
        {
            let _ = sender.try_send(());
        }
        if let Ok(mut slot) = self.action_wake.lock() {
            *slot = Some(sender);
        }
        *self.action_wake_task.borrow_mut() = Some(executor.spawn(async move {
            while receiver.next().await.is_some() {
                wake();
            }
        }));
    }

    /// Feed the latest accessibility tree to the AT-SPI2 adapter.
    pub fn update_tree(&self, tree: &crate::AccessibilityTree) {
        let previous = if let Ok(mut guard) = self.latest.lock() {
            guard.replace(tree.clone())
        } else {
            None
        };
        self.adapter.borrow_mut().update_if_active(|| {
            tree.to_accesskit_tree_update_after(
                previous.as_ref(),
                Some(TOOLKIT_NAME),
                Some(TOOLKIT_VERSION),
            )
        });
    }

    /// Drain action requests received from assistive technology, normalized
    /// against the latest kael accessibility tree.
    pub fn drain_actions(
        &self,
        tree: &crate::AccessibilityTree,
    ) -> Vec<crate::AccessibilityActionRequest> {
        let mut out = Vec::new();
        let pending = self
            .pending_actions
            .lock()
            .map(|mut pending| pending.take())
            .unwrap_or_default();
        out.extend(
            pending
                .into_iter()
                .filter_map(|request| request.normalize(tree)),
        );
        out
    }
}

impl Drop for AtSpiAccessibleRoot {
    fn drop(&mut self) {
        self.alive.store(false, Ordering::Release);
        if let Ok(mut sender) = self.action_wake.lock() {
            sender.take();
        }
        self.action_wake_task.get_mut().take();
        if let Ok(mut pending) = self.pending_actions.lock() {
            pending.take();
        }
    }
}

/// Check whether the AT-SPI2 accessibility bus is available on this system.
///
/// On Linux, AT-SPI2 does not require special permissions; any application can
/// register with the accessibility bus. AccessKit handles the actual bus
/// handshake internally, so this always reports `Granted`.
pub fn accessibility_status() -> PermissionStatus {
    PermissionStatus::Granted
}

#[cfg(test)]
mod action_queue_tests {
    use super::*;

    fn request(id: u64) -> ActionRequest {
        ActionRequest {
            action: accesskit::Action::Click,
            target_tree: accesskit::TreeId::ROOT,
            target_node: accesskit::NodeId(id),
            data: None,
        }
    }

    #[test]
    fn background_at_spi_batches_emit_one_foreground_signal() {
        let pending = Arc::new(Mutex::new(PendingActionQueue::default()));
        let (sender, mut receiver) = mpsc::channel(1);
        let wake = Arc::new(Mutex::new(Some(sender)));
        let mut handler = CollectingActionHandler {
            latest: Arc::new(Mutex::new(None)),
            pending: pending.clone(),
            wake: wake.clone(),
            alive: Arc::new(AtomicBool::new(true)),
        };
        std::thread::spawn(move || {
            for id in 1..=3 {
                handler.do_action(request(id));
            }
        })
        .join()
        .unwrap();
        assert_eq!(receiver.try_recv(), Ok(()));
        assert!(receiver.try_recv().is_err());
        assert_eq!(
            pending
                .lock()
                .unwrap()
                .take()
                .into_iter()
                .map(|r| match r {
                    crate::accessibility::PendingAccessibilityAction::Raw(r) => r.target_node.0,
                    _ => panic!("expected raw native request"),
                })
                .collect::<Vec<_>>(),
            [1, 2, 3]
        );
        CollectingActionHandler {
            latest: Arc::new(Mutex::new(None)),
            pending,
            wake,
            alive: Arc::new(AtomicBool::new(true)),
        }
        .do_action(request(4));
        assert_eq!(receiver.try_recv(), Ok(()));
    }

    #[test]
    fn disconnected_window_and_request_flood_remain_bounded() {
        let pending = Arc::new(Mutex::new(PendingActionQueue::default()));
        let (sender, receiver) = mpsc::channel(1);
        drop(receiver);
        let mut handler = CollectingActionHandler {
            latest: Arc::new(Mutex::new(None)),
            pending: pending.clone(),
            wake: Arc::new(Mutex::new(Some(sender))),
            alive: Arc::new(AtomicBool::new(true)),
        };
        for id in 0..PendingActionQueue::MAX_REQUESTS + 500 {
            handler.do_action(request(id as u64));
        }
        assert_eq!(
            pending.lock().unwrap().len(),
            PendingActionQueue::MAX_REQUESTS
        );
    }

    #[test]
    fn retained_at_spi_handler_cannot_queue_after_window_drop() {
        let pending = Arc::new(Mutex::new(PendingActionQueue::default()));
        let (sender, mut receiver) = mpsc::channel(1);
        let mut handler = CollectingActionHandler {
            latest: Arc::new(Mutex::new(None)),
            pending: pending.clone(),
            wake: Arc::new(Mutex::new(Some(sender))),
            alive: Arc::new(AtomicBool::new(false)),
        };
        handler.do_action(request(1));
        assert!(pending.lock().unwrap().is_empty());
        assert!(receiver.try_recv().is_err());
    }

    #[test]
    fn atomic_text_worker_transport_keeps_fifo_wakes_and_document_identity() {
        use crate::{
            AccessibilityAction as Action, AccessibilityNode, AccessibilityRole,
            AccessibilityTextDocument, AccessibilityTextSelection, AccessibilityTree,
        };
        fn tree(
            document: Arc<AccessibilityTextDocument>,
            owner: crate::AccessibilityId,
        ) -> AccessibilityTree {
            let mut root = AccessibilityNode::new(AccessibilityRole::TextInput);
            root.id = owner;
            root.actions = vec![
                Action::Click,
                Action::ReplaceSelectedText,
                Action::CopyText,
                Action::CutText,
                Action::PasteText,
            ];
            root.text_document = Some(document);
            root.text_selection = Some(AccessibilityTextSelection {
                anchor: 1,
                focus: 7,
            });
            AccessibilityTree::new(root)
        }
        fn selection(tree: &AccessibilityTree) -> accesskit::TextSelection {
            *tree.to_accesskit_tree_update(None, None).nodes[0]
                .1
                .text_selection()
                .unwrap()
        }
        let document = AccessibilityTextDocument::new("A日本🙂");
        let owner = crate::AccessibilityId::new();
        let original = tree(document, owner);
        let raw_selection = selection(&original);
        let latest = Arc::new(Mutex::new(Some(original.clone())));
        let pending = Arc::new(Mutex::new(PendingActionQueue::default()));
        let (sender, mut receiver) = mpsc::channel(1);
        let alive = Arc::new(AtomicBool::new(true));
        let mut handler = CollectingActionHandler {
            latest: latest.clone(),
            pending: pending.clone(),
            wake: Arc::new(Mutex::new(Some(sender))),
            alive: alive.clone(),
        };
        handler = std::thread::spawn(move || {
            handler.do_action(request(owner.0));
            for operation in [
                TextEditOperation::Replace("新🙂".into()),
                TextEditOperation::Copy,
            ] {
                assert!(handler.edit_text(TextEditRequest {
                    target_tree: accesskit::TreeId::ROOT,
                    target_node: accesskit::NodeId(owner.0),
                    selection: raw_selection,
                    operation
                }));
            }
            handler
        })
        .join()
        .unwrap();
        assert_eq!(receiver.try_recv(), Ok(()));
        assert!(
            receiver.try_recv().is_err(),
            "one batch produces one foreground wake"
        );
        let batch = pending.lock().unwrap().take();
        assert_eq!(
            batch
                .iter()
                .cloned()
                .filter_map(|event| event.normalize(&original))
                .map(|event| event.action)
                .collect::<Vec<_>>(),
            [Action::Click, Action::ReplaceSelectedText, Action::CopyText]
        );
        let replacement = tree(AccessibilityTextDocument::new("A別語🙂"), owner);
        assert_eq!(
            batch
                .iter()
                .cloned()
                .filter_map(|event| event.normalize(&replacement))
                .count(),
            1,
            "queued atomic text operations are rejected after document replacement; ordinary Click remains valid"
        );
        *latest.lock().unwrap() = Some(replacement);
        assert!(!handler.edit_text(TextEditRequest {
            target_tree: accesskit::TreeId::ROOT,
            target_node: accesskit::NodeId(owner.0),
            selection: raw_selection,
            operation: TextEditOperation::Cut
        }));
        alive.store(false, Ordering::Release);
        assert!(!handler.edit_text(TextEditRequest {
            target_tree: accesskit::TreeId::ROOT,
            target_node: accesskit::NodeId(owner.0),
            selection: raw_selection,
            operation: TextEditOperation::Copy
        }));
        assert!(pending.lock().unwrap().is_empty());
    }
}
