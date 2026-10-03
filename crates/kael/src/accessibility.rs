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
    collections::{HashMap, HashSet},
    sync::atomic::{AtomicU64, Ordering},
};

mod snapshot;
pub use snapshot::*;
mod text;
pub use text::*;
#[cfg(not(target_family = "wasm"))]
mod action_queue;
#[cfg(any(target_family = "wasm", test))]
pub(crate) mod dom_projection;
#[cfg(all(
    test,
    any(target_os = "linux", target_os = "freebsd", target_os = "windows")
))]
pub(crate) use action_queue::PendingAccessibilityAction;
#[cfg(not(target_family = "wasm"))]
pub(crate) use action_queue::PendingActionQueue;

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
        Self::reserve_range(1)
            .expect("accessibility identifier space exhausted")
            .get(0)
            .unwrap()
    }

    /// Reserve unique identities for a logical coordinate space in constant
    /// time and memory. This reserves identifiers, not accessibility nodes.
    /// Zero-sized reservations and exhausted identity space return `None`.
    pub fn reserve_range(count: u64) -> Option<AccessibilityIdRange> {
        reserve_accessibility_ids(&NEXT_ACCESSIBILITY_ID, count)
    }
}

/// A checked, uniquely allocated logical accessibility identity range.
///
/// Virtual controls can map coordinates to offsets without retaining an ID
/// map for every visited cell. Retiring a range never permits its identities
/// to alias a later model, even while a platform retains the outgoing snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AccessibilityIdRange {
    start: u64,
    count: u64,
}
impl AccessibilityIdRange {
    /// Number of reserved identities. No nodes or labels are allocated.
    pub fn len(self) -> u64 {
        self.count
    }
    /// Valid ranges are always nonempty.
    pub fn is_empty(self) -> bool {
        self.count == 0
    }
    /// Resolve an offset only if it belongs to this reserved range.
    pub fn get(self, offset: u64) -> Option<AccessibilityId> {
        (offset < self.count)
            .then(|| self.start.checked_add(offset).map(AccessibilityId))
            .flatten()
    }
}
fn reserve_accessibility_ids(counter: &AtomicU64, count: u64) -> Option<AccessibilityIdRange> {
    if count == 0 {
        return None;
    }
    let start = counter
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
            current.checked_add(count)
        })
        .ok()?;
    Some(AccessibilityIdRange { start, count })
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
    /// A semantic document or section heading.
    Heading,
    /// A generic grouping container.
    Group,
    /// A list of items.
    List,
    /// A single item within a list.
    ListItem,
    /// A semantic data table.
    Table,
    /// An interactive two-dimensional data grid.
    Grid,
    /// A row within a table or grid.
    Row,
    /// A data cell within a table or grid.
    Cell,
    /// A column header within a table or grid.
    ColumnHeader,
    /// A row header within a table or grid.
    RowHeader,
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
    /// Stable role key for content-safe diagnostics and automation traces.
    pub fn to_text(self) -> &'static str {
        match self {
            Self::Application => "application",
            Self::Window => "window",
            Self::Button => "button",
            Self::TextInput => "text-input",
            Self::StaticText => "static-text",
            Self::Heading => "heading",
            Self::Group => "group",
            Self::List => "list",
            Self::ListItem => "list-item",
            Self::Table => "table",
            Self::Grid => "grid",
            Self::Row => "row",
            Self::Cell => "cell",
            Self::ColumnHeader => "column-header",
            Self::RowHeader => "row-header",
            Self::ScrollBar => "scroll-bar",
            Self::Image => "image",
            Self::Link => "link",
            Self::Menu => "menu",
            Self::MenuItem => "menu-item",
            Self::Tab => "tab",
            Self::TabPanel => "tab-panel",
            Self::Toolbar => "toolbar",
            Self::Tree => "tree",
            Self::TreeItem => "tree-item",
            Self::CheckBox => "check-box",
            Self::RadioButton => "radio-button",
            Self::Slider => "slider",
            Self::ProgressBar => "progress-bar",
            Self::Separator => "separator",
            Self::Pane => "pane",
            Self::Dialog => "dialog",
            Self::Alert => "alert",
            Self::ComboBox => "combo-box",
            Self::Switch => "switch",
            Self::Unknown => "unknown",
        }
    }

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
                | AccessibilityRole::Table
                | AccessibilityRole::Grid
                | AccessibilityRole::Row
                | AccessibilityRole::Menu
                | AccessibilityRole::MenuItem
                | AccessibilityRole::TabPanel
                | AccessibilityRole::Tree
                | AccessibilityRole::TreeItem
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
            Self::Heading => Role::Heading,
            Self::Group | Self::Pane => Role::Group,
            Self::List => Role::List,
            Self::ListItem => Role::ListItem,
            Self::Table => Role::Table,
            Self::Grid => Role::Grid,
            Self::Row => Role::Row,
            Self::Cell => Role::Cell,
            Self::ColumnHeader => Role::ColumnHeader,
            Self::RowHeader => Role::RowHeader,
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

/// Sort order exposed by a table or grid column header.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AccessibilitySortDirection {
    /// Values are ordered from low to high or A to Z.
    Ascending,
    /// Values are ordered from high to low or Z to A.
    Descending,
    /// Values use an application-defined ordering.
    Other,
}

impl AccessibilitySortDirection {
    fn to_accesskit(self) -> accesskit::SortDirection {
        match self {
            Self::Ascending => accesskit::SortDirection::Ascending,
            Self::Descending => accesskit::SortDirection::Descending,
            Self::Other => accesskit::SortDirection::Other,
        }
    }

    /// Stable value used by browser ARIA and diagnostics.
    pub fn to_text(self) -> &'static str {
        match self {
            Self::Ascending => "ascending",
            Self::Descending => "descending",
            Self::Other => "other",
        }
    }
}

impl AccessibilityState {
    /// Number of enabled state flags.
    pub fn enabled_count(self) -> usize {
        self.iter().count()
    }

    /// Stable state summary for content-safe diagnostics.
    pub fn to_text(self) -> String {
        format!(
            "accessibility_state(count={}, focused={}, disabled={}, selected={}, expanded={}, collapsed={}, checked={}, indeterminate={}, pressed={}, required={}, invalid={}, busy={}, hidden={}, read_only={})",
            self.enabled_count(),
            self.contains(Self::FOCUSED),
            self.contains(Self::DISABLED),
            self.contains(Self::SELECTED),
            self.contains(Self::EXPANDED),
            self.contains(Self::COLLAPSED),
            self.contains(Self::CHECKED),
            self.contains(Self::INDETERMINATE),
            self.contains(Self::PRESSED),
            self.contains(Self::REQUIRED),
            self.contains(Self::INVALID),
            self.contains(Self::BUSY),
            self.contains(Self::HIDDEN),
            self.contains(Self::READ_ONLY)
        )
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
        /// The value can be reviewed and focused but not edited.
        const READ_ONLY = 1 << 12;
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
    /// Reveal a logical item in its scroll viewport without selecting it.
    ScrollToVisible,
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
    /// Set directed text selection or caret using the prepared document.
    SetTextSelection,
    /// Replace an immutable document range as one editing transaction.
    ReplaceSelectedText,
    /// Copy a checked document range to the platform clipboard.
    CopyText,
    /// Copy and atomically remove a checked document range.
    CutText,
    /// Atomically replace a checked document range with clipboard text.
    PasteText,
    /// Open the element's associated menu.
    ShowMenu,
    /// Dismiss the element (e.g., close a dialog).
    Dismiss,
    /// A custom action with an application-defined identifier.
    Custom(u32),
}

impl AccessibilityAction {
    /// Stable action key for content-safe diagnostics.
    pub fn to_text(self) -> &'static str {
        match self {
            Self::Click => "click",
            Self::Focus => "focus",
            Self::ScrollUp => "scroll-up",
            Self::ScrollDown => "scroll-down",
            Self::ScrollToVisible => "scroll-to-visible",
            Self::Expand => "expand",
            Self::Collapse => "collapse",
            Self::Toggle => "toggle",
            Self::Increment => "increment",
            Self::Decrement => "decrement",
            Self::SetValue => "set-value",
            Self::SetTextSelection => "set-text-selection",
            Self::ReplaceSelectedText => "replace-selected-text",
            Self::CopyText => "copy-text",
            Self::CutText => "cut-text",
            Self::PasteText => "paste-text",
            Self::ShowMenu => "show-menu",
            Self::Dismiss => "dismiss",
            Self::Custom(_) => "custom",
        }
    }

    /// Map to the closest AccessKit [`accesskit::Action`].
    pub fn to_accesskit(self) -> accesskit::Action {
        use accesskit::Action;
        match self {
            Self::Click | Self::Toggle => Action::Click,
            Self::Focus => Action::Focus,
            Self::ScrollUp => Action::ScrollUp,
            Self::ScrollDown => Action::ScrollDown,
            Self::ScrollToVisible => Action::ScrollIntoView,
            Self::Expand => Action::Expand,
            Self::Collapse => Action::Collapse,
            Self::Increment => Action::Increment,
            Self::Decrement => Action::Decrement,
            Self::SetValue => Action::SetValue,
            Self::SetTextSelection => Action::SetTextSelection,
            Self::ReplaceSelectedText => Action::ReplaceSelectedText,
            Self::CopyText | Self::CutText | Self::PasteText => Action::CustomAction,
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
            Action::ScrollIntoView => Some(Self::ScrollToVisible),
            Action::Expand => Some(Self::Expand),
            Action::Collapse => Some(Self::Collapse),
            Action::Increment => Some(Self::Increment),
            Action::Decrement => Some(Self::Decrement),
            Action::SetValue => Some(Self::SetValue),
            Action::SetTextSelection => Some(Self::SetTextSelection),
            Action::ReplaceSelectedText => Some(Self::ReplaceSelectedText),
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
    /// Directed, checked UTF-8 byte endpoints in one immutable text document.
    /// Handlers must check the document identity again before applying a
    /// deferred request, since editing may replace the document meanwhile.
    TextSelection {
        /// Immutable prepared document identity that owns these endpoints.
        document_id: AccessibilityId,
        /// Fixed selection endpoint.
        anchor: usize,
        /// Active endpoint or caret.
        focus: usize,
    },
    /// Reveal a run's checked byte span without changing selection or focus.
    TextReveal {
        /// Immutable document owning the byte span.
        document_id: AccessibilityId,
        /// First byte of the targeted run.
        start: usize,
        /// Exclusive last byte of the targeted run.
        end: usize,
        /// Native suggested viewport placement.
        alignment: AccessibilityTextAlignment,
    },
    /// Atomic editing request against one immutable document revision.
    TextReplacement {
        /// Immutable document owning the editing range.
        document_id: AccessibilityId,
        /// Inclusive range start in UTF-8 bytes.
        start: usize,
        /// Exclusive range end in UTF-8 bytes.
        end: usize,
        /// Replacement UTF-8 text, charged to the native queue byte budget.
        value: String,
    },
}

impl AccessibilityActionPayload {
    /// Stable payload kind for content-safe diagnostics.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Value(_) => "value",
            Self::NumericValue(_) => "numeric-value",
            Self::TextSelection { .. } => "text-selection",
            Self::TextReveal { .. } => "text-reveal",
            Self::TextReplacement { .. } => "text-replacement",
        }
    }

    /// Byte length of text payload data without exposing the payload text.
    pub fn value_len_bytes(&self) -> usize {
        match self {
            Self::Value(value) | Self::TextReplacement { value, .. } => value.len(),
            Self::NumericValue(_) | Self::TextSelection { .. } | Self::TextReveal { .. } => 0,
        }
    }

    /// Whether the payload carries a finite numeric value.
    pub fn has_finite_numeric_value(&self) -> bool {
        match self {
            Self::Value(_)
            | Self::TextSelection { .. }
            | Self::TextReveal { .. }
            | Self::TextReplacement { .. } => false,
            Self::NumericValue(value) => value.is_finite(),
        }
    }

    /// Content-safe payload summary.
    pub fn to_text(&self) -> String {
        format!(
            "accessibility_action_payload(kind={}, value_len_bytes={}, finite_numeric={})",
            self.kind(),
            self.value_len_bytes(),
            self.has_finite_numeric_value()
        )
    }
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
        // Raw TextPositions cannot be normalized without their owning document.
        if action == accesskit::Action::SetTextSelection {
            return None;
        }
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
    /// several Kael actions map to the same platform action. Disabled and hidden
    /// nodes reject every action, even if they still advertise the action.
    pub fn from_accesskit_for_node_with_data(
        node_id: AccessibilityId,
        node: &AccessibilityNode,
        action: accesskit::Action,
        data: Option<accesskit::ActionData>,
    ) -> Option<Self> {
        if node
            .states
            .intersects(AccessibilityState::DISABLED | AccessibilityState::HIDDEN)
        {
            return None;
        }
        if node.states.contains(AccessibilityState::READ_ONLY)
            && matches!(
                action,
                accesskit::Action::SetValue | accesskit::Action::ReplaceSelectedText
            )
        {
            return None;
        }
        if action == accesskit::Action::SetTextSelection {
            if !node
                .actions
                .contains(&AccessibilityAction::SetTextSelection)
            {
                return None;
            }
            let accesskit::ActionData::SetTextSelection(selection) = data? else {
                return None;
            };
            let document = node.text_document.as_ref()?;
            let selection = document.import_selection(selection)?;
            return Some(Self::with_payload(
                node_id,
                AccessibilityAction::SetTextSelection,
                AccessibilityActionPayload::TextSelection {
                    document_id: document.id(),
                    anchor: selection.anchor,
                    focus: selection.focus,
                },
            ));
        }
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

    /// Returns true when the request includes a payload.
    pub fn has_payload(&self) -> bool {
        self.payload.is_some()
    }

    /// Stable payload kind, or `none`.
    pub fn payload_kind(&self) -> &'static str {
        self.payload
            .as_ref()
            .map(AccessibilityActionPayload::kind)
            .unwrap_or("none")
    }

    /// Content-safe action request summary.
    pub fn to_text(&self) -> String {
        let payload_summary = self
            .payload
            .as_ref()
            .map(AccessibilityActionPayload::to_text)
            .unwrap_or_else(|| "none".to_string());
        format!(
            "accessibility_action_request(action={}, has_payload={}, payload_kind={}, payload={})",
            self.action.to_text(),
            self.has_payload(),
            self.payload_kind(),
            payload_summary
        )
    }
}

/// Routes normalized accessibility action requests to application handlers.
///
/// Platform adapters can feed this router after converting native action
/// callbacks into [`AccessibilityActionRequest`]. Applications can also use it
/// directly in tests or custom accessibility integrations.
#[derive(Default)]
pub struct AccessibilityActionRouter {
    handlers: HashMap<(AccessibilityId, AccessibilityAction), RegisteredAccessibilityActionHandler>,
    subtree_handlers: HashMap<
        AccessibilityId,
        (
            std::sync::Arc<AccessibilitySnapshot>,
            AccessibilityActionHandler,
        ),
    >,
}

type AccessibilityActionHandler = Box<dyn FnMut(AccessibilityActionRequest) + 'static>;

struct RegisteredAccessibilityActionHandler {
    handler: AccessibilityActionHandler,
    frame_only: bool,
}

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
        self.handlers.insert(
            (node_id, action),
            RegisteredAccessibilityActionHandler {
                handler: Box::new(handler),
                frame_only: false,
            },
        );
    }

    /// A painted element's handler cannot outlive its frame overlay. Logical
    /// subtree routing supplies offscreen actions without retaining old views.
    pub(crate) fn on_frame_action(
        &mut self,
        node_id: AccessibilityId,
        action: AccessibilityAction,
        handler: impl FnMut(AccessibilityActionRequest) + 'static,
    ) {
        self.handlers.insert(
            (node_id, action),
            RegisteredAccessibilityActionHandler {
                handler: Box::new(handler),
                frame_only: true,
            },
        );
    }

    /// Remove a handler for one node/action pair.
    pub fn remove_action(
        &mut self,
        node_id: AccessibilityId,
        action: AccessibilityAction,
    ) -> Option<AccessibilityActionHandler> {
        self.handlers
            .remove(&(node_id, action))
            .map(|entry| entry.handler)
    }

    /// Return whether a handler is registered for one node/action pair.
    pub fn has_handler(&self, node_id: AccessibilityId, action: AccessibilityAction) -> bool {
        self.handlers.contains_key(&(node_id, action))
            || self.subtree_handlers.values().any(|(snapshot, _)| {
                snapshot
                    .get(node_id)
                    .is_some_and(|node| node.actions.contains(&action))
            })
    }

    /// Route all supported actions in a logical subtree through one handler.
    pub fn on_subtree(
        &mut self,
        snapshot: std::sync::Arc<AccessibilitySnapshot>,
        handler: impl FnMut(AccessibilityActionRequest) + 'static,
    ) {
        self.subtree_handlers
            .insert(snapshot.root, (snapshot, Box::new(handler)));
    }

    /// Retain explicit handlers while their logical nodes exist. Painted
    /// handlers need a current frame overlay and retire when subtree routing
    /// already supplies the action, releasing outgoing view/model captures.
    /// This never collects or scans all logical node identities.
    pub fn retain_tree(&mut self, tree: &AccessibilityTree) {
        self.subtree_handlers.retain(|root, (snapshot, _)| {
            tree.nodes
                .snapshots
                .get(root)
                .is_some_and(|current| std::sync::Arc::ptr_eq(current, snapshot))
        });
        self.handlers.retain(|(id, action), entry| {
            if entry.frame_only {
                tree.nodes.frame.get(id).is_some_and(|node| {
                    node.actions.contains(action)
                        && !self.subtree_handlers.values().any(|(snapshot, _)| {
                            snapshot
                                .get(*id)
                                .is_some_and(|node| node.actions.contains(action))
                        })
                })
            } else {
                tree.get(*id).is_some()
            }
        });
    }

    /// Remove handlers for nodes that are no longer present in the active tree.
    pub fn retain_nodes(&mut self, node_ids: impl IntoIterator<Item = AccessibilityId>) {
        let node_ids: HashSet<_> = node_ids.into_iter().collect();
        self.handlers
            .retain(|(node_id, _), _| node_ids.contains(node_id));
    }

    /// Dispatch a normalized action request. Returns true when a handler ran.
    pub fn dispatch(&mut self, request: AccessibilityActionRequest) -> bool {
        // Logical subtree actions take priority over a mounted element's
        // synthetic pointer handler, which may capture an outgoing row index.
        for (snapshot, handler) in self.subtree_handlers.values_mut() {
            if snapshot
                .get(request.node_id)
                .is_some_and(|node| node.actions.contains(&request.action))
            {
                handler(request);
                return true;
            }
        }
        if let Some(entry) = self.handlers.get_mut(&(request.node_id, request.action)) {
            (entry.handler)(request);
            return true;
        }
        false
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

    /// Number of registered action handlers.
    pub fn handler_count(&self) -> usize {
        self.handlers.len()
    }

    /// Content-safe router summary.
    pub fn to_text(&self) -> String {
        format!(
            "accessibility_action_router(handler_count={})",
            self.handler_count()
        )
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

impl AccessibilityValue {
    /// Stable value kind for content-safe diagnostics.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Text(_) => "text",
            Self::Number(_) => "number",
            Self::Range { .. } => "range",
            Self::Toggle(_) => "toggle",
        }
    }

    /// Text byte length without exposing text value content.
    pub fn text_len_bytes(&self) -> usize {
        match self {
            Self::Text(text) => text.len(),
            _ => 0,
        }
    }

    /// Returns true when numeric content is finite and range content is valid.
    pub fn is_finite_or_valid(&self) -> bool {
        match self {
            Self::Text(_) | Self::Toggle(_) => true,
            Self::Number(number) => number.is_finite(),
            Self::Range {
                current,
                min,
                max,
                step,
            } => {
                current.is_finite()
                    && min.is_finite()
                    && max.is_finite()
                    && min <= max
                    && current >= min
                    && current <= max
                    && step.is_none_or(|step| step.is_finite() && step > 0.0)
            }
        }
    }

    /// Returns true when the value is a range with an explicit step.
    pub fn has_step(&self) -> bool {
        matches!(self, Self::Range { step: Some(_), .. })
    }

    /// Content-safe value summary.
    pub fn to_text(&self) -> String {
        format!(
            "accessibility_value(kind={}, text_len_bytes={}, finite_or_valid={}, has_step={})",
            self.kind(),
            self.text_len_bytes(),
            self.is_finite_or_valid(),
            self.has_step()
        )
    }
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

    /// Coarse size class for content-safe geometry summaries.
    pub fn size_class(&self) -> &'static str {
        let max_axis = self.width.max(self.height);
        if self.width <= 0.0 || self.height <= 0.0 {
            "empty"
        } else if max_axis < 24.0 {
            "tiny"
        } else if max_axis < 96.0 {
            "small"
        } else if max_axis < 320.0 {
            "medium"
        } else {
            "large"
        }
    }

    /// Returns true when the rect has positive area.
    pub fn has_area(&self) -> bool {
        self.width > 0.0 && self.height > 0.0
    }

    /// Content-safe geometry summary that avoids exact coordinates and sizes.
    pub fn to_text(&self) -> String {
        format!(
            "accessibility_rect(size_class={}, has_area={})",
            self.size_class(),
            self.has_area()
        )
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
    /// Hierarchical level for semantic headings and tree-like items.
    pub level: Option<usize>,
    /// Logical number of rows in a table or grid, including virtualized rows.
    pub row_count: Option<usize>,
    /// Logical number of columns in a table or grid, including virtualized columns.
    pub column_count: Option<usize>,
    /// One-based logical row index for a row or cell.
    pub row_index: Option<usize>,
    /// One-based logical column index for a cell or header.
    pub column_index: Option<usize>,
    /// Number of logical rows occupied by a cell.
    pub row_span: Option<usize>,
    /// Number of logical columns occupied by a cell.
    pub column_span: Option<usize>,
    /// Sort order for a column header.
    pub sort_direction: Option<AccessibilitySortDirection>,
    /// Screen-space bounds, once produced by layout.
    pub bounds: Option<AccessibilityRect>,
    /// Actions that can be invoked on this element.
    pub actions: Vec<AccessibilityAction>,
    /// Child node identifiers.
    pub children: AccessibilityChildren,
    /// Parent node identifier, if any.
    pub parent: Option<AccessibilityId>,
    /// Active logical descendant when keyboard focus stays on this container.
    /// The referenced node must be present in the accessible tree.
    pub active_descendant: Option<AccessibilityId>,
    /// Shared complete text runs, prepared once per document revision.
    pub text_document: Option<std::sync::Arc<AccessibilityTextDocument>>,
    /// Directed UTF-8 selection within `text_document`.
    pub text_selection: Option<AccessibilityTextSelection>,
    /// Actual bounded text-run geometry for this viewport.
    pub text_geometry: Option<std::sync::Arc<AccessibilityTextGeometry>>,
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
            level: None,
            row_count: None,
            column_count: None,
            row_index: None,
            column_index: None,
            row_span: None,
            column_span: None,
            sort_direction: None,
            bounds: None,
            actions: Vec::new(),
            children: AccessibilityChildren::default(),
            parent: None,
            active_descendant: None,
            text_document: None,
            text_selection: None,
            text_geometry: None,
        }
    }

    /// Convert this node to an AccessKit node (P3-A, roadmap §9 action 12).
    ///
    /// Maps role, label, description, child ids, and geometry. Bounds are emitted
    /// when [`AccessibilityNode::bounds`] is populated; the remaining P3-A work is
    /// the layout-side wiring that fills those bounds for every laid-out element.
    pub fn to_accesskit_node(&self) -> accesskit::Node {
        self.to_accesskit_node_with_children(true)
    }

    fn to_accesskit_node_with_children(&self, include_children: bool) -> accesskit::Node {
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
        if let Some(level) = self.level {
            node.set_level(level);
        }
        if let Some(bounds) = &self.bounds {
            node.set_bounds(bounds.to_accesskit());
        }
        if self.text_document.is_none() {
            apply_value(&mut node, self);
        }
        if let Some(document) = &self.text_document {
            // Native range providers read text from retained runs. Avoid copying
            // the full document into every root record on caret-only updates.
            node.clear_value();
            if self.actions.contains(&AccessibilityAction::ScrollToVisible) {
                node.add_child_action(accesskit::Action::ScrollIntoView);
            }
            if self.role == AccessibilityRole::TextInput && document.is_multiline() {
                node.set_role(accesskit::Role::MultilineTextInput);
            }
            if let Some(selection) = self
                .text_selection
                .and_then(|selection| document.export_selection(selection))
            {
                node.set_text_selection(selection);
            }
        }
        apply_states(&mut node, self.states);
        apply_collection_metadata(&mut node, self);
        if let Some(id) = self.active_descendant {
            node.set_active_descendant(accesskit::NodeId(id.0));
        }
        for action in &self.actions {
            node.add_action(action.to_accesskit());
        }
        let custom_actions = self
            .actions
            .iter()
            .filter_map(|action| {
                let (id, label) = match action {
                    AccessibilityAction::CopyText => (i32::MIN, "Copy text"),
                    AccessibilityAction::CutText => (i32::MIN + 1, "Cut text"),
                    AccessibilityAction::PasteText => (i32::MIN + 2, "Paste text"),
                    _ => return None,
                };
                Some(accesskit::CustomAction {
                    id,
                    description: label.into(),
                })
            })
            .collect::<Vec<_>>();
        if !custom_actions.is_empty() {
            node.set_custom_actions(custom_actions);
        }

        if include_children {
            let children: Vec<accesskit::NodeId> = self
                .children
                .iter()
                .map(|child| accesskit::NodeId(child.0))
                .collect();
            let children = children
                .into_iter()
                .chain(
                    self.text_document
                        .iter()
                        .flat_map(|document| document.run_ids()),
                )
                .collect::<Vec<_>>();
            node.set_children(children);
        }
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

    /// Set a semantic hierarchy level.
    pub fn with_level(mut self, level: usize) -> Self {
        self.level = Some(level);
        self
    }

    /// Set the logical row count, including virtualized rows.
    pub fn with_row_count(mut self, count: usize) -> Self {
        self.row_count = Some(count);
        self
    }

    /// Set the logical column count, including virtualized columns.
    pub fn with_column_count(mut self, count: usize) -> Self {
        self.column_count = Some(count);
        self
    }

    /// Set a one-based logical row index.
    pub fn with_row_index(mut self, index: usize) -> Self {
        self.row_index = Some(index);
        self
    }

    /// Set a one-based logical column index.
    pub fn with_column_index(mut self, index: usize) -> Self {
        self.column_index = Some(index);
        self
    }

    /// Set the number of logical rows occupied by a cell.
    pub fn with_row_span(mut self, span: usize) -> Self {
        self.row_span = Some(span);
        self
    }

    /// Set the number of logical columns occupied by a cell.
    pub fn with_column_span(mut self, span: usize) -> Self {
        self.column_span = Some(span);
        self
    }

    /// Set the sort order exposed by a column header.
    pub fn with_sort_direction(mut self, direction: AccessibilitySortDirection) -> Self {
        self.sort_direction = Some(direction);
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

    /// Byte length of the accessible label, without exposing label text.
    pub fn label_len_bytes(&self) -> usize {
        self.label.as_ref().map_or(0, |label| label.len())
    }

    /// Byte length of the accessible description, without exposing it.
    pub fn description_len_bytes(&self) -> usize {
        self.description
            .as_ref()
            .map_or(0, |description| description.len())
    }

    /// Byte length of the placeholder, without exposing placeholder text.
    pub fn placeholder_len_bytes(&self) -> usize {
        self.placeholder
            .as_ref()
            .map_or(0, |placeholder| placeholder.len())
    }

    /// Returns true when the node has an accessible label.
    pub fn has_label(&self) -> bool {
        self.label.is_some()
    }

    /// Returns true when the node has an accessible description.
    pub fn has_description(&self) -> bool {
        self.description.is_some()
    }

    /// Returns true when the node has an accessible value.
    pub fn has_value(&self) -> bool {
        self.value.is_some()
    }

    /// Returns true when the node has placeholder text.
    pub fn has_placeholder(&self) -> bool {
        self.placeholder.is_some()
    }

    /// Returns true when the node has layout bounds.
    pub fn has_bounds(&self) -> bool {
        self.bounds.is_some()
    }

    /// Returns true when the node has a parent id.
    pub fn has_parent(&self) -> bool {
        self.parent.is_some()
    }

    /// Number of advertised accessibility actions.
    pub fn action_count(&self) -> usize {
        self.actions.len()
    }

    /// Number of child node ids.
    pub fn child_count(&self) -> usize {
        self.children.len()
    }

    /// Content-safe node summary for diagnostics and automation traces.
    pub fn to_text(&self) -> String {
        let value_summary = self
            .value
            .as_ref()
            .map(AccessibilityValue::to_text)
            .unwrap_or_else(|| "none".to_string());
        let bounds_summary = self
            .bounds
            .as_ref()
            .map(AccessibilityRect::to_text)
            .unwrap_or_else(|| "none".to_string());
        format!(
            "accessibility_node(role={}, interactive={}, container={}, states={}, has_label={}, label_len_bytes={}, has_description={}, description_len_bytes={}, has_value={}, value={}, has_placeholder={}, placeholder_len_bytes={}, level={}, has_bounds={}, bounds={}, action_count={}, child_count={}, has_parent={})",
            self.role.to_text(),
            self.role.is_interactive(),
            self.role.is_container(),
            self.states.to_text(),
            self.has_label(),
            self.label_len_bytes(),
            self.has_description(),
            self.description_len_bytes(),
            self.has_value(),
            value_summary,
            self.has_placeholder(),
            self.placeholder_len_bytes(),
            self.level
                .map_or_else(|| "none".to_string(), |level| level.to_string()),
            self.has_bounds(),
            bounds_summary,
            self.action_count(),
            self.child_count(),
            self.has_parent()
        )
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
    pub nodes: AccessibilityNodeMap,
}

impl AccessibilityTree {
    /// Normalize against the current tree, resolving exported text-run IDs to
    /// their owning control. Lookup visits bounded mounted nodes and prepared
    /// text owners, never a document's individual runs or immutable tree rows.
    pub fn normalize_accesskit_action(
        &self,
        target: AccessibilityId,
        action: accesskit::Action,
        data: Option<accesskit::ActionData>,
    ) -> Option<AccessibilityActionRequest> {
        if let Some(node) = self.get(target) {
            if !self.is_exported(target) {
                return None;
            }
            let clipboard_action = match data.as_ref() {
                Some(accesskit::ActionData::CustomAction(i32::MIN)) => {
                    Some(AccessibilityAction::CopyText)
                }
                Some(accesskit::ActionData::CustomAction(id)) if *id == i32::MIN + 1 => {
                    Some(AccessibilityAction::CutText)
                }
                Some(accesskit::ActionData::CustomAction(id)) if *id == i32::MIN + 2 => {
                    Some(AccessibilityAction::PasteText)
                }
                _ => None,
            };
            if action == accesskit::Action::CustomAction
                && let Some(action) = clipboard_action
            {
                let document = node.text_document.as_ref()?;
                let raw = document.export_selection(node.text_selection?)?;
                return self.normalize_text_clipboard(target, raw, action);
            }
            if action == accesskit::Action::ReplaceSelectedText {
                let accesskit::ActionData::Value(value) = data? else {
                    return None;
                };
                let document = node.text_document.as_ref()?;
                let raw = document.export_selection(node.text_selection?)?;
                return self.normalize_text_replacement(target, raw, value.into());
            }
            return AccessibilityActionRequest::from_accesskit_for_node_with_data(
                target, node, action, data,
            );
        }
        if action != accesskit::Action::ScrollIntoView {
            return None;
        }
        let owner = self.nodes.text_owner(target)?;
        if !self.is_exported(owner.id)
            || owner
                .states
                .intersects(AccessibilityState::DISABLED | AccessibilityState::HIDDEN)
            || !owner
                .actions
                .contains(&AccessibilityAction::ScrollToVisible)
        {
            return None;
        }
        let document = owner.text_document.as_ref()?;
        let bytes = document.run_bytes(target)?;
        Some(AccessibilityActionRequest::with_payload(
            owner.id,
            AccessibilityAction::ScrollToVisible,
            AccessibilityActionPayload::TextReveal {
                document_id: document.id(),
                start: bytes.start,
                end: bytes.end,
                alignment: AccessibilityTextAlignment::from_data(data),
            },
        ))
    }

    /// Normalize one atomic replacement captured by a native text provider.
    /// Raw run IDs preserve origin until this conversion; foreign/stale IDs,
    /// unsupported editing, and read-only/disabled/hidden controls are rejected.
    pub fn normalize_text_replacement(
        &self,
        owner_id: AccessibilityId,
        selection: accesskit::TextSelection,
        value: String,
    ) -> Option<AccessibilityActionRequest> {
        let owner = self.get(owner_id)?;
        if !self.is_exported(owner_id)
            || owner.states.intersects(
                AccessibilityState::DISABLED
                    | AccessibilityState::HIDDEN
                    | AccessibilityState::READ_ONLY,
            )
            || !owner
                .actions
                .contains(&AccessibilityAction::ReplaceSelectedText)
        {
            return None;
        }
        let document = owner.text_document.as_ref()?;
        let selection = document.import_selection(selection)?;
        Some(AccessibilityActionRequest::with_payload(
            owner_id,
            AccessibilityAction::ReplaceSelectedText,
            AccessibilityActionPayload::TextReplacement {
                document_id: document.id(),
                start: selection.anchor.min(selection.focus),
                end: selection.anchor.max(selection.focus),
                value,
            },
        ))
    }

    /// Capture a native clipboard operation without mutating selection first.
    pub fn normalize_text_clipboard(
        &self,
        owner_id: AccessibilityId,
        selection: accesskit::TextSelection,
        action: AccessibilityAction,
    ) -> Option<AccessibilityActionRequest> {
        if !matches!(
            action,
            AccessibilityAction::CopyText
                | AccessibilityAction::CutText
                | AccessibilityAction::PasteText
        ) {
            return None;
        }
        let owner = self.get(owner_id)?;
        if !self.is_exported(owner_id)
            || !owner.actions.contains(&action)
            || owner
                .states
                .intersects(AccessibilityState::DISABLED | AccessibilityState::HIDDEN)
            || (action != AccessibilityAction::CopyText
                && owner.states.contains(AccessibilityState::READ_ONLY))
        {
            return None;
        }
        let document = owner.text_document.as_ref()?;
        let selection = document.import_selection(selection)?;
        Some(AccessibilityActionRequest::with_payload(
            owner_id,
            action,
            AccessibilityActionPayload::TextSelection {
                document_id: document.id(),
                anchor: selection.anchor,
                focus: selection.focus,
            },
        ))
    }

    /// Revalidate a previously normalized request before deferred dispatch.
    pub fn validate_action_request(
        &self,
        request: AccessibilityActionRequest,
    ) -> Option<AccessibilityActionRequest> {
        let node = self.get(request.node_id)?;
        if !self.is_exported(node.id)
            || node
                .states
                .intersects(AccessibilityState::DISABLED | AccessibilityState::HIDDEN)
            || !node.actions.contains(&request.action)
        {
            return None;
        }
        if node.states.contains(AccessibilityState::READ_ONLY)
            && matches!(
                request.action,
                AccessibilityAction::SetValue
                    | AccessibilityAction::CutText
                    | AccessibilityAction::PasteText
                    | AccessibilityAction::ReplaceSelectedText
            )
        {
            return None;
        }
        let selection = match request.payload.as_ref() {
            Some(AccessibilityActionPayload::TextSelection {
                document_id,
                anchor,
                focus,
            }) => Some((*document_id, *anchor, *focus)),
            Some(AccessibilityActionPayload::TextReveal {
                document_id,
                start,
                end,
                ..
            }) => Some((*document_id, *start, *end)),
            Some(AccessibilityActionPayload::TextReplacement {
                document_id,
                start,
                end,
                ..
            }) => {
                if node.states.contains(AccessibilityState::READ_ONLY) {
                    return None;
                }
                Some((*document_id, *start, *end))
            }
            _ => None,
        };
        if let Some((id, anchor, focus)) = selection {
            let document = node.text_document.as_ref()?;
            if document.id() != id
                || !document.contains_selection(AccessibilityTextSelection { anchor, focus })
            {
                return None;
            }
        }
        Some(request)
    }
    /// Create a new tree with the given root node.
    pub fn new(root: AccessibilityNode) -> Self {
        let root_id = root.id;
        let mut nodes = AccessibilityNodeMap::default();
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
        self.nodes
            .frame
            .iter()
            .find_map(|(id, node)| {
                if node.states.contains(AccessibilityState::FOCUSED) {
                    Some(*id)
                } else {
                    None
                }
            })
            .or_else(|| {
                self.nodes
                    .snapshots
                    .values()
                    .filter_map(|snapshot| snapshot.focused)
                    .find(|id| {
                        self.get(*id)
                            .is_some_and(|node| node.states.contains(AccessibilityState::FOCUSED))
                    })
            })
    }

    /// Semantic records changed since a previous frame, skipping shared snapshots.
    pub fn delta<'a>(&'a self, previous: Option<&Self>) -> AccessibilityTreeDelta<'a> {
        self.nodes.delta(previous.map(|tree| &tree.nodes))
    }

    /// Returns true when the root id is present in the node map.
    pub fn has_root_node(&self) -> bool {
        self.nodes.contains_key(&self.root)
    }

    /// Number of nodes in the tree.
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// Number of interactive nodes.
    pub fn interactive_node_count(&self) -> usize {
        self.nodes
            .values()
            .filter(|node| node.role.is_interactive())
            .count()
    }

    /// Number of nodes with at least one action.
    pub fn actionable_node_count(&self) -> usize {
        self.nodes
            .values()
            .filter(|node| !node.actions.is_empty())
            .count()
    }

    /// Number of hidden nodes.
    pub fn hidden_node_count(&self) -> usize {
        self.nodes
            .values()
            .filter(|node| node.states.contains(AccessibilityState::HIDDEN))
            .count()
    }

    /// Number of focused nodes, even if malformed.
    pub fn focused_node_count(&self) -> usize {
        self.nodes
            .values()
            .filter(|node| node.states.contains(AccessibilityState::FOCUSED))
            .count()
    }

    /// Number of edges implied by node children.
    pub fn edge_count(&self) -> usize {
        self.nodes.values().map(|node| node.children.len()).sum()
    }

    /// Content-safe tree summary for diagnostics and automation traces.
    pub fn to_text(&self) -> String {
        let report = self.audit_report();
        format!(
            "accessibility_tree(nodes={}, edges={}, has_root={}, focused_nodes={}, interactive_nodes={}, actionable_nodes={}, hidden_nodes={}, audit_errors={}, audit_warnings={}, ready={})",
            self.node_count(),
            self.edge_count(),
            self.has_root_node(),
            self.focused_node_count(),
            self.interactive_node_count(),
            self.actionable_node_count(),
            self.hidden_node_count(),
            report.error_count(),
            report.warning_count(),
            report.is_ready()
        )
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

            let mut ak_node = node.to_accesskit_node_with_children(false);
            if node
                .active_descendant
                .is_some_and(|id| !self.is_exported(id))
            {
                ak_node.clear_active_descendant();
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
            child_ids.extend(
                node.text_document
                    .iter()
                    .flat_map(|document| document.run_ids()),
            );
            ak_node.set_children(child_ids);

            nodes.push((accesskit::NodeId(id.0), ak_node));
            if let Some(document) = &node.text_document {
                nodes.extend(document.export_runs(node.text_geometry.as_deref()));
            }
        }

        let focus = self
            .focused_node()
            .filter(|id| self.is_exported(*id))
            .map(|id| accesskit::NodeId(id.0))
            .unwrap_or(root_id);

        let mut tree = accesskit::Tree::new(root_id);
        tree.toolkit_name = toolkit_name.map(str::to_owned);
        tree.toolkit_version = toolkit_version.map(str::to_owned);

        accesskit::TreeUpdate {
            nodes,
            tree: Some(tree),
            tree_id: accesskit::TreeId::ROOT,
            focus,
        }
    }

    fn is_exported(&self, id: AccessibilityId) -> bool {
        let mut cursor = id;
        // A malformed parent cycle is rejected without allocating a visited set.
        for _ in 0..=self.node_count() {
            let Some(node) = self.get(cursor) else {
                return false;
            };
            if cursor == self.root {
                return true;
            }
            if node.states.contains(AccessibilityState::HIDDEN) {
                return false;
            }
            let Some(parent) = node.parent else {
                return false;
            };
            let shared_edge = self
                .nodes
                .base(&cursor)
                .is_some_and(|node| node.parent == Some(parent))
                && self.nodes.base(&parent).is_some_and(|base| {
                    self.get(parent)
                        .is_some_and(|node| node.children.shares_storage(&base.children))
                });
            if !shared_edge
                && !self
                    .get(parent)
                    .is_some_and(|node| node.children.contains(&cursor))
            {
                return false;
            }
            cursor = parent;
        }
        false
    }

    /// Build an incremental AccessKit update ordered safely relative to the previous tree.
    ///
    /// AccessKit applies node records sequentially. When a retained node moves
    /// between parents, the old parent must release it before the new parent
    /// claims it. Otherwise the consumer can transiently orphan a newly added
    /// node and panic while emitting platform change events. This is especially
    /// easy to trigger when scrolling changes which semantic containers are
    /// painted in a frame.
    pub fn to_accesskit_tree_update_after(
        &self,
        previous: Option<&Self>,
        toolkit_name: Option<&str>,
        toolkit_version: Option<&str>,
    ) -> accesskit::TreeUpdate {
        let Some(previous) = previous else {
            return self.to_accesskit_tree_update(toolkit_name, toolkit_version);
        };
        let delta = self.delta(Some(previous));
        let mut changed: HashSet<_> = delta.nodes.iter().map(|node| node.id).collect();
        for id in &delta.removed {
            let mut parent = previous.get(*id).and_then(|node| node.parent);
            while let Some(id) = parent {
                if self.is_exported(id) {
                    changed.insert(id);
                    break;
                }
                parent = previous.get(id).and_then(|node| node.parent);
            }
        }
        for node in &delta.nodes {
            let old_visible = previous.is_exported(node.id);
            let visible = self.is_exported(node.id);
            if old_visible != visible {
                if let Some(parent) = node.parent {
                    changed.insert(parent);
                }
                if let Some(parent) = previous.get(node.id).and_then(|node| node.parent) {
                    changed.insert(parent);
                }
                if visible {
                    // Previously pruned descendants need records when an
                    // ancestor becomes visible, even if their data is unchanged.
                    let mut pending = node.children.to_vec();
                    let mut seen = HashSet::new();
                    while let Some(id) = pending.pop() {
                        if seen.insert(id) && self.is_exported(id) {
                            changed.insert(id);
                            if let Some(node) = self.get(id) {
                                pending.extend(node.children.iter().copied());
                            }
                        }
                    }
                }
            }
        }
        let mut nodes = Vec::with_capacity(changed.len());
        let mut changed: Vec<_> = changed.into_iter().collect();
        changed.sort_unstable_by_key(|id| id.0);
        for id in changed {
            let Some(node) = self.get(id) else {
                continue;
            };
            if !self.is_exported(node.id) {
                continue;
            }
            let mut exported = node.to_accesskit_node_with_children(false);
            if node
                .active_descendant
                .is_some_and(|id| !self.is_exported(id))
            {
                exported.clear_active_descendant();
            }
            // Only changed parent records copy child identities; retained logical
            // rows do not copy labels or child lists on ordinary redraws.
            exported.set_children(
                node.children
                    .iter()
                    .filter(|id| {
                        self.get(**id)
                            .is_some_and(|child| !child.states.contains(AccessibilityState::HIDDEN))
                    })
                    .map(|id| accesskit::NodeId(id.0))
                    .chain(
                        node.text_document
                            .iter()
                            .flat_map(|document| document.run_ids()),
                    )
                    .collect::<Vec<_>>(),
            );
            nodes.push((accesskit::NodeId(node.id.0), exported));
            if let Some(document) = &node.text_document {
                let retained = previous
                    .get(node.id)
                    .filter(|_| previous.is_exported(node.id))
                    .and_then(|node| node.text_document.as_ref())
                    .is_some_and(|old| std::sync::Arc::ptr_eq(old, document));
                if !retained {
                    nodes.extend(document.export_runs(node.text_geometry.as_deref()));
                } else {
                    nodes.extend(
                        document.export_geometry_after(
                            node.text_geometry.as_deref(),
                            retained
                                .then(|| {
                                    previous
                                        .get(node.id)
                                        .and_then(|old| old.text_geometry.as_deref())
                                })
                                .flatten(),
                        ),
                    );
                }
            }
        }
        let mut tree = accesskit::Tree::new(accesskit::NodeId(self.root.0));
        tree.toolkit_name = toolkit_name.map(str::to_owned);
        tree.toolkit_version = toolkit_version.map(str::to_owned);
        let mut update = accesskit::TreeUpdate {
            nodes,
            tree: Some(tree),
            tree_id: accesskit::TreeId::ROOT,
            focus: accesskit::NodeId(
                self.focused_node()
                    .filter(|id| self.is_exported(*id))
                    .unwrap_or(self.root)
                    .0,
            ),
        };

        let base_index: HashMap<_, _> = update
            .nodes
            .iter()
            .enumerate()
            .map(|(index, (id, _))| (AccessibilityId(id.0), index))
            .collect();
        let mut outgoing: HashMap<AccessibilityId, HashSet<AccessibilityId>> = HashMap::new();
        let mut indegree: HashMap<AccessibilityId, usize> =
            base_index.keys().copied().map(|id| (id, 0)).collect();

        for id in base_index.keys() {
            let Some(node) = self.nodes.get(id) else {
                continue;
            };
            let Some(previous_node) = previous.nodes.get(id) else {
                continue;
            };
            if previous_node.parent == node.parent {
                continue;
            }

            let mut release_parent = previous_node.parent;
            while let Some(parent) = release_parent {
                if self.is_exported(parent) {
                    break;
                }
                release_parent = previous.nodes.get(&parent).and_then(|node| node.parent);
            }
            let (Some(release_parent), Some(claim_parent)) = (release_parent, node.parent) else {
                continue;
            };
            if release_parent == claim_parent
                || !base_index.contains_key(&release_parent)
                || !base_index.contains_key(&claim_parent)
            {
                continue;
            }
            if outgoing
                .entry(release_parent)
                .or_default()
                .insert(claim_parent)
            {
                *indegree.entry(claim_parent).or_default() += 1;
            }
        }

        if outgoing.is_empty() {
            return update;
        }

        let mut ready: Vec<_> = indegree
            .iter()
            .filter_map(|(id, degree)| (*degree == 0).then_some(*id))
            .collect();
        ready.sort_by_key(|id| std::cmp::Reverse(base_index[id]));
        let mut ordered_ids = Vec::with_capacity(update.nodes.len());
        while let Some(id) = ready.pop() {
            ordered_ids.push(id);
            if let Some(children) = outgoing.get(&id) {
                for child in children {
                    let degree = indegree
                        .get_mut(child)
                        .expect("transition node must have an indegree entry");
                    *degree -= 1;
                    if *degree == 0 {
                        ready.push(*child);
                        ready.sort_by_key(|id| std::cmp::Reverse(base_index[id]));
                    }
                }
            }
        }

        // A cyclic reparent (for example two containers swapping children) has
        // no safe single-update ordering. Keep the full parent-first order in
        // that rare case; platform providers may choose to reset their adapter.
        if ordered_ids.len() != update.nodes.len() {
            return update;
        }

        let mut nodes_by_id: HashMap<_, _> = update.nodes.drain(..).collect();
        update.nodes = ordered_ids
            .into_iter()
            .filter_map(|id| {
                nodes_by_id
                    .remove(&accesskit::NodeId(id.0))
                    .map(|node| (accesskit::NodeId(id.0), node))
            })
            .collect();
        update
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

impl AccessibilityAuditSeverity {
    /// Stable severity key for content-safe diagnostics.
    pub fn to_text(self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Warning => "warning",
        }
    }
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

impl AccessibilityAuditIssueKind {
    /// Stable issue kind key for content-safe diagnostics.
    pub fn to_text(self) -> &'static str {
        match self {
            Self::MissingRoot => "missing-root",
            Self::MissingRole => "missing-role",
            Self::UnknownRole => "unknown-role",
            Self::MissingAccessibleName => "missing-accessible-name",
            Self::MissingInteractiveAction => "missing-interactive-action",
            Self::InvalidRange => "invalid-range",
            Self::ConflictingStates => "conflicting-states",
            Self::HiddenFocusedNode => "hidden-focused-node",
            Self::MissingChildNode => "missing-child-node",
            Self::ParentMismatch => "parent-mismatch",
            Self::MultipleFocusedNodes => "multiple-focused-nodes",
        }
    }
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

    /// Byte length of the human-readable message without exposing it.
    pub fn message_len_bytes(&self) -> usize {
        self.message.len()
    }

    /// Content-safe issue summary.
    pub fn to_text(&self) -> String {
        format!(
            "accessibility_audit_issue(severity={}, kind={}, has_node_id={}, role={}, message_len_bytes={})",
            self.severity.to_text(),
            self.kind.to_text(),
            self.node_id.is_some(),
            self.role.map(AccessibilityRole::to_text).unwrap_or("none"),
            self.message_len_bytes()
        )
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

    /// Total number of audit issues.
    pub fn issue_count(&self) -> usize {
        self.issues.len()
    }

    /// Number of blocking audit errors.
    pub fn error_count(&self) -> usize {
        self.issues
            .iter()
            .filter(|issue| issue.severity == AccessibilityAuditSeverity::Error)
            .count()
    }

    /// Number of non-blocking audit warnings.
    pub fn warning_count(&self) -> usize {
        self.issues
            .iter()
            .filter(|issue| issue.severity == AccessibilityAuditSeverity::Warning)
            .count()
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
        self.error_count() == 0
    }

    /// Compact summary for logs, diagnostics, and agent output.
    pub fn summary(&self) -> String {
        let errors = self.error_count();
        let warnings = self.warning_count();
        match (errors, warnings) {
            (0, 0) => "accessibility audit passed".to_string(),
            (0, warnings) => format!("accessibility audit passed with {warnings} warning(s)"),
            (errors, warnings) => {
                format!("accessibility audit found {errors} error(s), {warnings} warning(s)")
            }
        }
    }

    /// Return a stable content-safe summary for logs and agent traces.
    pub fn to_text(&self) -> String {
        format!(
            "accessibility audit: {} issues, {} errors, {} warnings, ready {}",
            self.issue_count(),
            self.error_count(),
            self.warning_count(),
            self.is_ready()
        )
    }
}

/// Next action for a checked accessibility and automation handoff.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessibilityAutomationNextAction {
    /// Audit or export a native accessibility tree.
    AuditAccessibilityTree,
    /// Validate element attributes before rendering custom controls.
    ValidateAttributes,
    /// Route an accessibility action request to app-owned handlers.
    RouteActionRequest,
    /// Send an assistive-technology announcement.
    AnnounceStatus,
    /// Move accessibility focus to an existing visible node.
    FocusAccessibilityNode,
    /// Use hosted DOM accessibility or selector automation for an explicit WebView island.
    UseHostedDomAutomation,
}

impl AccessibilityAutomationNextAction {
    /// Stable key for logs, tests, and generated-agent routing.
    pub fn key(self) -> &'static str {
        match self {
            Self::AuditAccessibilityTree => "audit-accessibility-tree",
            Self::ValidateAttributes => "validate-attributes",
            Self::RouteActionRequest => "route-action-request",
            Self::AnnounceStatus => "announce-status",
            Self::FocusAccessibilityNode => "focus-accessibility-node",
            Self::UseHostedDomAutomation => "use-hosted-dom-automation",
        }
    }
}

/// Checked request inside an accessibility and automation handoff.
#[derive(Debug, Clone, PartialEq)]
pub enum AccessibilityAutomationRequest {
    /// Audit a native accessibility tree.
    Tree(AccessibilityTree),
    /// Validate accessibility attributes before attaching them to a custom element.
    Attributes(AccessibilityAttributes),
    /// Route a normalized accessibility action request.
    ActionRequest(AccessibilityActionRequest),
    /// Send a status announcement to assistive technology.
    Announcement {
        /// User-facing announcement text.
        message: String,
    },
    /// Focus an existing visible node in a native accessibility tree.
    FocusTarget {
        /// Tree that must contain the focus target.
        tree: AccessibilityTree,
        /// Node that should receive focus.
        node_id: AccessibilityId,
    },
    /// Use hosted DOM/selector automation for an explicit WebView island.
    HostedDomAutomation {
        /// Stable hosted surface id.
        surface_id: String,
    },
}

impl AccessibilityAutomationRequest {
    /// Return the next action implied by this request.
    pub fn next_action(&self) -> AccessibilityAutomationNextAction {
        match self {
            Self::Tree(_) => AccessibilityAutomationNextAction::AuditAccessibilityTree,
            Self::Attributes(_) => AccessibilityAutomationNextAction::ValidateAttributes,
            Self::ActionRequest(_) => AccessibilityAutomationNextAction::RouteActionRequest,
            Self::Announcement { .. } => AccessibilityAutomationNextAction::AnnounceStatus,
            Self::FocusTarget { .. } => AccessibilityAutomationNextAction::FocusAccessibilityNode,
            Self::HostedDomAutomation { .. } => {
                AccessibilityAutomationNextAction::UseHostedDomAutomation
            }
        }
    }

    /// Validate without mutating focus, routing actions, or invoking platform APIs.
    pub fn validate(&self) -> anyhow::Result<()> {
        match self {
            Self::Tree(tree) => {
                let report = tree.audit_report();
                anyhow::ensure!(
                    report.is_ready(),
                    "accessibility automation tree has {} audit error(s)",
                    report.error_count()
                );
                Ok(())
            }
            Self::Attributes(attributes) => attributes.validate(),
            Self::ActionRequest(request) => {
                anyhow::ensure!(
                    request.node_id.0 > 0,
                    "accessibility action request node id must be non-zero"
                );
                Ok(())
            }
            Self::Announcement { message } => {
                validate_accessibility_handoff_text(message, "accessibility announcement", 256)
            }
            Self::FocusTarget { tree, node_id } => {
                validate_accessibility_focus_target(tree, *node_id)
            }
            Self::HostedDomAutomation { surface_id } => validate_accessibility_handoff_id(
                surface_id,
                "hosted DOM automation surface id",
                128,
            ),
        }
    }
}

/// Checked handoff for native accessibility and automation readiness.
#[derive(Debug, Clone, PartialEq)]
pub struct AccessibilityAutomationHandoff {
    requests: Vec<AccessibilityAutomationRequest>,
    next_action: AccessibilityAutomationNextAction,
}

impl AccessibilityAutomationHandoff {
    /// Requests included in this handoff.
    pub fn requests(&self) -> &[AccessibilityAutomationRequest] {
        &self.requests
    }

    /// Number of checked accessibility requests.
    pub fn request_count(&self) -> usize {
        self.requests.len()
    }

    /// Next action builders or agents should take.
    pub fn next_action(&self) -> AccessibilityAutomationNextAction {
        self.next_action
    }

    /// Whether this handoff audits a native accessibility tree.
    pub fn audits_tree(&self) -> bool {
        self.requests
            .iter()
            .any(|request| matches!(request, AccessibilityAutomationRequest::Tree(_)))
    }

    /// Whether this handoff validates custom element attributes.
    pub fn validates_attributes(&self) -> bool {
        self.requests
            .iter()
            .any(|request| matches!(request, AccessibilityAutomationRequest::Attributes(_)))
    }

    /// Whether this handoff routes an action request.
    pub fn routes_action_request(&self) -> bool {
        self.requests
            .iter()
            .any(|request| matches!(request, AccessibilityAutomationRequest::ActionRequest(_)))
    }

    /// Whether this handoff sends an assistive-technology announcement.
    pub fn announces_status(&self) -> bool {
        self.requests
            .iter()
            .any(|request| matches!(request, AccessibilityAutomationRequest::Announcement { .. }))
    }

    /// Whether this handoff moves accessibility focus.
    pub fn focuses_node(&self) -> bool {
        self.requests
            .iter()
            .any(|request| matches!(request, AccessibilityAutomationRequest::FocusTarget { .. }))
    }

    /// Whether this handoff delegates to hosted DOM/selector automation.
    pub fn uses_hosted_dom_automation(&self) -> bool {
        self.requests.iter().any(|request| {
            matches!(
                request,
                AccessibilityAutomationRequest::HostedDomAutomation { .. }
            )
        })
    }

    /// Content-safe summary for tests, diagnostics, and agent traces.
    pub fn to_text(&self) -> String {
        format!(
            "accessibility automation handoff: {} requests, next action {}, tree {}, attributes {}, action {}, announcement {}, focus {}, hosted dom {}",
            self.request_count(),
            self.next_action.key(),
            self.audits_tree(),
            self.validates_attributes(),
            self.routes_action_request(),
            self.announces_status(),
            self.focuses_node(),
            self.uses_hosted_dom_automation()
        )
    }
}

/// Builder for checked accessibility and automation handoffs.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AccessibilityAutomationHandoffBuilder {
    requests: Vec<AccessibilityAutomationRequest>,
}

impl AccessibilityAutomationHandoffBuilder {
    /// Create an empty accessibility automation handoff builder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a native accessibility tree audit.
    pub fn tree(mut self, tree: AccessibilityTree) -> Self {
        self.requests
            .push(AccessibilityAutomationRequest::Tree(tree));
        self
    }

    /// Add custom accessibility attributes.
    pub fn attributes(mut self, attributes: AccessibilityAttributes) -> Self {
        self.requests
            .push(AccessibilityAutomationRequest::Attributes(attributes));
        self
    }

    /// Add a normalized accessibility action request.
    pub fn action_request(mut self, request: AccessibilityActionRequest) -> Self {
        self.requests
            .push(AccessibilityAutomationRequest::ActionRequest(request));
        self
    }

    /// Add an assistive-technology announcement.
    pub fn announcement(mut self, message: impl Into<String>) -> Self {
        self.requests
            .push(AccessibilityAutomationRequest::Announcement {
                message: message.into(),
            });
        self
    }

    /// Add an accessibility focus target.
    pub fn focus_target(mut self, tree: AccessibilityTree, node_id: AccessibilityId) -> Self {
        self.requests
            .push(AccessibilityAutomationRequest::FocusTarget { tree, node_id });
        self
    }

    /// Add a hosted DOM/selector automation fallback.
    pub fn hosted_dom_automation(mut self, surface_id: impl Into<String>) -> Self {
        self.requests
            .push(AccessibilityAutomationRequest::HostedDomAutomation {
                surface_id: surface_id.into(),
            });
        self
    }

    /// Validate without consuming this builder.
    pub fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            !self.requests.is_empty(),
            "accessibility automation handoff must include at least one request"
        );
        anyhow::ensure!(
            self.requests.len() <= 32,
            "accessibility automation handoff cannot include more than 32 requests"
        );
        for request in &self.requests {
            request.validate()?;
        }
        Ok(())
    }

    /// Build the checked accessibility automation handoff.
    pub fn build_checked(self) -> anyhow::Result<AccessibilityAutomationHandoff> {
        self.validate()?;
        let next_action = accessibility_automation_next_action(&self.requests);
        Ok(AccessibilityAutomationHandoff {
            requests: self.requests,
            next_action,
        })
    }
}

fn accessibility_automation_next_action(
    requests: &[AccessibilityAutomationRequest],
) -> AccessibilityAutomationNextAction {
    [
        AccessibilityAutomationNextAction::AuditAccessibilityTree,
        AccessibilityAutomationNextAction::ValidateAttributes,
        AccessibilityAutomationNextAction::RouteActionRequest,
        AccessibilityAutomationNextAction::AnnounceStatus,
        AccessibilityAutomationNextAction::FocusAccessibilityNode,
        AccessibilityAutomationNextAction::UseHostedDomAutomation,
    ]
    .into_iter()
    .find(|action| {
        requests
            .iter()
            .any(|request| request.next_action() == *action)
    })
    .unwrap_or(AccessibilityAutomationNextAction::AuditAccessibilityTree)
}

fn validate_accessibility_focus_target(
    tree: &AccessibilityTree,
    node_id: AccessibilityId,
) -> anyhow::Result<()> {
    anyhow::ensure!(
        node_id.0 > 0,
        "accessibility focus target node id must be non-zero"
    );
    let Some(node) = tree.get(node_id) else {
        anyhow::bail!(
            "accessibility focus target {} is not present in the tree",
            node_id.0
        );
    };
    anyhow::ensure!(
        !node.states.contains(AccessibilityState::HIDDEN),
        "accessibility focus target {} is hidden",
        node_id.0
    );
    Ok(())
}

fn validate_accessibility_handoff_text(
    value: &str,
    label: &str,
    max_chars: usize,
) -> anyhow::Result<()> {
    anyhow::ensure!(!value.trim().is_empty(), "{label} cannot be empty");
    anyhow::ensure!(
        value == value.trim(),
        "{label} cannot have leading or trailing whitespace"
    );
    anyhow::ensure!(
        !value.chars().any(char::is_control),
        "{label} cannot contain control characters"
    );
    anyhow::ensure!(
        value.chars().count() <= max_chars,
        "{label} cannot be longer than {max_chars} characters"
    );
    Ok(())
}

fn validate_accessibility_handoff_id(
    value: &str,
    label: &str,
    max_chars: usize,
) -> anyhow::Result<()> {
    validate_accessibility_handoff_text(value, label, max_chars)?;
    anyhow::ensure!(
        value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.')),
        "{label} can only contain ASCII letters, numbers, '.', '-' or '_'"
    );
    Ok(())
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
        if node.actions.is_empty() && !node.states.contains(AccessibilityState::DISABLED) {
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
    if states.contains(AccessibilityState::READ_ONLY) {
        node.set_read_only();
    }
}

fn apply_collection_metadata(node: &mut accesskit::Node, source: &AccessibilityNode) {
    if let Some(count) = source.row_count {
        node.set_row_count(count);
    }
    if let Some(count) = source.column_count {
        node.set_column_count(count);
    }
    if let Some(index) = source.row_index {
        node.set_row_index(index);
    }
    if let Some(index) = source.column_index {
        node.set_column_index(index);
    }
    if let Some(span) = source.row_span {
        node.set_row_span(span);
    }
    if let Some(span) = source.column_span {
        node.set_column_span(span);
    }
    if let Some(direction) = source.sort_direction {
        node.set_sort_direction(direction.to_accesskit());
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
    /// Stable semantic identity, independent of a virtual row's visual position.
    /// IDs must be unique within a window's accessibility tree.
    pub id: Option<AccessibilityId>,
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
    /// Hierarchical level for semantic headings and tree-like items.
    pub level: Option<usize>,
    /// Logical number of rows in a table or grid, including virtualized rows.
    pub row_count: Option<usize>,
    /// Logical number of columns in a table or grid, including virtualized columns.
    pub column_count: Option<usize>,
    /// One-based logical row index for a row or cell.
    pub row_index: Option<usize>,
    /// One-based logical column index for a cell or header.
    pub column_index: Option<usize>,
    /// Number of logical rows occupied by a cell.
    pub row_span: Option<usize>,
    /// Number of logical columns occupied by a cell.
    pub column_span: Option<usize>,
    /// Sort order for a column header.
    pub sort_direction: Option<AccessibilitySortDirection>,
    /// State flags.
    pub states: AccessibilityState,
    /// Actions available on this element.
    pub actions: Vec<AccessibilityAction>,
    /// Whether the element is hidden from assistive technology.
    pub hidden: bool,
    /// Active logical descendant of a focused container.
    pub active_descendant: Option<AccessibilityId>,
    /// Complete immutable native text runs prepared once per content revision.
    pub text_document: Option<std::sync::Arc<AccessibilityTextDocument>>,
    /// Directed UTF-8 selection in `text_document`.
    pub text_selection: Option<AccessibilityTextSelection>,
    /// Actual bounded text-run geometry for this viewport.
    pub text_geometry: Option<std::sync::Arc<AccessibilityTextGeometry>>,
}

impl AccessibilityAttributes {
    /// Use a stable model-owned identity for this semantic node.
    pub fn id(mut self, id: AccessibilityId) -> Self {
        self.id = Some(id);
        self
    }

    /// Identify the active descendant while focus remains on the container.
    pub fn active_descendant(mut self, id: AccessibilityId) -> Self {
        self.active_descendant = Some(id);
        self
    }
    /// Attach complete prepared native text and its directed UTF-8 selection.
    /// Selection endpoints must be selectable boundaries in the document.
    /// Keep the document Arc across caret redraws; replace it on content edits.
    pub fn text_document(
        mut self,
        document: std::sync::Arc<AccessibilityTextDocument>,
        selection: AccessibilityTextSelection,
    ) -> Self {
        self.text_document = Some(document);
        self.text_selection = Some(selection);
        self
    }
    /// Attach validated, bounded geometry for the prepared text document.
    pub fn text_geometry(mut self, geometry: std::sync::Arc<AccessibilityTextGeometry>) -> Self {
        self.text_geometry = Some(geometry);
        self
    }
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

    /// Set a semantic hierarchy level.
    pub fn level(mut self, level: usize) -> Self {
        self.level = Some(level);
        self
    }

    /// Set and clamp a semantic heading level to the supported 1–6 range.
    pub fn heading_level(self, level: usize) -> Self {
        self.level(level.clamp(1, 6))
    }

    /// Set the logical row count, including rows not currently mounted.
    pub fn row_count(mut self, count: usize) -> Self {
        self.row_count = Some(count);
        self
    }

    /// Set the logical column count, including columns not currently mounted.
    pub fn column_count(mut self, count: usize) -> Self {
        self.column_count = Some(count);
        self
    }

    /// Set a one-based logical row index.
    pub fn row_index(mut self, index: usize) -> Self {
        self.row_index = Some(index);
        self
    }

    /// Set a one-based logical column index.
    pub fn column_index(mut self, index: usize) -> Self {
        self.column_index = Some(index);
        self
    }

    /// Set the number of logical rows occupied by a cell.
    pub fn row_span(mut self, span: usize) -> Self {
        self.row_span = Some(span);
        self
    }

    /// Set the number of logical columns occupied by a cell.
    pub fn column_span(mut self, span: usize) -> Self {
        self.column_span = Some(span);
        self
    }

    /// Set the sort order exposed by a column header.
    pub fn sort_direction(mut self, direction: AccessibilitySortDirection) -> Self {
        self.sort_direction = Some(direction);
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

        if role == AccessibilityRole::Heading {
            anyhow::ensure!(
                self.label
                    .as_ref()
                    .is_some_and(|label| !label.trim().is_empty()),
                "heading accessibility role needs a label"
            );
            anyhow::ensure!(
                self.level.is_some_and(|level| (1..=6).contains(&level)),
                "heading accessibility role needs a level between 1 and 6"
            );
        }

        anyhow::ensure!(
            self.row_index.is_none_or(|index| index > 0),
            "accessibility row index must be one-based"
        );
        anyhow::ensure!(
            self.column_index.is_none_or(|index| index > 0),
            "accessibility column index must be one-based"
        );
        anyhow::ensure!(
            self.row_span.is_none_or(|span| span > 0),
            "accessibility row span must be positive"
        );
        anyhow::ensure!(
            self.column_span.is_none_or(|span| span > 0),
            "accessibility column span must be positive"
        );
        if let Some(selection) = self.text_selection {
            anyhow::ensure!(
                self.text_document
                    .as_ref()
                    .is_some_and(|document| document.contains_selection(selection)),
                "accessibility text selection must belong to the prepared document"
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
            id: self.id.unwrap_or(id),
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
            level: self.level,
            row_count: self.row_count,
            column_count: self.column_count,
            row_index: self.row_index,
            column_index: self.column_index,
            row_span: self.row_span,
            column_span: self.column_span,
            sort_direction: self.sort_direction,
            bounds: None,
            actions: self.actions.clone(),
            children: AccessibilityChildren::default(),
            parent: None,
            active_descendant: self.active_descendant,
            text_document: self.text_document.clone(),
            text_selection: self.text_selection,
            text_geometry: self.text_geometry.clone(),
        }
    }

    /// Byte length of the label without exposing label text.
    pub fn label_len_bytes(&self) -> usize {
        self.label.as_ref().map_or(0, |label| label.len())
    }

    /// Byte length of the description without exposing description text.
    pub fn description_len_bytes(&self) -> usize {
        self.description
            .as_ref()
            .map_or(0, |description| description.len())
    }

    /// Byte length of the placeholder without exposing placeholder text.
    pub fn placeholder_len_bytes(&self) -> usize {
        self.placeholder
            .as_ref()
            .map_or(0, |placeholder| placeholder.len())
    }

    /// Returns true when a label is configured.
    pub fn has_label(&self) -> bool {
        self.label.is_some()
    }

    /// Returns true when a description is configured.
    pub fn has_description(&self) -> bool {
        self.description.is_some()
    }

    /// Returns true when a value is configured.
    pub fn has_value(&self) -> bool {
        self.value.is_some()
    }

    /// Returns true when placeholder text is configured.
    pub fn has_placeholder(&self) -> bool {
        self.placeholder.is_some()
    }

    /// Number of configured accessibility actions.
    pub fn action_count(&self) -> usize {
        self.actions.len()
    }

    /// Content-safe attribute summary for diagnostics and generated checks.
    pub fn to_text(&self) -> String {
        let value_summary = self
            .value
            .as_ref()
            .map(AccessibilityValue::to_text)
            .unwrap_or_else(|| "none".to_string());
        let report = self.audit_report();
        format!(
            "accessibility_attributes(role={}, has_label={}, label_len_bytes={}, has_description={}, description_len_bytes={}, has_value={}, value={}, has_placeholder={}, placeholder_len_bytes={}, level={}, states={}, action_count={}, hidden={}, audit_errors={}, audit_warnings={}, ready={})",
            self.role.map(AccessibilityRole::to_text).unwrap_or("none"),
            self.has_label(),
            self.label_len_bytes(),
            self.has_description(),
            self.description_len_bytes(),
            self.has_value(),
            value_summary,
            self.has_placeholder(),
            self.placeholder_len_bytes(),
            self.level
                .map_or_else(|| "none".to_string(), |level| level.to_string()),
            self.states.to_text(),
            self.action_count(),
            self.hidden,
            report.error_count(),
            report.warning_count(),
            report.is_ready()
        )
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
    fn accessibility_ranges_check_offsets_without_allocating_nodes() {
        let counter = AtomicU64::new(7);
        assert!(reserve_accessibility_ids(&counter, 0).is_none());
        assert_eq!(counter.load(Ordering::Relaxed), 7);
        let range = reserve_accessibility_ids(&counter, 16_384_000_000).unwrap();
        assert_eq!(range.get(0), Some(AccessibilityId(7)));
        assert_eq!(
            range.get(range.len() - 1),
            Some(AccessibilityId(16_384_000_006))
        );
        assert!(range.get(range.len()).is_none());
        assert!(range.get(u64::MAX).is_none());
        assert!(!range.is_empty());
        let next = reserve_accessibility_ids(&counter, 1).unwrap();
        assert_eq!(next.get(0), Some(AccessibilityId(16_384_000_007)));
    }

    #[test]
    fn failed_accessibility_range_reservations_leave_counter_unchanged() {
        let counter = AtomicU64::new(u64::MAX - 2);
        assert!(reserve_accessibility_ids(&counter, 3).is_none());
        assert_eq!(counter.load(Ordering::Relaxed), u64::MAX - 2);
        let last = reserve_accessibility_ids(&counter, 2).unwrap();
        assert_eq!(last.get(1), Some(AccessibilityId(u64::MAX - 1)));
        assert_eq!(counter.load(Ordering::Relaxed), u64::MAX);
        assert!(reserve_accessibility_ids(&counter, 1).is_none());
        assert_eq!(counter.load(Ordering::Relaxed), u64::MAX);
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
    fn heading_attributes_preserve_a_valid_semantic_level() {
        let attributes = AccessibilityAttributes::new(AccessibilityRole::Heading)
            .label("Component settings")
            .heading_level(2);
        assert!(attributes.validate().is_ok());

        let node = attributes.to_node(AccessibilityId::new());
        assert_eq!(node.role, AccessibilityRole::Heading);
        assert_eq!(node.level, Some(2));
        let accesskit = node.to_accesskit_node();
        assert_eq!(accesskit.role(), accesskit::Role::Heading);
        assert_eq!(accesskit.level(), Some(2));

        assert!(
            AccessibilityAttributes::new(AccessibilityRole::Heading)
                .label("Invalid heading")
                .level(0)
                .validate()
                .is_err()
        );
        assert!(
            AccessibilityAttributes::new(AccessibilityRole::Heading)
                .heading_level(3)
                .validate()
                .is_err()
        );
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
        assert_eq!(report.issue_count(), 4);
        assert_eq!(report.error_count(), 4);
        assert_eq!(report.warning_count(), 0);
        assert_eq!(
            report.to_text(),
            "accessibility audit: 4 issues, 4 errors, 0 warnings, ready false"
        );
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
        assert_eq!(
            missing_role.to_text(),
            "accessibility audit: 1 issues, 1 errors, 0 warnings, ready false"
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
    fn accessibility_primitives_summary_is_content_safe() {
        let state = AccessibilityState::FOCUSED
            | AccessibilityState::CHECKED
            | AccessibilityState::REQUIRED;
        let payload = AccessibilityActionPayload::Value("private replacement text".to_string());
        let request = AccessibilityActionRequest::with_payload(
            AccessibilityId::new(),
            AccessibilityAction::SetValue,
            payload.clone(),
        );
        let text_value = AccessibilityValue::Text("private field value".to_string());
        let range_value = AccessibilityValue::Range {
            current: 5.0,
            min: 0.0,
            max: 10.0,
            step: Some(1.0),
        };
        let rect = AccessibilityRect::new(12.0, 24.0, 160.0, 40.0);

        assert_eq!(AccessibilityRole::TextInput.to_text(), "text-input");
        assert_eq!(AccessibilityAction::SetValue.to_text(), "set-value");
        assert_eq!(AccessibilityAction::Custom(42).to_text(), "custom");
        assert_eq!(AccessibilityAuditSeverity::Warning.to_text(), "warning");
        assert_eq!(
            AccessibilityAuditIssueKind::MissingAccessibleName.to_text(),
            "missing-accessible-name"
        );
        assert_eq!(state.enabled_count(), 3);
        assert!(state.to_text().contains("focused=true"));
        assert!(state.to_text().contains("required=true"));

        assert_eq!(payload.kind(), "value");
        assert_eq!(payload.value_len_bytes(), "private replacement text".len());
        assert!(!payload.to_text().contains("private replacement"));

        assert!(request.has_payload());
        assert_eq!(request.payload_kind(), "value");
        let request_summary = request.to_text();
        assert!(request_summary.contains("action=set-value"));
        assert!(!request_summary.contains("private replacement"));

        assert_eq!(text_value.kind(), "text");
        assert_eq!(text_value.text_len_bytes(), "private field value".len());
        assert!(text_value.is_finite_or_valid());
        assert!(range_value.has_step());
        assert!(range_value.to_text().contains("kind=range"));
        assert!(!text_value.to_text().contains("private field"));

        assert_eq!(rect.size_class(), "medium");
        assert!(rect.has_area());
        assert!(!rect.to_text().contains("160"));
        assert!(!rect.to_text().contains("12"));
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
    fn accessibility_node_tree_and_attributes_summary_is_content_safe() {
        let root = AccessibilityNode::new(AccessibilityRole::Window).with_label("Private App");
        let root_id = root.id;
        let mut tree = AccessibilityTree::new(root);

        let input = AccessibilityNode::new(AccessibilityRole::TextInput)
            .with_label("Secret Search")
            .with_description("Internal customer search field")
            .with_value(AccessibilityValue::Text("customer@example.com".to_string()))
            .with_bounds(AccessibilityRect::new(10.0, 20.0, 240.0, 32.0))
            .with_states(AccessibilityState::FOCUSED)
            .with_actions(vec![
                AccessibilityAction::Focus,
                AccessibilityAction::SetValue,
            ]);
        let input_id = input.id;
        tree.insert(input);
        tree.set_parent(input_id, root_id);

        let hidden = AccessibilityNode::new(AccessibilityRole::Button)
            .with_label("Hidden Delete")
            .with_actions(vec![AccessibilityAction::Click])
            .with_states(AccessibilityState::HIDDEN);
        let hidden_id = hidden.id;
        tree.insert(hidden);
        tree.set_parent(hidden_id, root_id);

        let input_summary = tree.get(input_id).unwrap().to_text();
        assert!(input_summary.contains("role=text-input"));
        assert!(input_summary.contains("has_label=true"));
        assert!(input_summary.contains("has_description=true"));
        assert!(input_summary.contains("has_value=true"));
        assert!(input_summary.contains("action_count=2"));
        assert!(input_summary.contains("has_bounds=true"));
        assert!(!input_summary.contains("Secret Search"));
        assert!(!input_summary.contains("customer@example"));
        assert!(!input_summary.contains("Internal customer"));
        assert!(!input_summary.contains("240"));

        assert_eq!(tree.node_count(), 3);
        assert_eq!(tree.interactive_node_count(), 2);
        assert_eq!(tree.actionable_node_count(), 2);
        assert_eq!(tree.hidden_node_count(), 1);
        assert_eq!(tree.focused_node_count(), 1);
        assert_eq!(tree.edge_count(), 2);
        let tree_summary = tree.to_text();
        assert!(tree_summary.contains("nodes=3"));
        assert!(tree_summary.contains("interactive_nodes=2"));
        assert!(tree_summary.contains("hidden_nodes=1"));
        assert!(!tree_summary.contains("Private App"));
        assert!(!tree_summary.contains("Hidden Delete"));

        let attrs = AccessibilityAttributes::text_input("Private Prompt", "secret draft")
            .description("Private description")
            .placeholder("Sensitive placeholder");
        let attrs_summary = attrs.to_text();
        assert!(attrs_summary.contains("role=text-input"));
        assert!(attrs_summary.contains("has_label=true"));
        assert!(attrs_summary.contains("has_value=true"));
        assert!(attrs_summary.contains("ready=true"));
        assert!(!attrs_summary.contains("Private Prompt"));
        assert!(!attrs_summary.contains("secret draft"));
        assert!(!attrs_summary.contains("Private description"));
        assert!(!attrs_summary.contains("Sensitive placeholder"));
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
        assert_eq!(report.issue_count(), 4);
        assert_eq!(report.error_count(), 3);
        assert_eq!(report.warning_count(), 1);
        assert_eq!(
            report.to_text(),
            "accessibility audit: 4 issues, 3 errors, 1 warnings, ready false"
        );
        assert!(!report.to_text().contains("Hidden"));
        assert!(!report.to_text().contains("Search"));
        assert!(!report.to_text().contains("Orphan"));
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

        let issue_summary = report.issues()[0].to_text();
        assert!(issue_summary.contains("accessibility_audit_issue("));
        assert!(issue_summary.contains("message_len_bytes="));
        assert!(!issue_summary.contains("hidden accessibility"));
        assert!(!issue_summary.contains("Hidden"));
    }

    #[test]
    fn accessibility_audit_accepts_named_disabled_controls_without_actions() {
        let root = AccessibilityNode::new(AccessibilityRole::Window);
        let mut tree = AccessibilityTree::new(root);
        let disabled = AccessibilityNode::new(AccessibilityRole::Button)
            .with_label("Unavailable action")
            .with_states(AccessibilityState::DISABLED);
        let disabled_id = disabled.id;
        tree.insert(disabled);
        tree.set_parent(disabled_id, tree.root);

        let report = tree.audit_report();
        assert!(report.is_ready(), "{}", report.to_text());
        assert_eq!(report.error_count(), 0);
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
    fn explicit_semantic_ids_and_active_descendants_survive_export() {
        let root = AccessibilityNode::new(AccessibilityRole::Window);
        let mut tree = AccessibilityTree::new(root);
        let container_id = AccessibilityId::new();
        let logical_id = AccessibilityId::new();
        let attrs = AccessibilityAttributes::new(AccessibilityRole::Tree)
            .id(container_id)
            .active_descendant(logical_id)
            .state(AccessibilityState::FOCUSED, true);
        let container = attrs.to_node(AccessibilityId::new());
        assert_eq!(container.id, container_id);
        assert_eq!(
            container.to_accesskit_node().active_descendant(),
            Some(accesskit::NodeId(logical_id.0))
        );
        tree.insert(container);
        tree.set_parent(container_id, tree.root);
        let mut logical =
            AccessibilityNode::new(AccessibilityRole::TreeItem).with_label("Offscreen document");
        logical.id = logical_id;
        tree.insert(logical);
        tree.set_parent(logical_id, container_id);
        let update = tree.to_accesskit_tree_update(None, None);
        let container = update
            .nodes
            .iter()
            .find(|(id, _)| *id == accesskit::NodeId(container_id.0))
            .unwrap();
        assert_eq!(
            container.1.active_descendant(),
            Some(accesskit::NodeId(logical_id.0))
        );
        assert_eq!(update.focus, accesskit::NodeId(container_id.0));
        tree.remove(logical_id);
        let update = tree.to_accesskit_tree_update(None, None);
        let container = update
            .nodes
            .iter()
            .find(|(id, _)| *id == accesskit::NodeId(container_id.0))
            .unwrap();
        assert_eq!(container.1.active_descendant(), None);
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
        assert_eq!(
            AccessibilityRole::Grid.to_accesskit(),
            accesskit::Role::Grid
        );
        assert_eq!(
            AccessibilityRole::ColumnHeader.to_accesskit(),
            accesskit::Role::ColumnHeader
        );
    }

    #[test]
    fn virtual_grid_metadata_reaches_accesskit() {
        let grid = AccessibilityNode::new(AccessibilityRole::Grid)
            .with_row_count(1_000_001)
            .with_column_count(16_384);
        let accesskit_grid = grid.to_accesskit_node();
        assert_eq!(accesskit_grid.row_count(), Some(1_000_001));
        assert_eq!(accesskit_grid.column_count(), Some(16_384));

        let header = AccessibilityNode::new(AccessibilityRole::ColumnHeader)
            .with_column_index(7)
            .with_sort_direction(AccessibilitySortDirection::Descending);
        let accesskit_header = header.to_accesskit_node();
        assert_eq!(accesskit_header.column_index(), Some(7));
        assert_eq!(
            accesskit_header.sort_direction(),
            Some(accesskit::SortDirection::Descending)
        );

        let cell = AccessibilityNode::new(AccessibilityRole::Cell)
            .with_row_index(999_999)
            .with_column_index(16_384)
            .with_row_span(2)
            .with_column_span(3);
        let accesskit_cell = cell.to_accesskit_node();
        assert_eq!(accesskit_cell.row_index(), Some(999_999));
        assert_eq!(accesskit_cell.column_index(), Some(16_384));
        assert_eq!(accesskit_cell.row_span(), Some(2));
        assert_eq!(accesskit_cell.column_span(), Some(3));
    }

    #[test]
    fn node_converts_role_label_and_children() {
        let mut node = AccessibilityNode::new(AccessibilityRole::Button).with_label("OK");
        node.children = vec![AccessibilityId(7)].into();
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
            .with_states(
                AccessibilityState::DISABLED
                    | AccessibilityState::SELECTED
                    | AccessibilityState::READ_ONLY,
            )
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
        assert!(ak.is_read_only());
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

        let update = tree.to_accesskit_tree_update(Some("Kael"), Some("0.0.0"));
        let ak_tree = update.tree.expect("tree present");
        assert_eq!(ak_tree.root, accesskit::NodeId(root_id.0));
        assert_eq!(update.tree_id, accesskit::TreeId::ROOT);
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
    fn transition_update_releases_reparented_nodes_before_claiming_them() {
        let root = AccessibilityNode::new(AccessibilityRole::Window);
        let root_id = root.id;
        let first_parent = AccessibilityNode::new(AccessibilityRole::Group);
        let first_parent_id = first_parent.id;
        let second_parent = AccessibilityNode::new(AccessibilityRole::Group);
        let second_parent_id = second_parent.id;
        let child = AccessibilityNode::new(AccessibilityRole::Button);
        let child_id = child.id;

        let mut previous = AccessibilityTree::new(root.clone());
        previous.insert(second_parent.clone());
        previous.set_parent(second_parent_id, root_id);
        previous.insert(first_parent.clone());
        previous.set_parent(first_parent_id, root_id);
        previous.insert(child.clone());
        previous.set_parent(child_id, first_parent_id);

        // Keep the current root order as second, then first. A plain BFS update
        // would let the new parent claim the child before the old parent
        // releases it, which can orphan the node inside AccessKit.
        let mut current = AccessibilityTree::new(root);
        current.insert(second_parent);
        current.set_parent(second_parent_id, root_id);
        current.insert(first_parent);
        current.set_parent(first_parent_id, root_id);
        current.insert(child);
        current.set_parent(child_id, second_parent_id);

        let update = current.to_accesskit_tree_update_after(Some(&previous), Some("Kael"), None);
        let position = |id: AccessibilityId| {
            update
                .nodes
                .iter()
                .position(|(candidate, _)| *candidate == accesskit::NodeId(id.0))
                .expect("node should be present")
        };
        assert!(position(first_parent_id) < position(second_parent_id));
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

        let update = tree.to_accesskit_tree_update(None, None);
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
    fn native_actions_reject_disabled_and_hidden_nodes_even_when_advertised() {
        for action in [
            AccessibilityAction::Click,
            AccessibilityAction::Focus,
            AccessibilityAction::ScrollUp,
            AccessibilityAction::ScrollDown,
            AccessibilityAction::ScrollToVisible,
            AccessibilityAction::Expand,
            AccessibilityAction::Collapse,
            AccessibilityAction::Toggle,
            AccessibilityAction::Increment,
            AccessibilityAction::Decrement,
            AccessibilityAction::SetValue,
            AccessibilityAction::ShowMenu,
            AccessibilityAction::Dismiss,
        ] {
            let mut node =
                AccessibilityNode::new(AccessibilityRole::Button).with_actions(vec![action]);
            for state in [AccessibilityState::DISABLED, AccessibilityState::HIDDEN] {
                node.states = state;
                assert!(
                    AccessibilityActionRequest::from_accesskit_for_node(
                        node.id,
                        &node,
                        action.to_accesskit(),
                    )
                    .is_none(),
                    "{action:?} must reject {state:?} even while its action remains advertised"
                );
            }
            node.states = AccessibilityState::NONE;
            assert_eq!(
                AccessibilityActionRequest::from_accesskit_for_node(
                    node.id,
                    &node,
                    action.to_accesskit(),
                )
                .unwrap()
                .action,
                action,
                "re-enabled actions retain their native semantics"
            );
        }
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
    fn action_router_prunes_handlers_for_nodes_outside_the_active_tree() {
        let retained = AccessibilityId::new();
        let removed = AccessibilityId::new();
        let mut router = AccessibilityActionRouter::new();
        router.on_action(retained, AccessibilityAction::Click, |_| {});
        router.on_action(removed, AccessibilityAction::Focus, |_| {});

        router.retain_nodes([retained]);

        assert!(router.has_handler(retained, AccessibilityAction::Click));
        assert!(!router.has_handler(removed, AccessibilityAction::Focus));
        assert_eq!(router.handler_count(), 1);
    }

    #[test]
    fn painted_action_handlers_retire_without_removing_logical_actions() {
        use std::{cell::Cell, rc::Rc, sync::Arc};

        let window = AccessibilityNode::new(AccessibilityRole::Window);
        let mut root = AccessibilityNode::new(AccessibilityRole::Tree);
        let root_id = root.id;
        let mut row = AccessibilityNode::new(AccessibilityRole::TreeItem)
            .with_actions(vec![AccessibilityAction::Click]);
        let row_id = row.id;
        row.parent = Some(root_id);
        root.children.push(row_id);
        let snapshot = AccessibilitySnapshot::new(root_id, [root, row.clone()]).unwrap();
        let mut tree = AccessibilityTree::new(window);
        tree.nodes.attach(snapshot.clone());
        tree.set_parent(root_id, tree.root);
        tree.nodes.insert(row_id, row.clone());

        let seen = Rc::new(Cell::new(0));
        let captured = Arc::new(vec![0_u8; 1024]);
        let retired = Arc::downgrade(&captured);
        let mut router = AccessibilityActionRouter::new();
        let handler_seen = seen.clone();
        router.on_frame_action(row_id, AccessibilityAction::Click, move |_| {
            handler_seen.set(captured.len());
        });
        router.retain_tree(&tree.clone());
        assert!(router.dispatch(AccessibilityActionRequest::new(
            row_id,
            AccessibilityAction::Click,
        )));
        assert_eq!(seen.get(), 1024, "cached painted overlays retain handlers");
        tree.nodes.frame.remove(&row_id);
        router.retain_tree(&tree);
        assert!(tree.get(row_id).is_some(), "logical row remains available");
        assert_eq!(router.handler_count(), 0);
        assert!(
            retired.upgrade().is_none(),
            "outgoing captures are released"
        );

        let logical_seen = seen.clone();
        router.on_action(row_id, AccessibilityAction::Click, move |_| {
            logical_seen.set(2);
        });
        router.retain_tree(&tree);
        assert!(router.has_handler(row_id, AccessibilityAction::Click));
        assert_eq!(
            router.handler_count(),
            1,
            "explicit logical handlers persist"
        );

        tree.nodes.insert(row_id, row);
        router.on_frame_action(row_id, AccessibilityAction::Click, |_| {
            panic!("mounted pointer handler must not override logical routing")
        });
        let subtree_seen = seen.clone();
        router.on_subtree(snapshot, move |_| subtree_seen.set(3));
        router.retain_tree(&tree);
        assert_eq!(router.handler_count(), 0, "shadowed captures are released");
        tree.nodes.frame.remove(&row_id);
        router.retain_tree(&tree);
        assert!(router.dispatch(AccessibilityActionRequest::new(
            row_id,
            AccessibilityAction::Click,
        )));
        assert_eq!(seen.get(), 3, "offscreen action reaches current subtree");
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

        let update = tree.to_accesskit_tree_update(Some("Kael"), None);
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

    #[test]
    fn accessibility_automation_handoff_validates_native_actionability() {
        let root = AccessibilityNode::new(AccessibilityRole::Window);
        let root_id = root.id;
        let mut tree = AccessibilityTree::new(root);
        let button = AccessibilityNode::new(AccessibilityRole::Button)
            .with_label("Run audit")
            .with_actions(vec![AccessibilityAction::Focus, AccessibilityAction::Click])
            .with_states(AccessibilityState::FOCUSED);
        let button_id = button.id;
        tree.insert(button);
        tree.set_parent(button_id, root_id);

        let handoff = AccessibilityAutomationHandoffBuilder::new()
            .tree(tree.clone())
            .attributes(AccessibilityAttributes::button("Run audit"))
            .action_request(AccessibilityActionRequest::new(
                button_id,
                AccessibilityAction::Click,
            ))
            .announcement("Audit complete")
            .focus_target(tree, button_id)
            .hosted_dom_automation("hosted-preview")
            .build_checked()
            .unwrap();

        assert_eq!(handoff.request_count(), 6);
        assert_eq!(
            handoff.next_action(),
            AccessibilityAutomationNextAction::AuditAccessibilityTree
        );
        assert!(handoff.audits_tree());
        assert!(handoff.validates_attributes());
        assert!(handoff.routes_action_request());
        assert!(handoff.announces_status());
        assert!(handoff.focuses_node());
        assert!(handoff.uses_hosted_dom_automation());
        assert_eq!(
            AccessibilityAutomationNextAction::UseHostedDomAutomation.key(),
            "use-hosted-dom-automation"
        );
        assert_eq!(
            handoff.to_text(),
            "accessibility automation handoff: 6 requests, next action audit-accessibility-tree, tree true, attributes true, action true, announcement true, focus true, hosted dom true"
        );

        let hosted = AccessibilityAutomationHandoffBuilder::new()
            .hosted_dom_automation("hosted-editor")
            .build_checked()
            .unwrap();
        assert_eq!(
            hosted.next_action(),
            AccessibilityAutomationNextAction::UseHostedDomAutomation
        );
    }

    #[test]
    fn accessibility_automation_handoff_rejects_invalid_generated_inputs() {
        let root = AccessibilityNode::new(AccessibilityRole::Window);
        let root_id = root.id;
        let mut tree = AccessibilityTree::new(root);
        let hidden = AccessibilityNode::new(AccessibilityRole::Button)
            .with_label("Hidden")
            .with_actions(vec![AccessibilityAction::Click])
            .with_states(AccessibilityState::HIDDEN);
        let hidden_id = hidden.id;
        tree.insert(hidden);
        tree.set_parent(hidden_id, root_id);

        assert!(
            AccessibilityAutomationHandoffBuilder::new()
                .attributes(AccessibilityAttributes::new(AccessibilityRole::Button))
                .build_checked()
                .is_err()
        );
        assert!(
            AccessibilityAutomationHandoffBuilder::new()
                .announcement(" missing trim ")
                .build_checked()
                .is_err()
        );
        assert!(
            AccessibilityAutomationHandoffBuilder::new()
                .hosted_dom_automation("bad surface")
                .build_checked()
                .is_err()
        );
        assert!(
            AccessibilityAutomationHandoffBuilder::new()
                .action_request(AccessibilityActionRequest::new(
                    AccessibilityId(0),
                    AccessibilityAction::Click,
                ))
                .build_checked()
                .is_err()
        );
        assert!(
            AccessibilityAutomationHandoffBuilder::new()
                .focus_target(tree.clone(), hidden_id)
                .build_checked()
                .is_err()
        );

        let mut invalid_tree =
            AccessibilityTree::new(AccessibilityNode::new(AccessibilityRole::Window));
        invalid_tree.root = AccessibilityId(999_999);
        assert!(
            AccessibilityAutomationHandoffBuilder::new()
                .tree(invalid_tree)
                .build_checked()
                .is_err()
        );
        assert!(
            AccessibilityAutomationHandoffBuilder::new()
                .build_checked()
                .is_err()
        );
    }
}
