#![cfg_attr(not(test), allow(dead_code))]

//! NSAccessibility integration for the macOS backend via AccessKit.
//!
//! The platform window owns a [`MacAccessibilityProvider`] that wraps an
//! [`accesskit_macos::SubclassingAdapter`]. The adapter dynamically subclasses
//! the window's `NSView` so AppKit serves a full `NSAccessibility` tree to
//! VoiceOver. Each frame, the shared [`crate::AccessibilityTree`] is converted
//! to an [`accesskit::TreeUpdate`] and fed to the adapter; action requests from
//! assistive technology are collected for the window to route back into kael.

use std::cell::RefCell;
use std::ffi::c_void;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use accesskit::{ActionHandler, ActionRequest, ActivationHandler, TreeUpdate};
use accesskit_macos::SubclassingAdapter;

use crate::{
    AccessibilityAction, AccessibilityActionRequest, AccessibilityRole, AccessibilityState,
    AccessibilityValue, accessibility::PendingActionQueue,
};

const TOOLKIT_NAME: &str = "Kael";
const TOOLKIT_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Map a GPUI [`AccessibilityRole`] to an NSAccessibility role string.
pub fn role_to_ns_accessibility(role: AccessibilityRole) -> &'static str {
    match role {
        AccessibilityRole::Application => "NSAccessibilityApplicationRole",
        AccessibilityRole::Window => "NSAccessibilityWindowRole",
        AccessibilityRole::Button => "NSAccessibilityButtonRole",
        AccessibilityRole::TextInput => "NSAccessibilityTextFieldRole",
        AccessibilityRole::StaticText => "NSAccessibilityStaticTextRole",
        AccessibilityRole::Heading => "NSAccessibilityHeadingRole",
        AccessibilityRole::Group => "NSAccessibilityGroupRole",
        AccessibilityRole::List => "NSAccessibilityListRole",
        AccessibilityRole::ListItem => "NSAccessibilityStaticTextRole",
        AccessibilityRole::Table | AccessibilityRole::Grid => "NSAccessibilityTableRole",
        AccessibilityRole::Row => "NSAccessibilityRowRole",
        AccessibilityRole::Cell
        | AccessibilityRole::ColumnHeader
        | AccessibilityRole::RowHeader => "NSAccessibilityCellRole",
        AccessibilityRole::ScrollBar => "NSAccessibilityScrollBarRole",
        AccessibilityRole::Image => "NSAccessibilityImageRole",
        AccessibilityRole::Link => "NSAccessibilityLinkRole",
        AccessibilityRole::Menu => "NSAccessibilityMenuRole",
        AccessibilityRole::MenuItem => "NSAccessibilityMenuItemRole",
        AccessibilityRole::Tab => "NSAccessibilityRadioButtonRole",
        AccessibilityRole::TabPanel => "NSAccessibilityTabGroupRole",
        AccessibilityRole::Toolbar => "NSAccessibilityToolbarRole",
        AccessibilityRole::Tree => "NSAccessibilityOutlineRole",
        AccessibilityRole::TreeItem => "NSAccessibilityRowRole",
        AccessibilityRole::CheckBox => "NSAccessibilityCheckBoxRole",
        AccessibilityRole::RadioButton => "NSAccessibilityRadioButtonRole",
        AccessibilityRole::Slider => "NSAccessibilitySliderRole",
        AccessibilityRole::ProgressBar => "NSAccessibilityProgressIndicatorRole",
        AccessibilityRole::Separator => "NSAccessibilitySplitterRole",
        AccessibilityRole::Pane => "NSAccessibilityGroupRole",
        AccessibilityRole::Dialog => "NSAccessibilityDialogRole",
        AccessibilityRole::Alert => "NSAccessibilityAlertRole",
        AccessibilityRole::ComboBox => "NSAccessibilityComboBoxRole",
        AccessibilityRole::Switch => "NSAccessibilityCheckBoxRole",
        AccessibilityRole::Unknown => "NSAccessibilityUnknownRole",
    }
}

/// Map a GPUI [`AccessibilityState`] to NSAccessibility state attributes.
pub fn states_to_ns_attributes(states: AccessibilityState) -> Vec<&'static str> {
    let mut attrs = Vec::new();
    if states.contains(AccessibilityState::FOCUSED) {
        attrs.push("NSAccessibilityFocusedAttribute");
    }
    if states.contains(AccessibilityState::DISABLED) {
        attrs.push("NSAccessibilityEnabledAttribute");
    }
    if states.contains(AccessibilityState::SELECTED) {
        attrs.push("NSAccessibilitySelectedAttribute");
    }
    if states.contains(AccessibilityState::EXPANDED) {
        attrs.push("NSAccessibilityExpandedAttribute");
    }
    if states.contains(AccessibilityState::CHECKED) {
        attrs.push("NSAccessibilityValueAttribute");
    }
    attrs
}

/// Map a GPUI [`AccessibilityAction`] to an NSAccessibility action name.
pub fn action_to_ns_action(action: AccessibilityAction) -> &'static str {
    match action {
        AccessibilityAction::Click => "NSAccessibilityPressAction",
        AccessibilityAction::Focus => "NSAccessibilityRaiseAction",
        AccessibilityAction::ScrollUp => "NSAccessibilityScrollUpByPageAction",
        AccessibilityAction::ScrollDown => "NSAccessibilityScrollDownByPageAction",
        AccessibilityAction::ScrollToVisible => "NSAccessibilityScrollToVisibleAction",
        AccessibilityAction::Expand => "NSAccessibilityShowMenuAction",
        AccessibilityAction::Collapse => "NSAccessibilityCancelAction",
        AccessibilityAction::Toggle => "NSAccessibilityPressAction",
        AccessibilityAction::Increment => "NSAccessibilityIncrementAction",
        AccessibilityAction::Decrement => "NSAccessibilityDecrementAction",
        AccessibilityAction::SetValue => "NSAccessibilitySetValueAction",
        AccessibilityAction::SetTextSelection => "NSAccessibilitySelectedTextRangeAttribute",
        AccessibilityAction::ShowMenu => "NSAccessibilityShowMenuAction",
        AccessibilityAction::Dismiss => "NSAccessibilityCancelAction",
        AccessibilityAction::Custom(_) => "NSAccessibilityPressAction",
    }
}

/// Convert a GPUI [`AccessibilityValue`] to a string representation for NSAccessibility.
pub fn value_to_string(value: &AccessibilityValue) -> String {
    match value {
        AccessibilityValue::Text(text) => text.clone(),
        AccessibilityValue::Number(num) => num.to_string(),
        AccessibilityValue::Range { current, .. } => current.to_string(),
        AccessibilityValue::Toggle(val) => val.to_string(),
    }
}

type SharedUpdate = Arc<Mutex<Option<crate::AccessibilityTree>>>;

type PendingActions = Arc<Mutex<PendingActionQueue>>;
type ActionWake = Rc<RefCell<Option<Rc<dyn Fn()>>>>;

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
    pending: PendingActions,
    wake: ActionWake,
}

impl ActionHandler for CollectingActionHandler {
    fn do_action(&mut self, request: ActionRequest) {
        let (needs_wake, first_overflow) = if let Ok(mut pending) = self.pending.lock() {
            let was_empty = pending.is_empty();
            debug_assert_eq!(was_empty, pending.len() == 0);
            let accepted = pending.push(request);
            (accepted && was_empty, !accepted && pending.dropped() == 1)
        } else {
            (false, false)
        };
        // Bound retained requests and string values while preserving every
        // admitted Click/Toggle/Increment in FIFO order. Report once per batch,
        // outside the queue lock; native clients may otherwise flood logs too.
        if first_overflow {
            PendingActionQueue::report_overflow();
        }
        // Request a foreground frame after releasing the queue and callback
        // borrows. Idle windows otherwise never drain these actions, and a
        // reentrant callback must be free to inspect or drain the queue.
        if needs_wake {
            let wake = self.wake.borrow().clone();
            if let Some(wake) = wake {
                wake();
            }
        }
    }
}

/// A live NSAccessibility provider for a GPUI window, backed by AccessKit.
pub struct MacAccessibilityProvider {
    adapter: Option<SubclassingAdapter>,
    latest: SharedUpdate,
    pending_actions: PendingActions,
    action_wake: ActionWake,
    previous_tree: Option<crate::AccessibilityTree>,
}

impl MacAccessibilityProvider {
    /// Create a provider that subclasses the given `NSView` to serve an
    /// `NSAccessibility` tree.
    ///
    /// # Safety
    ///
    /// `view` must be a valid, unreleased pointer to an `NSView` that outlives
    /// this provider.
    pub unsafe fn new(view: *mut c_void) -> Self {
        let latest: SharedUpdate = Arc::new(Mutex::new(None));
        let pending_actions: PendingActions = Arc::new(Mutex::new(PendingActionQueue::default()));
        let action_wake: ActionWake = Rc::new(RefCell::new(None));
        let activation_handler = InitialTreeHandler {
            latest: latest.clone(),
        };
        let action_handler = CollectingActionHandler {
            pending: pending_actions.clone(),
            wake: action_wake.clone(),
        };
        let adapter = unsafe { SubclassingAdapter::new(view, activation_handler, action_handler) };
        Self {
            adapter: Some(adapter),
            latest,
            pending_actions,
            action_wake,
            previous_tree: None,
        }
    }

    /// Create a provider without a backing adapter (used in tests where no
    /// `NSView` is available).
    pub fn detached() -> Self {
        Self {
            adapter: None,
            latest: Arc::new(Mutex::new(None)),
            pending_actions: Arc::new(Mutex::new(PendingActionQueue::default())),
            action_wake: Rc::new(RefCell::new(None)),
            previous_tree: None,
        }
    }

    /// Wake the window after a new batch of accessibility actions is queued.
    ///
    /// AccessKit's macOS adapter calls this on the main thread, outside queue
    /// locks. Schedule work on the foreground executor and retain the window
    /// weakly so the adapter cannot keep a closed window alive.
    pub fn set_action_wake(&mut self, wake: impl Fn() + 'static) {
        *self.action_wake.borrow_mut() = Some(Rc::new(wake));
    }

    /// Feed the latest accessibility tree to the AccessKit adapter.
    pub fn update_tree(&mut self, tree: &crate::AccessibilityTree) {
        // Activation builds a full tree on demand. Ordinary frames retain the
        // cheap semantic snapshot and publish only changed AccessKit records.
        if let Ok(mut guard) = self.latest.lock() {
            *guard = Some(tree.clone());
        }
        if let Some(adapter) = self.adapter.as_mut() {
            if let Some(events) = adapter.update_if_active(|| {
                tree.to_accesskit_tree_update_after(
                    self.previous_tree.as_ref(),
                    Some(TOOLKIT_NAME),
                    Some(TOOLKIT_VERSION),
                )
            }) {
                events.raise();
            }
        }
        self.previous_tree = Some(tree.clone());
    }

    /// Notify the adapter that the window's focus state changed.
    pub fn update_view_focus_state(&mut self, is_focused: bool) {
        if let Some(adapter) = self.adapter.as_mut() {
            if let Some(events) = adapter.update_view_focus_state(is_focused) {
                events.raise();
            }
        }
    }

    /// Drain any action requests received from assistive technology, normalized
    /// against the latest kael accessibility tree.
    pub fn drain_actions(
        &self,
        tree: &crate::AccessibilityTree,
    ) -> Vec<AccessibilityActionRequest> {
        let mut out = Vec::new();
        let requests = self
            .pending_actions
            .lock()
            .map(|mut pending| pending.take())
            .unwrap_or_default();
        for request in requests {
            if request.target_tree != accesskit::TreeId::ROOT {
                continue;
            }
            let node_id = crate::AccessibilityId(request.target_node.0);
            if let Some(node) = tree.get(node_id) {
                if let Some(request) = AccessibilityActionRequest::from_accesskit_for_node_with_data(
                    node_id,
                    node,
                    request.action,
                    request.data,
                ) {
                    out.push(request);
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_role_to_ns_mapping() {
        assert_eq!(
            role_to_ns_accessibility(AccessibilityRole::Button),
            "NSAccessibilityButtonRole"
        );
        assert_eq!(
            role_to_ns_accessibility(AccessibilityRole::StaticText),
            "NSAccessibilityStaticTextRole"
        );
        assert_eq!(
            role_to_ns_accessibility(AccessibilityRole::List),
            "NSAccessibilityListRole"
        );
        assert_eq!(
            role_to_ns_accessibility(AccessibilityRole::Unknown),
            "NSAccessibilityUnknownRole"
        );
    }

    #[test]
    fn test_states_to_attrs() {
        let states = AccessibilityState::FOCUSED | AccessibilityState::CHECKED;
        let attrs = states_to_ns_attributes(states);
        assert!(attrs.contains(&"NSAccessibilityFocusedAttribute"));
        assert!(attrs.contains(&"NSAccessibilityValueAttribute"));
    }

    #[test]
    fn test_action_to_ns_action() {
        assert_eq!(
            action_to_ns_action(AccessibilityAction::Click),
            "NSAccessibilityPressAction"
        );
        assert_eq!(
            action_to_ns_action(AccessibilityAction::Toggle),
            "NSAccessibilityPressAction"
        );
    }

    #[test]
    fn test_value_to_string() {
        assert_eq!(
            value_to_string(&AccessibilityValue::Text("hello".to_string())),
            "hello"
        );
        assert_eq!(value_to_string(&AccessibilityValue::Number(42.0)), "42");
        assert_eq!(value_to_string(&AccessibilityValue::Toggle(true)), "true");
    }

    #[test]
    fn test_provider_creation() {
        let provider = MacAccessibilityProvider::detached();
        assert!(provider.adapter.is_none());
    }

    #[test]
    fn test_detached_provider_stores_latest_update() {
        let mut provider = MacAccessibilityProvider::detached();
        let root = crate::AccessibilityNode::new(AccessibilityRole::Window);
        let mut tree = crate::AccessibilityTree::new(root);
        let button = crate::AccessibilityNode::new(AccessibilityRole::Button).with_label("OK");
        let button_id = button.id;
        tree.insert(button);
        tree.set_parent(button_id, tree.root);

        provider.update_tree(&tree);

        let stored = provider.latest.lock().unwrap();
        let update = stored
            .as_ref()
            .expect("snapshot stored")
            .to_accesskit_tree_update(None, None);
        assert!(update.tree.is_some());
        assert!(update.nodes.iter().any(|(id, _)| id.0 == button_id.0));
    }

    #[test]
    fn test_detached_provider_has_no_pending_actions() {
        let provider = MacAccessibilityProvider::detached();
        let tree =
            crate::AccessibilityTree::new(crate::AccessibilityNode::new(AccessibilityRole::Window));
        assert!(provider.drain_actions(&tree).is_empty());
    }

    #[test]
    fn test_detached_provider_normalizes_pending_actions_against_tree() {
        let provider = MacAccessibilityProvider::detached();
        let root = crate::AccessibilityNode::new(AccessibilityRole::Window);
        let mut tree = crate::AccessibilityTree::new(root);
        let toggle = crate::AccessibilityNode::new(AccessibilityRole::Switch).with_actions(vec![
            AccessibilityAction::Focus,
            AccessibilityAction::Toggle,
        ]);
        let toggle_id = toggle.id;
        tree.insert(toggle);
        tree.set_parent(toggle_id, tree.root);

        provider
            .pending_actions
            .lock()
            .unwrap()
            .push(accesskit::ActionRequest {
                action: accesskit::Action::Click,
                target_tree: accesskit::TreeId::ROOT,
                target_node: accesskit::NodeId(toggle_id.0),
                data: None,
            });

        assert_eq!(
            provider.drain_actions(&tree),
            vec![AccessibilityActionRequest::new(
                toggle_id,
                AccessibilityAction::Toggle,
            )]
        );
        assert!(provider.drain_actions(&tree).is_empty());
    }

    fn clickable_tree() -> (crate::AccessibilityTree, crate::AccessibilityId) {
        let mut tree =
            crate::AccessibilityTree::new(crate::AccessibilityNode::new(AccessibilityRole::Window));
        let button = crate::AccessibilityNode::new(AccessibilityRole::Button)
            .with_label("Activate")
            .with_actions(vec![AccessibilityAction::Click]);
        let button_id = button.id;
        tree.insert(button);
        tree.set_parent(button_id, tree.root);
        (tree, button_id)
    }

    fn press_request(id: crate::AccessibilityId) -> ActionRequest {
        ActionRequest {
            action: accesskit::Action::Click,
            target_tree: accesskit::TreeId::ROOT,
            target_node: accesskit::NodeId(id.0),
            data: None,
        }
    }

    #[test]
    fn queued_actions_wake_once_per_batch_and_preserve_every_request() {
        use std::cell::Cell;
        let mut provider = MacAccessibilityProvider::detached();
        let wakes = Rc::new(Cell::new(0));
        let wake_count = wakes.clone();
        provider.set_action_wake(move || wake_count.set(wake_count.get() + 1));
        let mut handler = CollectingActionHandler {
            pending: provider.pending_actions.clone(),
            wake: provider.action_wake.clone(),
        };
        let (tree, button_id) = clickable_tree();
        for _ in 0..3 {
            handler.do_action(press_request(button_id));
        }
        assert_eq!(
            wakes.get(),
            1,
            "an idle window needs only one wake for a queued batch"
        );
        assert_eq!(provider.drain_actions(&tree).len(), 3);
        handler.do_action(press_request(button_id));
        assert_eq!(
            wakes.get(),
            2,
            "a drained queue must wake again for the next action"
        );
        assert_eq!(
            provider.drain_actions(&tree),
            vec![AccessibilityActionRequest::new(
                button_id,
                AccessibilityAction::Click,
            )]
        );
    }

    #[test]
    fn action_wake_runs_outside_queue_and_callback_borrows() {
        use std::cell::Cell;
        let mut provider = MacAccessibilityProvider::detached();
        let unlocked = Rc::new(Cell::new(false));
        let checked = unlocked.clone();
        let queue = provider.pending_actions.clone();
        let wake_slot = provider.action_wake.clone();
        provider.set_action_wake(move || {
            let pending = queue
                .try_lock()
                .expect("wake callback must not hold the queue mutex");
            assert_eq!(pending.len(), 1);
            drop(pending);
            // Reentrant wake replacement must not panic due to a held RefCell borrow.
            *wake_slot.borrow_mut() = None;
            checked.set(true);
        });
        let mut handler = CollectingActionHandler {
            pending: provider.pending_actions.clone(),
            wake: provider.action_wake.clone(),
        };
        handler.do_action(press_request(crate::AccessibilityId(1)));
        assert!(unlocked.get());
        assert!(provider.action_wake.borrow().is_none());
    }

    #[test]
    fn flooded_native_actions_are_bounded_and_do_not_multiply_wakes() {
        use std::cell::Cell;
        let mut provider = MacAccessibilityProvider::detached();
        let wakes = Rc::new(Cell::new(0));
        let wake_count = wakes.clone();
        provider.set_action_wake(move || wake_count.set(wake_count.get() + 1));
        let mut handler = CollectingActionHandler {
            pending: provider.pending_actions.clone(),
            wake: provider.action_wake.clone(),
        };
        let (tree, button_id) = clickable_tree();
        for _ in 0..PendingActionQueue::MAX_REQUESTS + 100 {
            handler.do_action(press_request(button_id));
        }
        assert_eq!(wakes.get(), 1);
        assert_eq!(provider.pending_actions.lock().unwrap().dropped(), 100);
        let accepted = provider.drain_actions(&tree);
        assert_eq!(accepted.len(), PendingActionQueue::MAX_REQUESTS);
        assert!(
            accepted
                .iter()
                .all(|request| request.action == AccessibilityAction::Click)
        );
        assert_eq!(provider.pending_actions.lock().unwrap().dropped(), 0);
        handler.do_action(press_request(button_id));
        assert_eq!(wakes.get(), 2);
        assert_eq!(provider.drain_actions(&tree).len(), 1);
    }

    #[test]
    fn dropping_provider_releases_its_wake_callback() {
        let lifetime = Rc::new(());
        let weak = Rc::downgrade(&lifetime);
        let mut provider = MacAccessibilityProvider::detached();
        provider.set_action_wake(move || {
            let _ = &lifetime;
        });
        assert!(weak.upgrade().is_some());
        drop(provider);
        assert!(
            weak.upgrade().is_none(),
            "wake callbacks must not outlive their provider"
        );
    }
}
