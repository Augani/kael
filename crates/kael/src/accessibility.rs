//! Shared accessibility model and semantic role system for GPUI.
//!
//! This module defines the cross-platform accessibility tree primitives:
//! roles, states, labels, values, and actions. Platform backends (macOS
//! NSAccessibility, Windows UIA, Linux AT-SPI2) map these shared types to
//! their native equivalents.
//!
//! # Integration with GPUI elements
//!
//! Elements and views can declare accessibility metadata via
//! [`AccessibilityAttributes`]. The framework collects these declarations
//! during layout and exposes them to the platform accessibility layer.

use std::sync::atomic::{AtomicU64, Ordering};

// ---------------------------------------------------------------------------
// Identity
// ---------------------------------------------------------------------------

static NEXT_ACCESSIBILITY_ID: AtomicU64 = AtomicU64::new(1);

/// A stable identifier for an accessible node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AccessibilityId(pub u64);

impl AccessibilityId {
    /// Generate a new unique accessibility identifier.
    pub fn new() -> Self {
        Self(NEXT_ACCESSIBILITY_ID.fetch_add(1, Ordering::Relaxed))
    }
}

impl Default for AccessibilityId {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Roles
// ---------------------------------------------------------------------------

/// Semantic roles for accessible elements.
///
/// Roles are intentionally coarse-grained and product-oriented. Platform
/// backends map each role to the closest native equivalent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AccessibilityRole {
    /// The root application container.
    Application,
    /// A top-level window.
    Window,
    /// An interactive button.
    Button,
    /// A text input field.
    TextInput,
    /// Non-editable text content.
    StaticText,
    /// A generic grouping container.
    Group,
    /// A list of items.
    List,
    /// A single item within a list.
    ListItem,
    /// A scroll bar control.
    ScrollBar,
    /// An image or icon.
    Image,
    /// A hyperlink.
    Link,
    /// A menu containing menu items.
    Menu,
    /// A single item within a menu.
    MenuItem,
    /// A tab in a tab group.
    Tab,
    /// A panel containing tabs.
    TabPanel,
    /// A toolbar containing controls.
    Toolbar,
    /// A hierarchical tree container.
    Tree,
    /// A single item within a tree.
    TreeItem,
    /// A check box toggle.
    CheckBox,
    /// A radio button in a group.
    RadioButton,
    /// A slider control.
    Slider,
    /// A progress indicator.
    ProgressBar,
    /// A visual separator.
    Separator,
    /// A generic pane region.
    Pane,
    /// A modal dialog.
    Dialog,
    /// An alert or warning banner.
    Alert,
    /// A combo box / drop-down selector.
    ComboBox,
    /// An on/off switch.
    Switch,
    /// An unrecognized or unmapped role.
    Unknown,
}

impl AccessibilityRole {
    /// Returns true if this role is typically interactive.
    pub fn is_interactive(&self) -> bool {
        matches!(
            self,
            AccessibilityRole::Button
                | AccessibilityRole::TextInput
                | AccessibilityRole::Link
                | AccessibilityRole::MenuItem
                | AccessibilityRole::Tab
                | AccessibilityRole::CheckBox
                | AccessibilityRole::RadioButton
                | AccessibilityRole::Slider
                | AccessibilityRole::ComboBox
                | AccessibilityRole::Switch
        )
    }

    /// Returns true if this role can have children in the accessibility tree.
    pub fn is_container(&self) -> bool {
        matches!(
            self,
            AccessibilityRole::Application
                | AccessibilityRole::Window
                | AccessibilityRole::Group
                | AccessibilityRole::List
                | AccessibilityRole::Menu
                | AccessibilityRole::MenuItem
                | AccessibilityRole::TabPanel
                | AccessibilityRole::Tree
                | AccessibilityRole::Pane
                | AccessibilityRole::Dialog
                | AccessibilityRole::Toolbar
        )
    }

    /// Map to the closest AccessKit role (P3-A spike, roadmap §9 action 12).
    pub fn to_accesskit(self) -> accesskit::Role {
        use accesskit::Role;
        match self {
            Self::Application | Self::Window => Role::Window,
            Self::Button => Role::Button,
            Self::TextInput => Role::TextInput,
            Self::StaticText => Role::Label,
            Self::Group | Self::Pane => Role::Group,
            Self::List => Role::List,
            Self::ListItem => Role::ListItem,
            Self::ScrollBar => Role::ScrollBar,
            Self::Image => Role::Image,
            Self::Link => Role::Link,
            Self::Menu => Role::Menu,
            Self::MenuItem => Role::MenuItem,
            Self::Tab => Role::Tab,
            Self::TabPanel => Role::TabPanel,
            Self::Toolbar => Role::Toolbar,
            Self::Tree => Role::Tree,
            Self::TreeItem => Role::TreeItem,
            Self::CheckBox => Role::CheckBox,
            Self::RadioButton => Role::RadioButton,
            Self::Slider => Role::Slider,
            Self::ProgressBar => Role::ProgressIndicator,
            Self::Separator => Role::Splitter,
            Self::Dialog => Role::Dialog,
            Self::Alert => Role::Alert,
            Self::ComboBox => Role::ComboBox,
            Self::Switch => Role::Switch,
            Self::Unknown => Role::Unknown,
        }
    }
}

// ---------------------------------------------------------------------------
// States
// ---------------------------------------------------------------------------

bitflags::bitflags! {
    /// Accessibility state flags for an element.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
    pub struct AccessibilityState: u32 {
        /// No special state.
        const NONE = 0;
        /// The element currently has keyboard focus.
        const FOCUSED = 1 << 0;
        /// The element is disabled and cannot be interacted with.
        const DISABLED = 1 << 1;
        /// The element is selected within a container.
        const SELECTED = 1 << 2;
        /// The element is expanded (e.g., a tree node).
        const EXPANDED = 1 << 3;
        /// The element is collapsed.
        const COLLAPSED = 1 << 4;
        /// The element is checked or toggled on.
        const CHECKED = 1 << 5;
        /// The element is in an indeterminate state.
        const INDETERMINATE = 1 << 6;
        /// The element is currently pressed.
        const PRESSED = 1 << 7;
        /// The element is required for form submission.
        const REQUIRED = 1 << 8;
        /// The element's value is invalid.
        const INVALID = 1 << 9;
        /// The element is busy loading or processing.
        const BUSY = 1 << 10;
        /// The element is hidden from the accessibility tree.
        const HIDDEN = 1 << 11;
    }
}

// ---------------------------------------------------------------------------
// Actions
// ---------------------------------------------------------------------------

/// Actions that assistive technology can invoke on an element.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AccessibilityAction {
    /// Activate the element (e.g., press a button).
    Click,
    /// Move keyboard focus to the element.
    Focus,
    /// Scroll the element upward.
    ScrollUp,
    /// Scroll the element downward.
    ScrollDown,
    /// Expand a collapsible element.
    Expand,
    /// Collapse an expanded element.
    Collapse,
    /// Toggle the element's state (e.g., check box).
    Toggle,
    /// Increment a ranged value.
    Increment,
    /// Decrement a ranged value.
    Decrement,
    /// Open the element's associated menu.
    ShowMenu,
    /// Dismiss the element (e.g., close a dialog).
    Dismiss,
    /// A custom action with an application-defined identifier.
    Custom(u32),
}

// ---------------------------------------------------------------------------
// Value
// ---------------------------------------------------------------------------

/// The current value of an accessible element, if applicable.
#[derive(Debug, Clone, PartialEq)]
pub enum AccessibilityValue {
    /// A textual value.
    Text(String),
    /// A numeric value.
    Number(f64),
    /// A value within a range.
    Range {
        /// The current value.
        current: f64,
        /// The minimum allowed value.
        min: f64,
        /// The maximum allowed value.
        max: f64,
        /// The step increment, if constrained.
        step: Option<f64>,
    },
    /// A boolean toggle value.
    Toggle(bool),
}

// ---------------------------------------------------------------------------
// Node
// ---------------------------------------------------------------------------

/// The screen-space rectangle of an accessibility node, in logical pixels.
///
/// Origin is the top-left corner; `width`/`height` extend right/down. This is the
/// geometry an assistive technology needs to draw focus rings and hit-test, and it
/// maps directly onto [`accesskit::Rect`] (a min/max-corner box).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AccessibilityRect {
    /// Left edge (logical pixels from the window origin).
    pub x: f64,
    /// Top edge (logical pixels from the window origin).
    pub y: f64,
    /// Width in logical pixels.
    pub width: f64,
    /// Height in logical pixels.
    pub height: f64,
}

impl AccessibilityRect {
    /// Construct a rect from an origin and size.
    pub fn new(x: f64, y: f64, width: f64, height: f64) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    /// Convert to an AccessKit min/max-corner rectangle.
    pub fn to_accesskit(&self) -> accesskit::Rect {
        accesskit::Rect {
            x0: self.x,
            y0: self.y,
            x1: self.x + self.width,
            y1: self.y + self.height,
        }
    }
}

/// A single node in the accessibility tree.
#[derive(Debug, Clone, PartialEq)]
pub struct AccessibilityNode {
    /// The unique identifier for this node.
    pub id: AccessibilityId,
    /// The semantic role of this element.
    pub role: AccessibilityRole,
    /// The current state flags.
    pub states: AccessibilityState,
    /// The primary accessible label (e.g., button text).
    pub label: Option<String>,
    /// A longer description, if needed.
    pub description: Option<String>,
    /// The current value, for form-like elements.
    pub value: Option<AccessibilityValue>,
    /// Placeholder text for inputs.
    pub placeholder: Option<String>,
    /// Screen-space bounds, once produced by layout.
    pub bounds: Option<AccessibilityRect>,
    /// Actions that can be invoked on this element.
    pub actions: Vec<AccessibilityAction>,
    /// Child node identifiers.
    pub children: Vec<AccessibilityId>,
    /// Parent node identifier, if any.
    pub parent: Option<AccessibilityId>,
}

impl AccessibilityNode {
    /// Create a new accessibility node with the given role.
    pub fn new(role: AccessibilityRole) -> Self {
        Self {
            id: AccessibilityId::new(),
            role,
            states: AccessibilityState::NONE,
            label: None,
            description: None,
            value: None,
            placeholder: None,
            bounds: None,
            actions: Vec::new(),
            children: Vec::new(),
            parent: None,
        }
    }

    /// Convert this node to an AccessKit node (P3-A, roadmap §9 action 12).
    ///
    /// Maps role, label, description, child ids, and geometry. Bounds are emitted
    /// when [`AccessibilityNode::bounds`] is populated; the remaining P3-A work is
    /// the layout-side wiring that fills those bounds for every laid-out element.
    pub fn to_accesskit_node(&self) -> accesskit::Node {
        let mut node = accesskit::Node::new(self.role.to_accesskit());
        if let Some(label) = &self.label {
            node.set_label(label.as_str());
        }
        if let Some(description) = &self.description {
            node.set_description(description.as_str());
        }
        if let Some(bounds) = &self.bounds {
            node.set_bounds(bounds.to_accesskit());
        }
        let children: Vec<accesskit::NodeId> = self
            .children
            .iter()
            .map(|child| accesskit::NodeId(child.0))
            .collect();
        node.set_children(children);
        node
    }

    /// Set the label for this node.
    pub fn with_label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Set the description for this node.
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Set the value for this node.
    pub fn with_value(mut self, value: AccessibilityValue) -> Self {
        self.value = Some(value);
        self
    }

    /// Set the screen-space bounds for this node.
    pub fn with_bounds(mut self, bounds: AccessibilityRect) -> Self {
        self.bounds = Some(bounds);
        self
    }

    /// Set the state flags for this node.
    pub fn with_states(mut self, states: AccessibilityState) -> Self {
        self.states = states;
        self
    }

    /// Set the actions for this node.
    pub fn with_actions(mut self, actions: Vec<AccessibilityAction>) -> Self {
        self.actions = actions;
        self
    }

    /// Add a child identifier to this node.
    pub fn add_child(&mut self, child_id: AccessibilityId) {
        self.children.push(child_id);
    }
}

// ---------------------------------------------------------------------------
// Tree
// ---------------------------------------------------------------------------

/// The accessibility tree for a window or application surface.
#[derive(Debug, Clone, PartialEq)]
pub struct AccessibilityTree {
    /// The identifier of the root node.
    pub root: AccessibilityId,
    /// All nodes in the tree, keyed by identifier.
    pub nodes: std::collections::HashMap<AccessibilityId, AccessibilityNode>,
}

impl AccessibilityTree {
    /// Create a new tree with the given root node.
    pub fn new(root: AccessibilityNode) -> Self {
        let root_id = root.id;
        let mut nodes = std::collections::HashMap::new();
        nodes.insert(root_id, root);
        Self {
            root: root_id,
            nodes,
        }
    }

    /// Insert a node into the tree.
    pub fn insert(&mut self, node: AccessibilityNode) {
        self.nodes.insert(node.id, node);
    }

    /// Remove a node from the tree.
    pub fn remove(&mut self, id: AccessibilityId) {
        self.nodes.remove(&id);
    }

    /// Get a reference to a node by identifier.
    pub fn get(&self, id: AccessibilityId) -> Option<&AccessibilityNode> {
        self.nodes.get(&id)
    }

    /// Get a mutable reference to a node by identifier.
    pub fn get_mut(&mut self, id: AccessibilityId) -> Option<&mut AccessibilityNode> {
        self.nodes.get_mut(&id)
    }

    /// Establish a parent-child relationship between two nodes.
    pub fn set_parent(&mut self, child: AccessibilityId, parent: AccessibilityId) {
        if let Some(node) = self.nodes.get_mut(&child) {
            node.parent = Some(parent);
        }
        if let Some(node) = self.nodes.get_mut(&parent) {
            if !node.children.contains(&child) {
                node.children.push(child);
            }
        }
    }

    /// Return the identifier of the currently focused node, if any.
    pub fn focused_node(&self) -> Option<AccessibilityId> {
        self.nodes.iter().find_map(|(id, node)| {
            if node.states.contains(AccessibilityState::FOCUSED) {
                Some(*id)
            } else {
                None
            }
        })
    }
}

// ---------------------------------------------------------------------------
// Element attributes (builder API)
// ---------------------------------------------------------------------------

/// Accessibility metadata that can be attached to GPUI elements.
///
/// This is the primary API for views and components to declare their
/// accessibility semantics.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct AccessibilityAttributes {
    /// The semantic role of the element.
    pub role: Option<AccessibilityRole>,
    /// The primary accessible label.
    pub label: Option<String>,
    /// A longer accessible description.
    pub description: Option<String>,
    /// The current value.
    pub value: Option<AccessibilityValue>,
    /// Placeholder text for inputs.
    pub placeholder: Option<String>,
    /// State flags.
    pub states: AccessibilityState,
    /// Actions available on this element.
    pub actions: Vec<AccessibilityAction>,
    /// Whether the element is hidden from assistive technology.
    pub hidden: bool,
}

impl AccessibilityAttributes {
    /// Create attributes for the given role.
    pub fn new(role: AccessibilityRole) -> Self {
        Self {
            role: Some(role),
            ..Default::default()
        }
    }

    /// Set the label.
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Set the description.
    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Set the value.
    pub fn value(mut self, value: AccessibilityValue) -> Self {
        self.value = Some(value);
        self
    }

    /// Set the placeholder text.
    pub fn placeholder(mut self, placeholder: impl Into<String>) -> Self {
        self.placeholder = Some(placeholder.into());
        self
    }

    /// Set the state flags.
    pub fn states(mut self, states: AccessibilityState) -> Self {
        self.states = states;
        self
    }

    /// Set the available actions.
    pub fn actions(mut self, actions: Vec<AccessibilityAction>) -> Self {
        self.actions = actions;
        self
    }

    /// Set whether the element is hidden.
    pub fn hidden(mut self, hidden: bool) -> Self {
        self.hidden = hidden;
        self
    }

    /// Convert these attributes into a full [`AccessibilityNode`] with the given id.
    pub fn to_node(&self, id: AccessibilityId) -> AccessibilityNode {
        AccessibilityNode {
            id,
            role: self.role.unwrap_or(AccessibilityRole::Unknown),
            states: if self.hidden {
                self.states | AccessibilityState::HIDDEN
            } else {
                self.states
            },
            label: self.label.clone(),
            description: self.description.clone(),
            value: self.value.clone(),
            placeholder: self.placeholder.clone(),
            bounds: None,
            actions: self.actions.clone(),
            children: Vec::new(),
            parent: None,
        }
    }
}

// ---------------------------------------------------------------------------
// Trait for elements/views to expose accessibility metadata
// ---------------------------------------------------------------------------

/// Implemented by types that can provide accessibility metadata.
pub trait Accessible {
    /// Return the accessibility metadata for this element.
    fn accessibility_attributes(&self) -> AccessibilityAttributes;
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_accessibility_id_unique() {
        let id1 = AccessibilityId::new();
        let id2 = AccessibilityId::new();
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_role_is_interactive() {
        assert!(AccessibilityRole::Button.is_interactive());
        assert!(AccessibilityRole::TextInput.is_interactive());
        assert!(!AccessibilityRole::StaticText.is_interactive());
        assert!(!AccessibilityRole::Group.is_interactive());
    }

    #[test]
    fn test_role_is_container() {
        assert!(AccessibilityRole::Window.is_container());
        assert!(AccessibilityRole::List.is_container());
        assert!(!AccessibilityRole::Button.is_container());
    }

    #[test]
    fn test_state_flags() {
        let mut state = AccessibilityState::NONE;
        state |= AccessibilityState::FOCUSED | AccessibilityState::CHECKED;
        assert!(state.contains(AccessibilityState::FOCUSED));
        assert!(state.contains(AccessibilityState::CHECKED));
        assert!(!state.contains(AccessibilityState::DISABLED));
    }

    #[test]
    fn test_node_builder() {
        let node = AccessibilityNode::new(AccessibilityRole::Button)
            .with_label("Submit")
            .with_states(AccessibilityState::FOCUSED)
            .with_actions(vec![AccessibilityAction::Click]);

        assert_eq!(node.role, AccessibilityRole::Button);
        assert_eq!(node.label.as_deref(), Some("Submit"));
        assert!(node.states.contains(AccessibilityState::FOCUSED));
        assert_eq!(node.actions, vec![AccessibilityAction::Click]);
    }

    #[test]
    fn test_tree_insert_and_focused() {
        let root = AccessibilityNode::new(AccessibilityRole::Window);
        let mut tree = AccessibilityTree::new(root);

        let button = AccessibilityNode::new(AccessibilityRole::Button)
            .with_states(AccessibilityState::FOCUSED);
        let button_id = button.id;
        tree.insert(button);
        tree.set_parent(button_id, tree.root);

        assert_eq!(tree.focused_node(), Some(button_id));
        assert!(
            tree.get(button_id)
                .unwrap()
                .states
                .contains(AccessibilityState::FOCUSED)
        );
    }

    #[test]
    fn test_attributes_to_node() {
        let attrs = AccessibilityAttributes::new(AccessibilityRole::CheckBox)
            .label("Enable notifications")
            .states(AccessibilityState::CHECKED)
            .hidden(true);

        let id = AccessibilityId::new();
        let node = attrs.to_node(id);

        assert_eq!(node.role, AccessibilityRole::CheckBox);
        assert_eq!(node.label.as_deref(), Some("Enable notifications"));
        assert!(node.states.contains(AccessibilityState::CHECKED));
        assert!(node.states.contains(AccessibilityState::HIDDEN));
    }

    #[test]
    fn test_value_variants() {
        let text = AccessibilityValue::Text("hello".to_string());
        let num = AccessibilityValue::Number(42.0);
        let range = AccessibilityValue::Range {
            current: 5.0,
            min: 0.0,
            max: 10.0,
            step: Some(1.0),
        };
        let toggle = AccessibilityValue::Toggle(true);

        assert_eq!(text, AccessibilityValue::Text("hello".to_string()));
        assert_ne!(text, num);
        assert!(matches!(range, AccessibilityValue::Range { .. }));
        assert!(matches!(toggle, AccessibilityValue::Toggle(true)));
    }

    #[test]
    fn test_tree_construction() {
        let root = AccessibilityNode::new(AccessibilityRole::Window);
        let mut tree = AccessibilityTree::new(root);

        let button = AccessibilityNode::new(AccessibilityRole::Button).with_label("Click me");
        let button_id = button.id;
        tree.insert(button);
        tree.set_parent(button_id, tree.root);

        assert_eq!(tree.nodes.len(), 2);
        assert_eq!(tree.get(button_id).unwrap().role, AccessibilityRole::Button);
        assert!(tree.get(tree.root).unwrap().children.contains(&button_id));
    }

    #[test]
    fn test_focus_propagation_in_tree() {
        let root = AccessibilityNode::new(AccessibilityRole::Window);
        let mut tree = AccessibilityTree::new(root);

        let input1 = AccessibilityNode::new(AccessibilityRole::TextInput)
            .with_states(AccessibilityState::FOCUSED);
        let input1_id = input1.id;
        tree.insert(input1);
        tree.set_parent(input1_id, tree.root);

        assert_eq!(tree.focused_node(), Some(input1_id));

        let input2 = AccessibilityNode::new(AccessibilityRole::TextInput);
        let input2_id = input2.id;
        tree.insert(input2);
        tree.set_parent(input2_id, tree.root);

        if let Some(node) = tree.get_mut(input1_id) {
            node.states &= !AccessibilityState::FOCUSED;
        }
        if let Some(node) = tree.get_mut(input2_id) {
            node.states |= AccessibilityState::FOCUSED;
        }

        assert_eq!(tree.focused_node(), Some(input2_id));
    }

    #[test]
    fn test_virtualized_list_items() {
        let root = AccessibilityNode::new(AccessibilityRole::Window);
        let mut tree = AccessibilityTree::new(root);

        let list = AccessibilityNode::new(AccessibilityRole::List);
        let list_id = list.id;
        tree.insert(list);
        tree.set_parent(list_id, tree.root);

        for i in 0..3 {
            let item = AccessibilityNode::new(AccessibilityRole::ListItem)
                .with_label(format!("Item {}", i));
            let item_id = item.id;
            tree.insert(item);
            tree.set_parent(item_id, list_id);
        }

        let list_node = tree.get(list_id).unwrap();
        assert_eq!(list_node.children.len(), 3);
        assert_eq!(list_node.role, AccessibilityRole::List);
    }

    #[test]
    fn test_tree_parent_child_relationships() {
        let root = AccessibilityNode::new(AccessibilityRole::Window);
        let mut tree = AccessibilityTree::new(root);

        let group = AccessibilityNode::new(AccessibilityRole::Group);
        let group_id = group.id;
        tree.insert(group);
        tree.set_parent(group_id, tree.root);

        let button = AccessibilityNode::new(AccessibilityRole::Button);
        let button_id = button.id;
        tree.insert(button);
        tree.set_parent(button_id, group_id);

        assert_eq!(tree.get(button_id).unwrap().parent, Some(group_id));
        assert_eq!(tree.get(group_id).unwrap().parent, Some(tree.root));
        assert!(tree.get(group_id).unwrap().children.contains(&button_id));
    }
}

#[cfg(test)]
mod accesskit_spike_tests {
    use super::*;

    #[test]
    fn role_maps_to_accesskit() {
        assert_eq!(
            AccessibilityRole::Button.to_accesskit(),
            accesskit::Role::Button
        );
        assert_eq!(
            AccessibilityRole::Slider.to_accesskit(),
            accesskit::Role::Slider
        );
        assert_eq!(
            AccessibilityRole::Unknown.to_accesskit(),
            accesskit::Role::Unknown
        );
    }

    #[test]
    fn node_converts_role_label_and_children() {
        let mut node = AccessibilityNode::new(AccessibilityRole::Button).with_label("OK");
        node.children = vec![AccessibilityId(7)];
        let ak = node.to_accesskit_node();
        assert_eq!(ak.role(), accesskit::Role::Button);
        assert_eq!(ak.label(), Some("OK"));
        assert_eq!(ak.children(), [accesskit::NodeId(7)]);
    }

    #[test]
    fn node_emits_geometry_when_bounds_present() {
        let node = AccessibilityNode::new(AccessibilityRole::Button)
            .with_bounds(AccessibilityRect::new(10.0, 20.0, 100.0, 40.0));
        let ak = node.to_accesskit_node();
        let rect = ak.bounds().expect("bounds should be emitted");
        assert_eq!(rect.x0, 10.0);
        assert_eq!(rect.y0, 20.0);
        assert_eq!(rect.x1, 110.0);
        assert_eq!(rect.y1, 60.0);
    }

    #[test]
    fn node_omits_geometry_when_bounds_absent() {
        let node = AccessibilityNode::new(AccessibilityRole::Button);
        assert!(node.to_accesskit_node().bounds().is_none());
    }
}
