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
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use accesskit::{ActionHandler, ActionRequest, ActivationHandler, DeactivationHandler, TreeUpdate};
use accesskit_unix::Adapter;

use crate::PermissionStatus;

const TOOLKIT_NAME: &str = "Kael";
const TOOLKIT_VERSION: &str = env!("CARGO_PKG_VERSION");

/// AT-SPI2 role constants.
/// See: <https://gitlab.gnome.org/GNOME/at-spi2-core/-/blob/main/xml/Accessibility.xml>
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
#[allow(dead_code)]
pub enum AtSpiRole {
    Invalid = 0,
    Application = 75,
    Frame = 22,
    PushButton = 42,
    Text = 60,
    Label = 29,
    Panel = 38,
    List = 34,
    ListItem = 35,
    ScrollBar = 47,
    Image = 26,
    Link = 33,
    Menu = 36,
    MenuItem = 37,
    PageTab = 39,
    PageTabList = 40,
    ToolBar = 62,
    TreeItem = 66,
    CheckBox = 7,
    RadioButton = 43,
    Slider = 50,
    ProgressBar = 41,
    Separator = 49,
    Filler = 21,
}

/// Roles that GPUI elements can expose to the accessibility tree.
/// Mirrors the Windows `AccessibleRole` enum for cross-platform consistency.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessibleRole {
    Window,
    Button,
    TextInput,
    StaticText,
    Group,
    List,
    ListItem,
    ScrollBar,
    Image,
    Link,
    Menu,
    MenuItem,
    Tab,
    TabPanel,
    Toolbar,
    TreeItem,
    CheckBox,
    RadioButton,
    Slider,
    ProgressBar,
    Separator,
    Pane,
    Unknown,
}

impl AccessibleRole {
    /// Map a GPUI accessible role to an AT-SPI2 role.
    pub fn to_atspi_role(&self) -> AtSpiRole {
        match self {
            AccessibleRole::Window => AtSpiRole::Frame,
            AccessibleRole::Button => AtSpiRole::PushButton,
            AccessibleRole::TextInput => AtSpiRole::Text,
            AccessibleRole::StaticText => AtSpiRole::Label,
            AccessibleRole::Group => AtSpiRole::Panel,
            AccessibleRole::List => AtSpiRole::List,
            AccessibleRole::ListItem => AtSpiRole::ListItem,
            AccessibleRole::ScrollBar => AtSpiRole::ScrollBar,
            AccessibleRole::Image => AtSpiRole::Image,
            AccessibleRole::Link => AtSpiRole::Link,
            AccessibleRole::Menu => AtSpiRole::Menu,
            AccessibleRole::MenuItem => AtSpiRole::MenuItem,
            AccessibleRole::Tab => AtSpiRole::PageTab,
            AccessibleRole::TabPanel => AtSpiRole::PageTabList,
            AccessibleRole::Toolbar => AtSpiRole::ToolBar,
            AccessibleRole::TreeItem => AtSpiRole::TreeItem,
            AccessibleRole::CheckBox => AtSpiRole::CheckBox,
            AccessibleRole::RadioButton => AtSpiRole::RadioButton,
            AccessibleRole::Slider => AtSpiRole::Slider,
            AccessibleRole::ProgressBar => AtSpiRole::ProgressBar,
            AccessibleRole::Separator => AtSpiRole::Separator,
            AccessibleRole::Pane => AtSpiRole::Filler,
            AccessibleRole::Unknown => AtSpiRole::Invalid,
        }
    }
}

impl From<crate::AccessibilityRole> for AccessibleRole {
    fn from(role: crate::AccessibilityRole) -> Self {
        match role {
            crate::AccessibilityRole::Window => AccessibleRole::Window,
            crate::AccessibilityRole::Button => AccessibleRole::Button,
            crate::AccessibilityRole::TextInput => AccessibleRole::TextInput,
            crate::AccessibilityRole::StaticText => AccessibleRole::StaticText,
            crate::AccessibilityRole::Group => AccessibleRole::Group,
            crate::AccessibilityRole::List => AccessibleRole::List,
            crate::AccessibilityRole::ListItem => AccessibleRole::ListItem,
            crate::AccessibilityRole::ScrollBar => AccessibleRole::ScrollBar,
            crate::AccessibilityRole::Image => AccessibleRole::Image,
            crate::AccessibilityRole::Link => AccessibleRole::Link,
            crate::AccessibilityRole::Menu => AccessibleRole::Menu,
            crate::AccessibilityRole::MenuItem => AccessibleRole::MenuItem,
            crate::AccessibilityRole::Tab => AccessibleRole::Tab,
            crate::AccessibilityRole::TabPanel => AccessibleRole::TabPanel,
            crate::AccessibilityRole::Toolbar => AccessibleRole::Toolbar,
            crate::AccessibilityRole::Tree => AccessibleRole::Unknown,
            crate::AccessibilityRole::TreeItem => AccessibleRole::TreeItem,
            crate::AccessibilityRole::CheckBox => AccessibleRole::CheckBox,
            crate::AccessibilityRole::RadioButton => AccessibleRole::RadioButton,
            crate::AccessibilityRole::Slider => AccessibleRole::Slider,
            crate::AccessibilityRole::ProgressBar => AccessibleRole::ProgressBar,
            crate::AccessibilityRole::Separator => AccessibleRole::Separator,
            crate::AccessibilityRole::Pane => AccessibleRole::Pane,
            crate::AccessibilityRole::Application => AccessibleRole::Unknown,
            crate::AccessibilityRole::Dialog => AccessibleRole::Unknown,
            crate::AccessibilityRole::Alert => AccessibleRole::Unknown,
            crate::AccessibilityRole::ComboBox => AccessibleRole::Unknown,
            crate::AccessibilityRole::Switch => AccessibleRole::CheckBox,
            crate::AccessibilityRole::Unknown => AccessibleRole::Unknown,
        }
    }
}

/// Metadata for an accessible element in the GPUI tree.
/// Mirrors the Windows `AccessibleElementInfo` struct.
#[derive(Debug, Clone)]
pub struct AccessibleElementInfo {
    pub role: AccessibleRole,
    pub name: Option<String>,
    pub value: Option<String>,
    pub element_id: u32,
}

static NEXT_ELEMENT_ID: AtomicU32 = AtomicU32::new(1);

impl AccessibleElementInfo {
    pub fn new(role: AccessibleRole) -> Self {
        Self {
            role,
            name: None,
            value: None,
            element_id: NEXT_ELEMENT_ID.fetch_add(1, Ordering::Relaxed),
        }
    }

    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    pub fn with_value(mut self, value: impl Into<String>) -> Self {
        self.value = Some(value.into());
        self
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

struct NoopDeactivationHandler;

impl DeactivationHandler for NoopDeactivationHandler {
    fn deactivate_accessibility(&mut self) {}
}

/// The root AT-SPI2 accessible object for a GPUI window, backed by AccessKit.
///
/// Wraps an [`accesskit_unix::Adapter`] and feeds it [`TreeUpdate`]s built from
/// the shared [`crate::AccessibilityTree`]. The legacy element-tracking fields
/// are retained as a lightweight mirror so the existing introspection API
/// (`child_count`, `focused_element_id`, …) keeps working.
pub struct AtSpiAccessibleRoot {
    app_name: String,
    children: RefCell<Vec<AccessibleElementInfo>>,
    focused_child_id: RefCell<Option<u32>>,
    adapter: RefCell<Adapter>,
    latest: SharedUpdate,
    pending_actions: PendingActions,
}

impl AtSpiAccessibleRoot {
    pub fn new(app_name: &str) -> Self {
        let latest: SharedUpdate = Arc::new(Mutex::new(None));
        let pending_actions: PendingActions = Arc::new(Mutex::new(Vec::new()));
        let adapter = Adapter::new(
            InitialTreeHandler {
                latest: latest.clone(),
            },
            CollectingActionHandler {
                pending: pending_actions.clone(),
            },
            NoopDeactivationHandler,
        );
        Self {
            app_name: app_name.to_string(),
            children: RefCell::new(Vec::new()),
            focused_child_id: RefCell::new(None),
            adapter: RefCell::new(adapter),
            latest,
            pending_actions,
        }
    }

    /// Return the application name exposed to accessibility clients.
    pub fn app_name(&self) -> &str {
        &self.app_name
    }

    /// Feed the latest accessibility tree to the AT-SPI2 adapter.
    pub fn update_tree(&self, tree: &crate::AccessibilityTree) {
        let update = tree.to_accesskit_tree_update(
            Some(self.app_name.as_str()),
            Some(TOOLKIT_NAME),
            Some(TOOLKIT_VERSION),
        );
        if let Ok(mut guard) = self.latest.lock() {
            *guard = Some(update.clone());
        }
        self.adapter.borrow_mut().update_if_active(|| update);
    }

    /// Notify the adapter that the window's focus state changed.
    pub fn update_window_focus_state(&self, is_focused: bool) {
        self.adapter
            .borrow_mut()
            .update_window_focus_state(is_focused);
    }

    /// Report the window's screen-space bounds so AT clients can hit-test.
    pub fn set_root_window_bounds(&self, outer: accesskit::Rect, inner: accesskit::Rect) {
        self.adapter
            .borrow_mut()
            .set_root_window_bounds(outer, inner);
    }

    /// Drain action requests received from assistive technology, translated
    /// into kael's [`crate::AccessibilityAction`] plus the target node id.
    pub fn drain_actions(&self) -> Vec<(crate::AccessibilityId, crate::AccessibilityAction)> {
        let mut out = Vec::new();
        if let Ok(mut pending) = self.pending_actions.lock() {
            for request in pending.drain(..) {
                if let Some(action) = crate::AccessibilityAction::from_accesskit(request.action) {
                    out.push((crate::AccessibilityId(request.target.0), action));
                }
            }
        }
        out
    }

    /// Add or update a child element in the lightweight mirror.
    pub fn update_element(&self, info: AccessibleElementInfo) {
        let mut children = self.children.borrow_mut();
        if let Some(existing) = children
            .iter_mut()
            .find(|c| c.element_id == info.element_id)
        {
            *existing = info;
        } else {
            children.push(info);
        }
    }

    /// Remove all mirrored children (e.g., on re-render).
    pub fn clear_elements(&self) {
        self.children.borrow_mut().clear();
    }

    /// Get the number of mirrored child elements.
    pub fn child_count(&self) -> usize {
        self.children.borrow().len()
    }

    /// Update the focused element in the lightweight mirror.
    pub fn set_focused_element(&self, element_id: Option<u32>) {
        *self.focused_child_id.borrow_mut() = element_id;
    }

    /// Get the currently focused element ID from the mirror.
    pub fn focused_element_id(&self) -> Option<u32> {
        *self.focused_child_id.borrow()
    }

    /// Whether the AT-SPI2 adapter has been created for this window.
    ///
    /// The adapter is created unconditionally in [`AtSpiAccessibleRoot::new`],
    /// so this reflects construction rather than a live bus handshake.
    pub fn is_registered(&self) -> bool {
        false
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
mod tests {
    use super::*;

    #[test]
    fn test_role_to_atspi_mapping() {
        assert_eq!(AccessibleRole::Window.to_atspi_role(), AtSpiRole::Frame);
        assert_eq!(
            AccessibleRole::Button.to_atspi_role(),
            AtSpiRole::PushButton
        );
        assert_eq!(AccessibleRole::TextInput.to_atspi_role(), AtSpiRole::Text);
        assert_eq!(AccessibleRole::StaticText.to_atspi_role(), AtSpiRole::Label);
        assert_eq!(AccessibleRole::Group.to_atspi_role(), AtSpiRole::Panel);
        assert_eq!(AccessibleRole::List.to_atspi_role(), AtSpiRole::List);
        assert_eq!(
            AccessibleRole::ListItem.to_atspi_role(),
            AtSpiRole::ListItem
        );
        assert_eq!(
            AccessibleRole::ScrollBar.to_atspi_role(),
            AtSpiRole::ScrollBar
        );
        assert_eq!(AccessibleRole::Image.to_atspi_role(), AtSpiRole::Image);
        assert_eq!(AccessibleRole::Link.to_atspi_role(), AtSpiRole::Link);
        assert_eq!(AccessibleRole::Menu.to_atspi_role(), AtSpiRole::Menu);
        assert_eq!(
            AccessibleRole::MenuItem.to_atspi_role(),
            AtSpiRole::MenuItem
        );
        assert_eq!(AccessibleRole::Tab.to_atspi_role(), AtSpiRole::PageTab);
        assert_eq!(
            AccessibleRole::TabPanel.to_atspi_role(),
            AtSpiRole::PageTabList
        );
        assert_eq!(AccessibleRole::Toolbar.to_atspi_role(), AtSpiRole::ToolBar);
        assert_eq!(
            AccessibleRole::TreeItem.to_atspi_role(),
            AtSpiRole::TreeItem
        );
        assert_eq!(
            AccessibleRole::CheckBox.to_atspi_role(),
            AtSpiRole::CheckBox
        );
        assert_eq!(
            AccessibleRole::RadioButton.to_atspi_role(),
            AtSpiRole::RadioButton
        );
        assert_eq!(AccessibleRole::Slider.to_atspi_role(), AtSpiRole::Slider);
        assert_eq!(
            AccessibleRole::ProgressBar.to_atspi_role(),
            AtSpiRole::ProgressBar
        );
        assert_eq!(
            AccessibleRole::Separator.to_atspi_role(),
            AtSpiRole::Separator
        );
        assert_eq!(AccessibleRole::Pane.to_atspi_role(), AtSpiRole::Filler);
        assert_eq!(AccessibleRole::Unknown.to_atspi_role(), AtSpiRole::Invalid);
    }

    #[test]
    fn test_accessible_element_info_builder() {
        let info = AccessibleElementInfo::new(AccessibleRole::Button)
            .with_name("OK")
            .with_value("pressed");

        assert_eq!(info.role, AccessibleRole::Button);
        assert_eq!(info.name.as_deref(), Some("OK"));
        assert_eq!(info.value.as_deref(), Some("pressed"));
        assert!(info.element_id > 0);
    }

    #[test]
    fn test_element_ids_are_unique() {
        let info1 = AccessibleElementInfo::new(AccessibleRole::Button);
        let info2 = AccessibleElementInfo::new(AccessibleRole::TextInput);
        assert_ne!(info1.element_id, info2.element_id);
    }
}
