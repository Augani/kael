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

use std::{
    collections::HashMap,
    sync::atomic::{AtomicU64, Ordering},
};

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
    /// Set a form or ranged value to a specific value.
    SetValue,
    /// Open the element's associated menu.
    ShowMenu,
    /// Dismiss the element (e.g., close a dialog).
    Dismiss,
    /// A custom action with an application-defined identifier.
    Custom(u32),
}

impl AccessibilityAction {
    /// Map to the closest AccessKit [`accesskit::Action`].
    pub fn to_accesskit(self) -> accesskit::Action {
        use accesskit::Action;
        match self {
            Self::Click | Self::Toggle => Action::Click,
            Self::Focus => Action::Focus,
            Self::ScrollUp => Action::ScrollUp,
            Self::ScrollDown => Action::ScrollDown,
            Self::Expand => Action::Expand,
            Self::Collapse => Action::Collapse,
            Self::Increment => Action::Increment,
            Self::Decrement => Action::Decrement,
            Self::SetValue => Action::SetValue,
            Self::ShowMenu => Action::Click,
            Self::Dismiss => Action::Collapse,
            Self::Custom(_) => Action::CustomAction,
        }
    }

    /// Map an AccessKit [`accesskit::Action`] back to a kael action, if one applies.
    pub fn from_accesskit(action: accesskit::Action) -> Option<Self> {
        use accesskit::Action;
        match action {
            Action::Click => Some(Self::Click),
            Action::Focus => Some(Self::Focus),
            Action::ScrollUp => Some(Self::ScrollUp),
            Action::ScrollDown => Some(Self::ScrollDown),
            Action::Expand => Some(Self::Expand),
            Action::Collapse => Some(Self::Collapse),
            Action::Increment => Some(Self::Increment),
            Action::Decrement => Some(Self::Decrement),
            Action::SetValue => Some(Self::SetValue),
            _ => None,
        }
    }
}

/// Extra data supplied with an accessibility action request.
#[derive(Debug, Clone, PartialEq)]
pub enum AccessibilityActionPayload {
    /// A string value, such as replacement text for a text input.
    Value(String),
    /// A numeric value, such as a slider position.
    NumericValue(f64),
}

/// A normalized assistive-technology action request for one accessibility node.
#[derive(Debug, Clone, PartialEq)]
pub struct AccessibilityActionRequest {
    /// The node targeted by the request.
    pub node_id: AccessibilityId,
    /// The action requested on that node.
    pub action: AccessibilityAction,
    /// Optional value data supplied by the platform action.
    pub payload: Option<AccessibilityActionPayload>,
}

impl AccessibilityActionRequest {
    /// Create a normalized action request.
    pub fn new(node_id: AccessibilityId, action: AccessibilityAction) -> Self {
        Self {
            node_id,
            action,
            payload: None,
        }
    }

    /// Create a normalized action request with extra action data.
    pub fn with_payload(
        node_id: AccessibilityId,
        action: AccessibilityAction,
        payload: AccessibilityActionPayload,
    ) -> Self {
        Self {
            node_id,
            action,
            payload: Some(payload),
        }
    }

    /// Create a request from a raw AccessKit action without node context.
    pub fn from_accesskit(node_id: AccessibilityId, action: accesskit::Action) -> Option<Self> {
        AccessibilityAction::from_accesskit(action).map(|action| Self::new(node_id, action))
    }

    /// Create a request from an AccessKit action and optional payload.
    pub fn from_accesskit_with_data(
        node_id: AccessibilityId,
        action: accesskit::Action,
        data: Option<accesskit::ActionData>,
    ) -> Option<Self> {
        let action = AccessibilityAction::from_accesskit(action)?;
        Some(match data {
            Some(accesskit::ActionData::Value(value))
                if action == AccessibilityAction::SetValue =>
            {
                Self::with_payload(
                    node_id,
                    action,
                    AccessibilityActionPayload::Value(value.into()),
                )
            }
            Some(accesskit::ActionData::NumericValue(value))
                if action == AccessibilityAction::SetValue =>
            {
                Self::with_payload(
                    node_id,
                    action,
                    AccessibilityActionPayload::NumericValue(value),
                )
            }
            _ => Self::new(node_id, action),
        })
    }

    /// Create a request from a raw AccessKit action, using the node's advertised
    /// actions to recover Kael-specific semantics when several Kael actions map
    /// to the same platform action.
    pub fn from_accesskit_for_node(
        node_id: AccessibilityId,
        node: &AccessibilityNode,
        action: accesskit::Action,
    ) -> Option<Self> {
        Self::from_accesskit_for_node_with_data(node_id, node, action, None)
    }

    /// Create a request from a raw AccessKit action and optional payload, using
    /// the node's advertised actions to recover Kael-specific semantics when
    /// several Kael actions map to the same platform action.
    pub fn from_accesskit_for_node_with_data(
        node_id: AccessibilityId,
        node: &AccessibilityNode,
        action: accesskit::Action,
        data: Option<accesskit::ActionData>,
    ) -> Option<Self> {
        let mut request = Self::from_accesskit_with_data(node_id, action, data)?;

        if request.action == AccessibilityAction::Click {
            if node.actions.contains(&AccessibilityAction::Toggle)
                && !node.actions.contains(&AccessibilityAction::Click)
            {
                request.action = AccessibilityAction::Toggle;
            } else if node.actions.contains(&AccessibilityAction::ShowMenu)
                && !node.actions.contains(&AccessibilityAction::Click)
            {
                request.action = AccessibilityAction::ShowMenu;
            }
        } else if request.action == AccessibilityAction::Collapse
            && node.actions.contains(&AccessibilityAction::Dismiss)
            && !node.actions.contains(&AccessibilityAction::Collapse)
        {
            request.action = AccessibilityAction::Dismiss;
        }

        if node.actions.contains(&request.action) {
            Some(request)
        } else {
            None
        }
    }
}

/// Routes normalized accessibility action requests to application handlers.
///
/// Platform adapters can feed this router after converting native action
/// callbacks into [`AccessibilityActionRequest`]. Applications can also use it
/// directly in tests or custom accessibility integrations.
#[derive(Default)]
pub struct AccessibilityActionRouter {
    handlers: HashMap<(AccessibilityId, AccessibilityAction), AccessibilityActionHandler>,
}

type AccessibilityActionHandler = Box<dyn FnMut(AccessibilityActionRequest) + 'static>;

impl AccessibilityActionRouter {
    /// Create an empty router.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a handler for one node/action pair.
    pub fn on_action(
        &mut self,
        node_id: AccessibilityId,
        action: AccessibilityAction,
        handler: impl FnMut(AccessibilityActionRequest) + 'static,
    ) {
        self.handlers.insert((node_id, action), Box::new(handler));
    }

    /// Remove a handler for one node/action pair.
    pub fn remove_action(
        &mut self,
        node_id: AccessibilityId,
        action: AccessibilityAction,
    ) -> Option<AccessibilityActionHandler> {
        self.handlers.remove(&(node_id, action))
    }

    /// Return whether a handler is registered for one node/action pair.
    pub fn has_handler(&self, node_id: AccessibilityId, action: AccessibilityAction) -> bool {
        self.handlers.contains_key(&(node_id, action))
    }

    /// Dispatch a normalized action request. Returns true when a handler ran.
    pub fn dispatch(&mut self, request: AccessibilityActionRequest) -> bool {
        let Some(handler) = self.handlers.get_mut(&(request.node_id, request.action)) else {
            return false;
        };
        handler(request);
        true
    }

    /// Dispatch a raw AccessKit action against a node. Returns true when the
    /// action was supported by the node and a matching handler ran.
    pub fn dispatch_accesskit(
        &mut self,
        node_id: AccessibilityId,
        node: &AccessibilityNode,
        action: accesskit::Action,
    ) -> bool {
        let Some(request) =
            AccessibilityActionRequest::from_accesskit_for_node(node_id, node, action)
        else {
            return false;
        };
        self.dispatch(request)
    }
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

    /// Build a rect from an element's laid-out bounds in window coordinates.
    pub fn from_bounds(bounds: crate::Bounds<crate::Pixels>) -> Self {
        Self {
            x: bounds.origin.x.0 as f64,
            y: bounds.origin.y.0 as f64,
            width: bounds.size.width.0 as f64,
            height: bounds.size.height.0 as f64,
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
        if let Some(placeholder) = &self.placeholder {
            node.set_placeholder(placeholder.as_str());
        }
        if let Some(bounds) = &self.bounds {
            node.set_bounds(bounds.to_accesskit());
        }
        apply_value(&mut node, self);
        apply_states(&mut node, self.states);
        for action in &self.actions {
            node.add_action(action.to_accesskit());
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

    /// Audit this tree for common accessibility issues before platform export.
    pub fn audit_report(&self) -> AccessibilityAuditReport {
        let mut issues = Vec::new();

        if !self.nodes.contains_key(&self.root) {
            issues.push(accessibility_audit_issue(
                AccessibilityAuditSeverity::Error,
                AccessibilityAuditIssueKind::MissingRoot,
                None,
                None,
                "accessibility tree root is missing from the node map",
            ));
            return AccessibilityAuditReport { issues };
        }

        let mut focused_nodes = Vec::new();
        for (id, node) in &self.nodes {
            audit_accessibility_node(*id, node, &mut issues);
            if node.states.contains(AccessibilityState::FOCUSED) {
                focused_nodes.push(*id);
            }
            for child in &node.children {
                match self.nodes.get(child) {
                    Some(child_node) => {
                        if child_node.parent != Some(*id) {
                            issues.push(accessibility_audit_issue(
                                AccessibilityAuditSeverity::Warning,
                                AccessibilityAuditIssueKind::ParentMismatch,
                                Some(*child),
                                Some(child_node.role),
                                "accessibility child node parent link does not match its container",
                            ));
                        }
                    }
                    None => issues.push(accessibility_audit_issue(
                        AccessibilityAuditSeverity::Error,
                        AccessibilityAuditIssueKind::MissingChildNode,
                        Some(*child),
                        None,
                        "accessibility node references a child that is missing from the tree",
                    )),
                }
            }
        }

        if focused_nodes.len() > 1 {
            issues.push(accessibility_audit_issue(
                AccessibilityAuditSeverity::Error,
                AccessibilityAuditIssueKind::MultipleFocusedNodes,
                focused_nodes.first().copied(),
                self.nodes.get(&focused_nodes[0]).map(|node| node.role),
                "accessibility tree has more than one focused node",
            ));
        }

        AccessibilityAuditReport { issues }
    }

    /// Build a full AccessKit [`accesskit::TreeUpdate`] from this tree.
    ///
    /// Only nodes reachable from the root are emitted, and every parent's child
    /// list is filtered to nodes that actually exist, so the resulting update is
    /// always internally consistent (AccessKit panics on a malformed tree).
    /// Hidden nodes are pruned along with their subtrees.
    pub fn to_accesskit_tree_update(
        &self,
        app_name: Option<&str>,
        toolkit_name: Option<&str>,
        toolkit_version: Option<&str>,
    ) -> accesskit::TreeUpdate {
        let root_id = accesskit::NodeId(self.root.0);
        let mut nodes: Vec<(accesskit::NodeId, accesskit::Node)> = Vec::new();
        let mut visited = std::collections::HashSet::new();
        let mut queue = std::collections::VecDeque::new();
        queue.push_back(self.root);

        while let Some(id) = queue.pop_front() {
            if !visited.insert(id) {
                continue;
            }
            let Some(node) = self.nodes.get(&id) else {
                continue;
            };
            if id != self.root && node.states.contains(AccessibilityState::HIDDEN) {
                continue;
            }

            let mut ak_node = accesskit::Node::new(node.role.to_accesskit());
            if let Some(label) = &node.label {
                ak_node.set_label(label.as_str());
            }
            if let Some(description) = &node.description {
                ak_node.set_description(description.as_str());
            }
            if let Some(placeholder) = &node.placeholder {
                ak_node.set_placeholder(placeholder.as_str());
            }
            if let Some(bounds) = &node.bounds {
                ak_node.set_bounds(bounds.to_accesskit());
            }
            apply_value(&mut ak_node, node);
            apply_states(&mut ak_node, node.states);
            for action in &node.actions {
                ak_node.add_action(action.to_accesskit());
            }

            let mut child_ids = Vec::new();
            for child in &node.children {
                if let Some(child_node) = self.nodes.get(child) {
                    if child_node.states.contains(AccessibilityState::HIDDEN) {
                        continue;
                    }
                    child_ids.push(accesskit::NodeId(child.0));
                    queue.push_back(*child);
                }
            }
            ak_node.set_children(child_ids);

            nodes.push((accesskit::NodeId(id.0), ak_node));
        }

        let focus = self
            .focused_node()
            .filter(|id| visited.contains(id))
            .map(|id| accesskit::NodeId(id.0))
            .unwrap_or(root_id);

        let mut tree = accesskit::Tree::new(root_id);
        tree.app_name = app_name.map(str::to_owned);
        tree.toolkit_name = toolkit_name.map(str::to_owned);
        tree.toolkit_version = toolkit_version.map(str::to_owned);

        accesskit::TreeUpdate {
            nodes,
            tree: Some(tree),
            focus,
        }
    }
}

/// Severity of an accessibility audit issue.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AccessibilityAuditSeverity {
    /// The tree or attributes are likely invalid or unusable.
    Error,
    /// The tree or attributes are valid but likely lower quality.
    Warning,
}

/// Common accessibility issue categories.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AccessibilityAuditIssueKind {
    /// The tree root id is not present in the node map.
    MissingRoot,
    /// Attribute metadata omitted a role.
    MissingRole,
    /// A node uses `Unknown` for a role.
    UnknownRole,
    /// An interactive node has no accessible name.
    MissingAccessibleName,
    /// An interactive node advertises no actions.
    MissingInteractiveAction,
    /// A range value has invalid or non-finite bounds.
    InvalidRange,
    /// Mutually exclusive state flags are present together.
    ConflictingStates,
    /// A focused node is hidden from assistive technology.
    HiddenFocusedNode,
    /// A child id is missing from the tree.
    MissingChildNode,
    /// A child node's parent pointer disagrees with its container.
    ParentMismatch,
    /// More than one node is marked focused.
    MultipleFocusedNodes,
}

/// One accessibility audit finding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccessibilityAuditIssue {
    severity: AccessibilityAuditSeverity,
    kind: AccessibilityAuditIssueKind,
    node_id: Option<AccessibilityId>,
    role: Option<AccessibilityRole>,
    message: String,
}

impl AccessibilityAuditIssue {
    /// Issue severity.
    pub fn severity(&self) -> AccessibilityAuditSeverity {
        self.severity
    }

    /// Issue category.
    pub fn kind(&self) -> AccessibilityAuditIssueKind {
        self.kind
    }

    /// Related node id, when available.
    pub fn node_id(&self) -> Option<AccessibilityId> {
        self.node_id
    }

    /// Related node role, when available.
    pub fn role(&self) -> Option<AccessibilityRole> {
        self.role
    }

    /// Human-readable issue message.
    pub fn message(&self) -> &str {
        &self.message
    }
}

/// Non-throwing accessibility audit report for app and agent checks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccessibilityAuditReport {
    issues: Vec<AccessibilityAuditIssue>,
}

impl AccessibilityAuditReport {
    /// All audit issues.
    pub fn issues(&self) -> &[AccessibilityAuditIssue] {
        &self.issues
    }

    /// Blocking audit errors.
    pub fn errors(&self) -> Vec<&AccessibilityAuditIssue> {
        self.issues
            .iter()
            .filter(|issue| issue.severity == AccessibilityAuditSeverity::Error)
            .collect()
    }

    /// Non-blocking audit warnings.
    pub fn warnings(&self) -> Vec<&AccessibilityAuditIssue> {
        self.issues
            .iter()
            .filter(|issue| issue.severity == AccessibilityAuditSeverity::Warning)
            .collect()
    }

    /// Whether no blocking errors were found.
    pub fn is_ready(&self) -> bool {
        self.errors().is_empty()
    }

    /// Compact summary for logs, diagnostics, and agent output.
    pub fn summary(&self) -> String {
        let errors = self.errors().len();
        let warnings = self.warnings().len();
        match (errors, warnings) {
            (0, 0) => "accessibility audit passed".to_string(),
            (0, warnings) => format!("accessibility audit passed with {warnings} warning(s)"),
            (errors, warnings) => {
                format!("accessibility audit found {errors} error(s), {warnings} warning(s)")
            }
        }
    }
}

fn audit_accessibility_node(
    node_id: AccessibilityId,
    node: &AccessibilityNode,
    issues: &mut Vec<AccessibilityAuditIssue>,
) {
    if node.role == AccessibilityRole::Unknown {
        issues.push(accessibility_audit_issue(
            AccessibilityAuditSeverity::Warning,
            AccessibilityAuditIssueKind::UnknownRole,
            Some(node_id),
            Some(node.role),
            "accessibility node uses an unknown role",
        ));
    }

    if node.role.is_interactive() {
        let has_name = non_empty_accessibility_text(node.label.as_deref())
            || non_empty_accessibility_text(node.description.as_deref())
            || non_empty_accessibility_text(node.placeholder.as_deref());
        if !has_name {
            issues.push(accessibility_audit_issue(
                AccessibilityAuditSeverity::Error,
                AccessibilityAuditIssueKind::MissingAccessibleName,
                Some(node_id),
                Some(node.role),
                "interactive accessibility node needs a label, description, or placeholder",
            ));
        }
        if node.actions.is_empty() {
            issues.push(accessibility_audit_issue(
                AccessibilityAuditSeverity::Error,
                AccessibilityAuditIssueKind::MissingInteractiveAction,
                Some(node_id),
                Some(node.role),
                "interactive accessibility node needs at least one action",
            ));
        }
    }

    if let Some(AccessibilityValue::Range {
        current,
        min,
        max,
        step,
    }) = node.value.as_ref()
    {
        let valid = current.is_finite()
            && min.is_finite()
            && max.is_finite()
            && min <= max
            && current >= min
            && current <= max
            && step.is_none_or(|step| step.is_finite() && step > 0.0);
        if !valid {
            issues.push(accessibility_audit_issue(
                AccessibilityAuditSeverity::Error,
                AccessibilityAuditIssueKind::InvalidRange,
                Some(node_id),
                Some(node.role),
                "accessibility range value must be finite and within min/max bounds",
            ));
        }
    }

    if node.states.contains(AccessibilityState::EXPANDED)
        && node.states.contains(AccessibilityState::COLLAPSED)
    {
        issues.push(accessibility_audit_issue(
            AccessibilityAuditSeverity::Error,
            AccessibilityAuditIssueKind::ConflictingStates,
            Some(node_id),
            Some(node.role),
            "accessibility node cannot be both expanded and collapsed",
        ));
    }

    if node.states.contains(AccessibilityState::CHECKED)
        && node.states.contains(AccessibilityState::INDETERMINATE)
    {
        issues.push(accessibility_audit_issue(
            AccessibilityAuditSeverity::Error,
            AccessibilityAuditIssueKind::ConflictingStates,
            Some(node_id),
            Some(node.role),
            "accessibility node cannot be both checked and indeterminate",
        ));
    }

    if node.states.contains(AccessibilityState::FOCUSED)
        && node.states.contains(AccessibilityState::HIDDEN)
    {
        issues.push(accessibility_audit_issue(
            AccessibilityAuditSeverity::Error,
            AccessibilityAuditIssueKind::HiddenFocusedNode,
            Some(node_id),
            Some(node.role),
            "hidden accessibility node cannot also be focused",
        ));
    }
}

fn accessibility_audit_issue(
    severity: AccessibilityAuditSeverity,
    kind: AccessibilityAuditIssueKind,
    node_id: Option<AccessibilityId>,
    role: Option<AccessibilityRole>,
    message: impl Into<String>,
) -> AccessibilityAuditIssue {
    AccessibilityAuditIssue {
        severity,
        kind,
        node_id,
        role,
        message: message.into(),
    }
}

fn non_empty_accessibility_text(value: Option<&str>) -> bool {
    value.is_some_and(|value| !value.trim().is_empty())
}

fn apply_value(node: &mut accesskit::Node, source: &AccessibilityNode) {
    match &source.value {
        Some(AccessibilityValue::Text(text)) => node.set_value(text.as_str()),
        Some(AccessibilityValue::Number(number)) => node.set_numeric_value(*number),
        Some(AccessibilityValue::Range {
            current,
            min,
            max,
            step,
        }) => {
            node.set_numeric_value(*current);
            node.set_min_numeric_value(*min);
            node.set_max_numeric_value(*max);
            if let Some(step) = step {
                node.set_numeric_value_step(*step);
            }
        }
        Some(AccessibilityValue::Toggle(on)) => {
            node.set_toggled(if *on {
                accesskit::Toggled::True
            } else {
                accesskit::Toggled::False
            });
        }
        None => {}
    }
}

fn apply_states(node: &mut accesskit::Node, states: AccessibilityState) {
    if states.contains(AccessibilityState::DISABLED) {
        node.set_disabled();
    }
    if states.contains(AccessibilityState::SELECTED) {
        node.set_selected(true);
    }
    if states.contains(AccessibilityState::EXPANDED) {
        node.set_expanded(true);
    } else if states.contains(AccessibilityState::COLLAPSED) {
        node.set_expanded(false);
    }
    if states.contains(AccessibilityState::INDETERMINATE) {
        node.set_toggled(accesskit::Toggled::Mixed);
    } else if states.contains(AccessibilityState::CHECKED) {
        node.set_toggled(accesskit::Toggled::True);
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

    /// Create a clickable button with a label and standard focus/click actions.
    pub fn button(label: impl Into<String>) -> Self {
        Self::new(AccessibilityRole::Button)
            .label(label)
            .actions(vec![AccessibilityAction::Focus, AccessibilityAction::Click])
    }

    /// Create a hyperlink with a label and standard focus/click actions.
    pub fn link(label: impl Into<String>) -> Self {
        Self::new(AccessibilityRole::Link)
            .label(label)
            .actions(vec![AccessibilityAction::Focus, AccessibilityAction::Click])
    }

    /// Create a checkbox with a label and checked state.
    pub fn checkbox(label: impl Into<String>, checked: bool) -> Self {
        Self::new(AccessibilityRole::CheckBox)
            .label(label)
            .toggle_state(checked)
            .value(AccessibilityValue::Toggle(checked))
            .actions(vec![
                AccessibilityAction::Focus,
                AccessibilityAction::Toggle,
                AccessibilityAction::Click,
            ])
    }

    /// Create an on/off switch with a label and current state.
    pub fn switch(label: impl Into<String>, on: bool) -> Self {
        Self::new(AccessibilityRole::Switch)
            .label(label)
            .toggle_state(on)
            .value(AccessibilityValue::Toggle(on))
            .actions(vec![
                AccessibilityAction::Focus,
                AccessibilityAction::Toggle,
                AccessibilityAction::Click,
            ])
    }

    /// Create a radio button with a label and selection state.
    pub fn radio_button(label: impl Into<String>, selected: bool) -> Self {
        Self::new(AccessibilityRole::RadioButton)
            .label(label)
            .selected(selected)
            .actions(vec![AccessibilityAction::Focus, AccessibilityAction::Click])
    }

    /// Create a slider with a labelled range value.
    pub fn slider(
        label: impl Into<String>,
        current: f64,
        min: f64,
        max: f64,
        step: Option<f64>,
    ) -> Self {
        Self::new(AccessibilityRole::Slider)
            .label(label)
            .value(AccessibilityValue::Range {
                current,
                min,
                max,
                step,
            })
            .actions(vec![
                AccessibilityAction::Focus,
                AccessibilityAction::Increment,
                AccessibilityAction::Decrement,
                AccessibilityAction::SetValue,
            ])
    }

    /// Create a progress bar with a labelled range value.
    pub fn progress_bar(label: impl Into<String>, current: f64, min: f64, max: f64) -> Self {
        Self::new(AccessibilityRole::ProgressBar)
            .label(label)
            .value(AccessibilityValue::Range {
                current,
                min,
                max,
                step: None,
            })
    }

    /// Create a text input with a label and current text value.
    pub fn text_input(label: impl Into<String>, value: impl Into<String>) -> Self {
        Self::new(AccessibilityRole::TextInput)
            .label(label)
            .value(AccessibilityValue::Text(value.into()))
            .actions(vec![
                AccessibilityAction::Focus,
                AccessibilityAction::SetValue,
            ])
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

    /// Add or remove one state flag.
    pub fn state(mut self, state: AccessibilityState, enabled: bool) -> Self {
        if enabled {
            self.states |= state;
        } else {
            self.states.remove(state);
        }
        self
    }

    /// Mark the element focused.
    pub fn focused(self, focused: bool) -> Self {
        self.state(AccessibilityState::FOCUSED, focused)
    }

    /// Mark the element disabled.
    pub fn disabled(self, disabled: bool) -> Self {
        self.state(AccessibilityState::DISABLED, disabled)
    }

    /// Mark the element selected.
    pub fn selected(self, selected: bool) -> Self {
        self.state(AccessibilityState::SELECTED, selected)
    }

    /// Mark the element expanded or collapsed.
    pub fn expanded(mut self, expanded: bool) -> Self {
        self.states
            .remove(AccessibilityState::EXPANDED | AccessibilityState::COLLAPSED);
        if expanded {
            self.states |= AccessibilityState::EXPANDED;
        } else {
            self.states |= AccessibilityState::COLLAPSED;
        }
        self
    }

    /// Mark a toggle-like element checked or unchecked.
    pub fn toggle_state(self, checked: bool) -> Self {
        self.state(AccessibilityState::CHECKED, checked)
    }

    /// Mark the element as indeterminate.
    pub fn indeterminate(self, indeterminate: bool) -> Self {
        self.state(AccessibilityState::INDETERMINATE, indeterminate)
    }

    /// Mark the element pressed.
    pub fn pressed(self, pressed: bool) -> Self {
        self.state(AccessibilityState::PRESSED, pressed)
    }

    /// Mark the element required.
    pub fn required(self, required: bool) -> Self {
        self.state(AccessibilityState::REQUIRED, required)
    }

    /// Mark the element invalid.
    pub fn invalid(self, invalid: bool) -> Self {
        self.state(AccessibilityState::INVALID, invalid)
    }

    /// Mark the element busy.
    pub fn busy(self, busy: bool) -> Self {
        self.state(AccessibilityState::BUSY, busy)
    }

    /// Set the available actions.
    pub fn actions(mut self, actions: Vec<AccessibilityAction>) -> Self {
        self.actions = actions;
        self
    }

    /// Add one available action if it is not already present.
    pub fn action(mut self, action: AccessibilityAction) -> Self {
        if !self.actions.contains(&action) {
            self.actions.push(action);
        }
        self
    }

    /// Set whether the element is hidden.
    pub fn hidden(mut self, hidden: bool) -> Self {
        self.hidden = hidden;
        self
    }

    /// Validate common accessibility requirements for custom elements.
    pub fn validate(&self) -> anyhow::Result<()> {
        let Some(role) = self.role else {
            anyhow::bail!("accessibility role is required");
        };

        if role.is_interactive() {
            let has_name = self
                .label
                .as_ref()
                .is_some_and(|label| !label.trim().is_empty())
                || self
                    .description
                    .as_ref()
                    .is_some_and(|description| !description.trim().is_empty())
                || self
                    .placeholder
                    .as_ref()
                    .is_some_and(|placeholder| !placeholder.trim().is_empty());
            anyhow::ensure!(
                has_name,
                "interactive accessibility role {role:?} needs a label, description, or placeholder"
            );
            anyhow::ensure!(
                !self.actions.is_empty(),
                "interactive accessibility role {role:?} needs at least one action"
            );
        }

        if let Some(AccessibilityValue::Range {
            current,
            min,
            max,
            step,
        }) = self.value.as_ref()
        {
            anyhow::ensure!(
                current.is_finite() && min.is_finite() && max.is_finite(),
                "accessibility range values must be finite"
            );
            anyhow::ensure!(min <= max, "accessibility range min cannot exceed max");
            anyhow::ensure!(
                current >= min && current <= max,
                "accessibility range current value must be within min and max"
            );
            if let Some(step) = *step {
                anyhow::ensure!(
                    step.is_finite() && step > 0.0,
                    "accessibility range step must be finite and positive"
                );
            }
        }

        Ok(())
    }

    /// Return a non-throwing audit report for these attributes.
    pub fn audit_report(&self) -> AccessibilityAuditReport {
        if self.role.is_none() {
            return AccessibilityAuditReport {
                issues: vec![accessibility_audit_issue(
                    AccessibilityAuditSeverity::Error,
                    AccessibilityAuditIssueKind::MissingRole,
                    None,
                    None,
                    "accessibility role is required",
                )],
            };
        };
        let node = self.to_node(AccessibilityId::new());
        let mut issues = Vec::new();
        audit_accessibility_node(node.id, &node, &mut issues);
        AccessibilityAuditReport { issues }
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
    fn test_semantic_accessibility_recipes_set_required_fields() {
        let button = AccessibilityAttributes::button("Save");
        assert_eq!(button.role, Some(AccessibilityRole::Button));
        assert_eq!(button.label.as_deref(), Some("Save"));
        assert_eq!(
            button.actions,
            vec![AccessibilityAction::Focus, AccessibilityAction::Click]
        );
        assert!(button.validate().is_ok());

        let switch = AccessibilityAttributes::switch("Enable sync", true);
        assert_eq!(switch.role, Some(AccessibilityRole::Switch));
        assert_eq!(switch.value, Some(AccessibilityValue::Toggle(true)));
        assert!(switch.states.contains(AccessibilityState::CHECKED));
        assert!(switch.actions.contains(&AccessibilityAction::Toggle));
        assert!(switch.validate().is_ok());

        let slider = AccessibilityAttributes::slider("Volume", 50.0, 0.0, 100.0, Some(5.0));
        assert_eq!(slider.role, Some(AccessibilityRole::Slider));
        assert!(slider.actions.contains(&AccessibilityAction::Increment));
        assert!(slider.actions.contains(&AccessibilityAction::Decrement));
        assert!(slider.validate().is_ok());
    }

    #[test]
    fn test_accessibility_state_helpers_are_chainable() {
        let attrs = AccessibilityAttributes::button("Upload")
            .disabled(true)
            .pressed(true)
            .busy(true)
            .disabled(false);

        assert!(!attrs.states.contains(AccessibilityState::DISABLED));
        assert!(attrs.states.contains(AccessibilityState::PRESSED));
        assert!(attrs.states.contains(AccessibilityState::BUSY));
    }

    #[test]
    fn test_accessibility_validation_catches_common_custom_control_errors() {
        assert!(
            AccessibilityAttributes::new(AccessibilityRole::Button)
                .action(AccessibilityAction::Click)
                .validate()
                .is_err()
        );
        assert!(
            AccessibilityAttributes::new(AccessibilityRole::Button)
                .label("Do thing")
                .validate()
                .is_err()
        );
        assert!(
            AccessibilityAttributes::slider("Volume", 120.0, 0.0, 100.0, Some(1.0))
                .validate()
                .is_err()
        );
        assert!(
            AccessibilityAttributes::slider("Volume", 50.0, 0.0, 100.0, Some(0.0))
                .validate()
                .is_err()
        );
        assert!(
            AccessibilityAttributes::slider("Volume", f64::NAN, 0.0, 100.0, Some(1.0))
                .validate()
                .is_err()
        );
    }

    #[test]
    fn test_accessibility_attributes_audit_reports_all_common_errors() {
        let attrs = AccessibilityAttributes::new(AccessibilityRole::Button)
            .states(AccessibilityState::EXPANDED | AccessibilityState::COLLAPSED)
            .value(AccessibilityValue::Range {
                current: f64::NAN,
                min: 0.0,
                max: 100.0,
                step: Some(1.0),
            });

        let report = attrs.audit_report();
        assert!(!report.is_ready());
        assert!(report.summary().contains("error"));
        assert!(report.issues().iter().any(|issue| {
            issue.kind() == AccessibilityAuditIssueKind::MissingAccessibleName
                && issue.severity() == AccessibilityAuditSeverity::Error
        }));
        assert!(report.issues().iter().any(|issue| {
            issue.kind() == AccessibilityAuditIssueKind::MissingInteractiveAction
        }));
        assert!(
            report
                .issues()
                .iter()
                .any(|issue| { issue.kind() == AccessibilityAuditIssueKind::InvalidRange })
        );
        assert!(
            report
                .issues()
                .iter()
                .any(|issue| { issue.kind() == AccessibilityAuditIssueKind::ConflictingStates })
        );

        let missing_role = AccessibilityAttributes::default().audit_report();
        assert_eq!(
            missing_role.issues()[0].kind(),
            AccessibilityAuditIssueKind::MissingRole
        );
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
    fn test_accessibility_tree_audit_reports_structural_issues() {
        let root = AccessibilityNode::new(AccessibilityRole::Window);
        let mut tree = AccessibilityTree::new(root);

        let focused_hidden = AccessibilityNode::new(AccessibilityRole::Button)
            .with_label("Hidden")
            .with_actions(vec![AccessibilityAction::Click])
            .with_states(AccessibilityState::FOCUSED | AccessibilityState::HIDDEN);
        let focused_hidden_id = focused_hidden.id;
        tree.insert(focused_hidden);
        tree.set_parent(focused_hidden_id, tree.root);

        let second_focused = AccessibilityNode::new(AccessibilityRole::TextInput)
            .with_label("Search")
            .with_actions(vec![AccessibilityAction::Focus])
            .with_states(AccessibilityState::FOCUSED);
        let second_focused_id = second_focused.id;
        tree.insert(second_focused);
        tree.set_parent(second_focused_id, tree.root);

        let orphan = AccessibilityNode::new(AccessibilityRole::Button)
            .with_label("Orphan")
            .with_actions(vec![AccessibilityAction::Click]);
        let orphan_id = orphan.id;
        tree.insert(orphan);
        tree.get_mut(tree.root).unwrap().add_child(orphan_id);

        let missing_id = AccessibilityId::new();
        tree.get_mut(tree.root).unwrap().add_child(missing_id);

        let report = tree.audit_report();
        assert!(!report.is_ready());
        assert!(report.issues().iter().any(|issue| {
            issue.kind() == AccessibilityAuditIssueKind::HiddenFocusedNode
                && issue.node_id() == Some(focused_hidden_id)
        }));
        assert!(
            report
                .issues()
                .iter()
                .any(|issue| { issue.kind() == AccessibilityAuditIssueKind::MultipleFocusedNodes })
        );
        assert!(report.issues().iter().any(|issue| {
            issue.kind() == AccessibilityAuditIssueKind::ParentMismatch
                && issue.node_id() == Some(orphan_id)
        }));
        assert!(report.issues().iter().any(|issue| {
            issue.kind() == AccessibilityAuditIssueKind::MissingChildNode
                && issue.node_id() == Some(missing_id)
        }));
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

    #[test]
    fn node_maps_value_and_states() {
        let node = AccessibilityNode::new(AccessibilityRole::Slider)
            .with_value(AccessibilityValue::Range {
                current: 3.0,
                min: 0.0,
                max: 10.0,
                step: Some(1.0),
            })
            .with_states(AccessibilityState::DISABLED | AccessibilityState::SELECTED)
            .with_actions(vec![
                AccessibilityAction::Increment,
                AccessibilityAction::Focus,
            ]);
        let ak = node.to_accesskit_node();
        assert_eq!(ak.numeric_value(), Some(3.0));
        assert_eq!(ak.min_numeric_value(), Some(0.0));
        assert_eq!(ak.max_numeric_value(), Some(10.0));
        assert!(ak.is_disabled());
        assert!(ak.is_selected().unwrap_or(false));
        assert!(ak.supports_action(accesskit::Action::Increment));
        assert!(ak.supports_action(accesskit::Action::Focus));
    }

    #[test]
    fn tree_update_is_consistent_and_focus_resolves() {
        let root = AccessibilityNode::new(AccessibilityRole::Window);
        let root_id = root.id;
        let mut tree = AccessibilityTree::new(root);

        let button = AccessibilityNode::new(AccessibilityRole::Button)
            .with_label("Save")
            .with_states(AccessibilityState::FOCUSED);
        let button_id = button.id;
        tree.insert(button);
        tree.set_parent(button_id, root_id);

        let update = tree.to_accesskit_tree_update(Some("App"), Some("Kael"), Some("0.0.0"));
        let ak_tree = update.tree.expect("tree present");
        assert_eq!(ak_tree.root, accesskit::NodeId(root_id.0));
        assert_eq!(ak_tree.app_name.as_deref(), Some("App"));
        assert_eq!(update.focus, accesskit::NodeId(button_id.0));

        let root_node = update
            .nodes
            .iter()
            .find(|(id, _)| *id == accesskit::NodeId(root_id.0))
            .map(|(_, node)| node)
            .expect("root emitted");
        assert_eq!(root_node.children(), [accesskit::NodeId(button_id.0)]);
        assert!(
            update
                .nodes
                .iter()
                .any(|(id, _)| *id == accesskit::NodeId(button_id.0))
        );
    }

    #[test]
    fn tree_update_prunes_hidden_subtrees() {
        let root = AccessibilityNode::new(AccessibilityRole::Window);
        let root_id = root.id;
        let mut tree = AccessibilityTree::new(root);

        let hidden = AccessibilityNode::new(AccessibilityRole::Group)
            .with_states(AccessibilityState::HIDDEN);
        let hidden_id = hidden.id;
        tree.insert(hidden);
        tree.set_parent(hidden_id, root_id);

        let child = AccessibilityNode::new(AccessibilityRole::Button).with_label("Buried");
        let child_id = child.id;
        tree.insert(child);
        tree.set_parent(child_id, hidden_id);

        let update = tree.to_accesskit_tree_update(None, None, None);
        assert!(
            !update
                .nodes
                .iter()
                .any(|(id, _)| *id == accesskit::NodeId(hidden_id.0))
        );
        assert!(
            !update
                .nodes
                .iter()
                .any(|(id, _)| *id == accesskit::NodeId(child_id.0))
        );
        let root_node = update
            .nodes
            .iter()
            .find(|(id, _)| *id == accesskit::NodeId(root_id.0))
            .map(|(_, node)| node)
            .expect("root emitted");
        assert!(root_node.children().is_empty());
    }

    #[test]
    fn action_round_trips_through_accesskit() {
        assert_eq!(
            AccessibilityAction::from_accesskit(AccessibilityAction::Click.to_accesskit()),
            Some(AccessibilityAction::Click)
        );
        assert_eq!(
            AccessibilityAction::from_accesskit(AccessibilityAction::Increment.to_accesskit()),
            Some(AccessibilityAction::Increment)
        );
    }

    #[test]
    fn action_request_uses_node_actions_to_recover_semantics() {
        let node_id = AccessibilityId::new();
        let toggle_only = AccessibilityNode::new(AccessibilityRole::Switch).with_actions(vec![
            AccessibilityAction::Focus,
            AccessibilityAction::Toggle,
        ]);
        let menu_only = AccessibilityNode::new(AccessibilityRole::ComboBox).with_actions(vec![
            AccessibilityAction::Focus,
            AccessibilityAction::ShowMenu,
        ]);
        let dismiss_only = AccessibilityNode::new(AccessibilityRole::Dialog)
            .with_actions(vec![AccessibilityAction::Dismiss]);

        assert_eq!(
            AccessibilityActionRequest::from_accesskit_for_node(
                node_id,
                &toggle_only,
                accesskit::Action::Click,
            ),
            Some(AccessibilityActionRequest::new(
                node_id,
                AccessibilityAction::Toggle,
            ))
        );
        assert_eq!(
            AccessibilityActionRequest::from_accesskit_for_node(
                node_id,
                &menu_only,
                accesskit::Action::Click,
            ),
            Some(AccessibilityActionRequest::new(
                node_id,
                AccessibilityAction::ShowMenu,
            ))
        );
        assert_eq!(
            AccessibilityActionRequest::from_accesskit_for_node(
                node_id,
                &dismiss_only,
                accesskit::Action::Collapse,
            ),
            Some(AccessibilityActionRequest::new(
                node_id,
                AccessibilityAction::Dismiss,
            ))
        );
        assert_eq!(
            AccessibilityActionRequest::from_accesskit_for_node(
                node_id,
                &toggle_only,
                accesskit::Action::Increment,
            ),
            None
        );
    }

    #[test]
    fn action_request_preserves_set_value_payloads() {
        let node_id = AccessibilityId::new();
        let slider = AccessibilityNode::new(AccessibilityRole::Slider).with_actions(vec![
            AccessibilityAction::Focus,
            AccessibilityAction::Increment,
            AccessibilityAction::Decrement,
            AccessibilityAction::SetValue,
        ]);

        assert_eq!(
            AccessibilityActionRequest::from_accesskit_for_node_with_data(
                node_id,
                &slider,
                accesskit::Action::SetValue,
                Some(accesskit::ActionData::NumericValue(42.0)),
            ),
            Some(AccessibilityActionRequest::with_payload(
                node_id,
                AccessibilityAction::SetValue,
                AccessibilityActionPayload::NumericValue(42.0),
            ))
        );

        assert_eq!(
            AccessibilityActionRequest::from_accesskit_for_node_with_data(
                node_id,
                &slider,
                accesskit::Action::SetValue,
                Some(accesskit::ActionData::Value("medium".into())),
            ),
            Some(AccessibilityActionRequest::with_payload(
                node_id,
                AccessibilityAction::SetValue,
                AccessibilityActionPayload::Value("medium".into()),
            ))
        );
    }

    #[test]
    fn text_input_recipe_supports_set_value() {
        let attrs = AccessibilityAttributes::text_input("Search", "kael");
        let node = attrs.to_node(AccessibilityId::new());

        assert_eq!(node.value, Some(AccessibilityValue::Text("kael".into())));
        assert!(node.actions.contains(&AccessibilityAction::Focus));
        assert!(node.actions.contains(&AccessibilityAction::SetValue));
        attrs.validate().unwrap();
    }

    #[test]
    fn action_router_dispatches_registered_handlers() {
        use std::{cell::RefCell, rc::Rc};

        let node_id = AccessibilityId::new();
        let node = AccessibilityNode::new(AccessibilityRole::Button)
            .with_actions(vec![AccessibilityAction::Focus, AccessibilityAction::Click]);
        let seen = Rc::new(RefCell::new(Vec::new()));
        let seen_handler = seen.clone();
        let mut router = AccessibilityActionRouter::new();

        router.on_action(node_id, AccessibilityAction::Click, move |request| {
            seen_handler.borrow_mut().push(request);
        });

        assert!(router.has_handler(node_id, AccessibilityAction::Click));
        assert!(router.dispatch_accesskit(node_id, &node, accesskit::Action::Click));
        assert_eq!(
            seen.borrow().as_slice(),
            &[AccessibilityActionRequest::new(
                node_id,
                AccessibilityAction::Click,
            )]
        );
        assert!(!router.dispatch_accesskit(node_id, &node, accesskit::Action::Focus));
        assert!(
            router
                .remove_action(node_id, AccessibilityAction::Click)
                .is_some()
        );
        assert!(!router.has_handler(node_id, AccessibilityAction::Click));
    }

    #[test]
    fn form_like_tree_maps_roles_values_and_states() {
        let root = AccessibilityNode::new(AccessibilityRole::Window);
        let root_id = root.id;
        let mut tree = AccessibilityTree::new(root);

        let text = AccessibilityNode::new(AccessibilityRole::TextInput)
            .with_value(AccessibilityValue::Text("Control room".into()));
        let text_id = text.id;
        tree.insert(text);
        tree.set_parent(text_id, root_id);

        let checkbox = AccessibilityNode::new(AccessibilityRole::CheckBox)
            .with_label("Enable notifications")
            .with_states(AccessibilityState::CHECKED);
        let checkbox_id = checkbox.id;
        tree.insert(checkbox);
        tree.set_parent(checkbox_id, root_id);

        let slider = AccessibilityNode::new(AccessibilityRole::Slider).with_value(
            AccessibilityValue::Range {
                current: 65.0,
                min: 0.0,
                max: 100.0,
                step: Some(1.0),
            },
        );
        let slider_id = slider.id;
        tree.insert(slider);
        tree.set_parent(slider_id, root_id);

        let button = AccessibilityNode::new(AccessibilityRole::Button)
            .with_label("Review delivery reset")
            .with_actions(vec![AccessibilityAction::Click, AccessibilityAction::Focus]);
        let button_id = button.id;
        tree.insert(button);
        tree.set_parent(button_id, root_id);

        let update = tree.to_accesskit_tree_update(Some("form_controls"), Some("Kael"), None);
        let lookup = |id: AccessibilityId| {
            update
                .nodes
                .iter()
                .find(|(nid, _)| *nid == accesskit::NodeId(id.0))
                .map(|(_, node)| node)
                .expect("node emitted")
        };

        assert_eq!(lookup(text_id).role(), accesskit::Role::TextInput);
        assert_eq!(lookup(text_id).value(), Some("Control room"));

        let cb = lookup(checkbox_id);
        assert_eq!(cb.role(), accesskit::Role::CheckBox);
        assert_eq!(cb.label(), Some("Enable notifications"));
        assert_eq!(cb.toggled(), Some(accesskit::Toggled::True));

        let sl = lookup(slider_id);
        assert_eq!(sl.role(), accesskit::Role::Slider);
        assert_eq!(sl.numeric_value(), Some(65.0));
        assert_eq!(sl.max_numeric_value(), Some(100.0));

        let btn = lookup(button_id);
        assert_eq!(btn.role(), accesskit::Role::Button);
        assert!(btn.supports_action(accesskit::Action::Click));

        let root_children = lookup(root_id).children().len();
        assert_eq!(root_children, 4);
    }
}
