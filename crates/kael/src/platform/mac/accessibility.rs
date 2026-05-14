#![cfg_attr(not(test), allow(dead_code))]

//! NSAccessibility mapping stubs for the macOS backend.
//!
//! This module maps GPUI's shared accessibility types to macOS NSAccessibility
//! role constants. The actual NSAccessibility integration would be implemented
//! in the platform window, but these stubs provide the role conversion layer.

use crate::{AccessibilityAction, AccessibilityRole, AccessibilityState, AccessibilityValue};

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

/// A live NSAccessibility provider for a GPUI window.
///
/// This struct maps GPUI's shared accessibility tree to NSAccessibility
/// role constants and holds the current tree so the platform window can
/// update the native NSView's accessibility properties.
pub struct MacAccessibilityProvider {
    app_name: String,
    tree: Option<crate::AccessibilityTree>,
}

impl MacAccessibilityProvider {
    /// Create a new provider with the given application name.
    pub fn new(app_name: &str) -> Self {
        Self {
            app_name: app_name.to_string(),
            tree: None,
        }
    }

    /// Return the application name exposed to accessibility clients.
    pub fn app_name(&self) -> &str {
        &self.app_name
    }

    /// Update the provider with the latest accessibility tree.
    pub fn update_tree(&mut self, tree: &crate::AccessibilityTree) {
        self.tree = Some(tree.clone());
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
        let provider = MacAccessibilityProvider::new("test-app");
        assert_eq!(provider.app_name(), "test-app");
    }
}
