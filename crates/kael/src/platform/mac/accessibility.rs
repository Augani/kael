#![cfg_attr(not(test), allow(dead_code))]

//! NSAccessibility integration for the macOS backend via AccessKit.
//!
//! The platform window owns a [`MacAccessibilityProvider`] that wraps an
//! [`accesskit_macos::SubclassingAdapter`]. The adapter dynamically subclasses
//! the window's `NSView` so AppKit serves a full `NSAccessibility` tree to
//! VoiceOver. Each frame, the shared [`crate::AccessibilityTree`] is converted
//! to an [`accesskit::TreeUpdate`] and fed to the adapter; action requests from
//! assistive technology are collected for the window to route back into kael.

use std::ffi::c_void;
use std::sync::{Arc, Mutex};

use accesskit::{ActionHandler, ActionRequest, ActivationHandler, TreeUpdate};
use accesskit_macos::SubclassingAdapter;

use crate::{AccessibilityAction, AccessibilityRole, AccessibilityState, AccessibilityValue};

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
        AccessibilityRole::Group => "NSAccessibilityGroupRole",
        AccessibilityRole::List => "NSAccessibilityListRole",
        AccessibilityRole::ListItem => "NSAccessibilityStaticTextRole",
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
        AccessibilityAction::Expand => "NSAccessibilityShowMenuAction",
        AccessibilityAction::Collapse => "NSAccessibilityCancelAction",
        AccessibilityAction::Toggle => "NSAccessibilityPressAction",
        AccessibilityAction::Increment => "NSAccessibilityIncrementAction",
        AccessibilityAction::Decrement => "NSAccessibilityDecrementAction",
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

type SharedUpdate = Arc<Mutex<Option<TreeUpdate>>>;
type PendingActions = Arc<Mutex<Vec<ActionRequest>>>;

struct InitialTreeHandler {
    latest: SharedUpdate,
}

impl ActivationHandler for InitialTreeHandler {
    fn request_initial_tree(&mut self) -> Option<TreeUpdate> {
        self.latest.lock().ok().and_then(|guard| guard.clone())
    }
}

struct CollectingActionHandler {
    pending: PendingActions,
}

impl ActionHandler for CollectingActionHandler {
    fn do_action(&mut self, request: ActionRequest) {
        if let Ok(mut pending) = self.pending.lock() {
            pending.push(request);
        }
    }
}

/// A live NSAccessibility provider for a GPUI window, backed by AccessKit.
pub struct MacAccessibilityProvider {
    app_name: String,
    adapter: Option<SubclassingAdapter>,
    latest: SharedUpdate,
    pending_actions: PendingActions,
}

impl MacAccessibilityProvider {
    /// Create a provider that subclasses the given `NSView` to serve an
    /// `NSAccessibility` tree.
    ///
    /// # Safety
    ///
    /// `view` must be a valid, unreleased pointer to an `NSView` that outlives
    /// this provider.
    pub unsafe fn new(app_name: &str, view: *mut c_void) -> Self {
        let latest: SharedUpdate = Arc::new(Mutex::new(None));
        let pending_actions: PendingActions = Arc::new(Mutex::new(Vec::new()));
        let activation_handler = InitialTreeHandler {
            latest: latest.clone(),
        };
        let action_handler = CollectingActionHandler {
            pending: pending_actions.clone(),
        };
        let adapter = unsafe { SubclassingAdapter::new(view, activation_handler, action_handler) };
        Self {
            app_name: app_name.to_string(),
            adapter: Some(adapter),
            latest,
            pending_actions,
        }
    }

    /// Create a provider without a backing adapter (used in tests where no
    /// `NSView` is available).
    pub fn detached(app_name: &str) -> Self {
        Self {
            app_name: app_name.to_string(),
            adapter: None,
            latest: Arc::new(Mutex::new(None)),
            pending_actions: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Return the application name exposed to accessibility clients.
    pub fn app_name(&self) -> &str {
        &self.app_name
    }

    /// Feed the latest accessibility tree to the AccessKit adapter.
    pub fn update_tree(&mut self, tree: &crate::AccessibilityTree) {
        let update = tree.to_accesskit_tree_update(
            Some(self.app_name.as_str()),
            Some(TOOLKIT_NAME),
            Some(TOOLKIT_VERSION),
        );
        if let Ok(mut guard) = self.latest.lock() {
            *guard = Some(update.clone());
        }
        if let Some(adapter) = self.adapter.as_mut() {
            if let Some(events) = adapter.update_if_active(|| update) {
                events.raise();
            }
        }
    }

    /// Notify the adapter that the window's focus state changed.
    pub fn update_view_focus_state(&mut self, is_focused: bool) {
        if let Some(adapter) = self.adapter.as_mut() {
            if let Some(events) = adapter.update_view_focus_state(is_focused) {
                events.raise();
            }
        }
    }

    /// Drain any action requests received from assistive technology, translated
    /// into kael's [`AccessibilityAction`] plus the target node id.
    pub fn drain_actions(&self) -> Vec<(crate::AccessibilityId, AccessibilityAction)> {
        let mut out = Vec::new();
        if let Ok(mut pending) = self.pending_actions.lock() {
            for request in pending.drain(..) {
                if let Some(action) = AccessibilityAction::from_accesskit(request.action) {
                    out.push((crate::AccessibilityId(request.target.0), action));
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
        let provider = MacAccessibilityProvider::detached("test-app");
        assert_eq!(provider.app_name(), "test-app");
    }

    #[test]
    fn test_detached_provider_stores_latest_update() {
        let mut provider = MacAccessibilityProvider::detached("test-app");
        let root = crate::AccessibilityNode::new(AccessibilityRole::Window);
        let mut tree = crate::AccessibilityTree::new(root);
        let button = crate::AccessibilityNode::new(AccessibilityRole::Button).with_label("OK");
        let button_id = button.id;
        tree.insert(button);
        tree.set_parent(button_id, tree.root);

        provider.update_tree(&tree);

        let stored = provider.latest.lock().unwrap();
        let update = stored.as_ref().expect("update stored");
        assert!(update.tree.is_some());
        assert!(update.nodes.iter().any(|(id, _)| id.0 == button_id.0));
    }

    #[test]
    fn test_detached_provider_has_no_pending_actions() {
        let provider = MacAccessibilityProvider::detached("test-app");
        assert!(provider.drain_actions().is_empty());
    }
}
