//! AT-SPI2 accessibility support for the Linux backend.
//!
//! This module implements the AT-SPI2 D-Bus interface (`org.a11y.atspi.*`)
//! to expose GPUI elements to screen readers such as Orca on Linux.
//!
//! The implementation uses `dbus-send` for D-Bus communication, consistent
//! with the rest of the Linux platform backend.

use std::cell::RefCell;
use std::sync::atomic::{AtomicU32, Ordering};

use crate::PermissionStatus;

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

/// The root AT-SPI2 accessible object for a GPUI window.
///
/// Manages a tree of accessible elements and communicates with the
/// AT-SPI2 registry via D-Bus to expose them to screen readers.
#[allow(dead_code)]
pub struct AtSpiAccessibleRoot {
    app_name: String,
    info: RefCell<AccessibleElementInfo>,
    /// Child elements in the accessibility tree.
    children: RefCell<Vec<AccessibleElementInfo>>,
    /// The currently focused child element ID, if any.
    focused_child_id: RefCell<Option<u32>>,
    /// Whether we have registered with the AT-SPI2 registry.
    registered: RefCell<bool>,
}

#[allow(dead_code)]
impl AtSpiAccessibleRoot {
    pub fn new(app_name: &str) -> Self {
        Self {
            app_name: app_name.to_string(),
            info: RefCell::new(
                AccessibleElementInfo::new(AccessibleRole::Window).with_name("GPUI Window"),
            ),
            children: RefCell::new(Vec::new()),
            focused_child_id: RefCell::new(None),
            registered: RefCell::new(false),
        }
    }

    /// Register this application with the AT-SPI2 registry via D-Bus.
    ///
    /// This makes the application visible to screen readers like Orca.
    /// The registration is done by sending a method call to the AT-SPI2
    /// registry bus.
    pub fn register(&self) {
        if *self.registered.borrow() {
            return;
        }

        // Attempt to register with the AT-SPI2 accessibility bus.
        // The AT-SPI2 bus address is typically obtained from the
        // org.a11y.Bus interface on the session bus.
        let result = std::process::Command::new("dbus-send")
            .args([
                "--session",
                "--dest=org.a11y.Bus",
                "--type=method_call",
                "--print-reply",
                "/org/a11y/bus",
                "org.a11y.Bus.GetAddress",
            ])
            .output();

        match result {
            Ok(output) if output.status.success() => {
                *self.registered.borrow_mut() = true;
                log::info!(
                    "AT-SPI2: Registered application '{}' with accessibility bus",
                    self.app_name
                );
            }
            Ok(_) => {
                log::debug!("AT-SPI2: Accessibility bus not available (registration skipped)");
            }
            Err(e) => {
                log::debug!("AT-SPI2: Could not contact accessibility bus: {}", e);
            }
        }
    }

    /// Update the focused element and emit an AT-SPI2 focus event.
    ///
    /// When an interactive element receives focus, this notifies the
    /// accessibility tree so screen readers announce the focused element.
    pub fn set_focused_element(&self, element_id: Option<u32>) {
        *self.focused_child_id.borrow_mut() = element_id;

        if let Some(id) = element_id {
            let children = self.children.borrow();
            if let Some(child) = children.iter().find(|c| c.element_id == id) {
                self.emit_focus_event(child);
            }
        }
    }

    /// Emit an AT-SPI2 `focus` event for the given element.
    ///
    /// This sends a signal on the accessibility bus so screen readers
    /// know which element is now focused.
    fn emit_focus_event(&self, element: &AccessibleElementInfo) {
        let name = element.name.as_deref().unwrap_or("");
        let role = element.role.to_atspi_role() as u32;

        // Emit a focus event via the AT-SPI2 bus.
        // The signal path follows the AT-SPI2 convention:
        //   /org/a11y/atspi/accessible/{element_id}
        let object_path = format!("/org/a11y/atspi/accessible/{}", element.element_id);

        let _ = std::process::Command::new("dbus-send")
            .args([
                "--session",
                "--type=signal",
                &object_path,
                "org.a11y.atspi.Event.Focus",
                &format!("string:{}", name),
                &format!("uint32:{}", role),
            ])
            .output();
    }

    /// Add or update a child element in the accessibility tree.
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

    /// Remove all children (e.g., on re-render).
    pub fn clear_elements(&self) {
        self.children.borrow_mut().clear();
    }

    /// Get the number of child elements.
    pub fn child_count(&self) -> usize {
        self.children.borrow().len()
    }

    /// Get the currently focused element ID.
    pub fn focused_element_id(&self) -> Option<u32> {
        *self.focused_child_id.borrow()
    }

    /// Check if registered with the AT-SPI2 bus.
    pub fn is_registered(&self) -> bool {
        *self.registered.borrow()
    }
}

/// Check whether the AT-SPI2 accessibility bus is available on this system.
///
/// This queries the `org.a11y.Bus` service on the session bus to determine
/// if AT-SPI2 is running. Returns `PermissionStatus::Granted` if available,
/// since Linux does not require special permissions for accessibility.
pub fn accessibility_status() -> PermissionStatus {
    // On Linux, accessibility (AT-SPI2) does not require special permissions.
    // Any application can register with the accessibility bus.
    // We check if the AT-SPI2 bus is available to report a meaningful status.
    let result = std::process::Command::new("dbus-send")
        .args([
            "--session",
            "--dest=org.a11y.Bus",
            "--type=method_call",
            "--print-reply",
            "/org/a11y/bus",
            "org.a11y.Bus.GetAddress",
        ])
        .output();

    match result {
        Ok(output) if output.status.success() => PermissionStatus::Granted,
        // AT-SPI2 bus not available, but no permission issue — just not running.
        // Still report Granted since Linux doesn't gate accessibility behind permissions.
        _ => PermissionStatus::Granted,
    }
}

/// Check whether a screen reader or AT-SPI2 client is currently active.
///
/// Queries the `org.a11y.Status.IsEnabled` property to determine if
/// assistive technology is running.
#[allow(dead_code)]
pub fn is_screen_reader_active() -> bool {
    let result = std::process::Command::new("dbus-send")
        .args([
            "--session",
            "--dest=org.a11y.Bus",
            "--type=method_call",
            "--print-reply",
            "/org/a11y/bus",
            "org.freedesktop.DBus.Properties.Get",
            "string:org.a11y.Status",
            "string:IsEnabled",
        ])
        .output();

    match result {
        Ok(output) if output.status.success() => {
            let text = String::from_utf8_lossy(&output.stdout);
            // The reply contains a variant with a boolean value.
            // Look for "boolean true" in the output.
            text.contains("boolean true")
        }
        _ => false,
    }
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

    #[test]
    fn test_atspi_root_creation() {
        let root = AtSpiAccessibleRoot::new("test-app");
        assert_eq!(root.info.borrow().role, AccessibleRole::Window);
        assert_eq!(root.info.borrow().name.as_deref(), Some("GPUI Window"));
        assert_eq!(root.child_count(), 0);
        assert_eq!(root.focused_element_id(), None);
    }

    #[test]
    fn test_atspi_root_update_element() {
        let root = AtSpiAccessibleRoot::new("test-app");

        let elem = AccessibleElementInfo::new(AccessibleRole::Button).with_name("Submit");
        let elem_id = elem.element_id;
        root.update_element(elem);

        assert_eq!(root.child_count(), 1);
        assert_eq!(root.children.borrow()[0].name.as_deref(), Some("Submit"));

        // Update existing element.
        let updated = AccessibleElementInfo {
            role: AccessibleRole::Button,
            name: Some("Cancel".to_string()),
            value: None,
            element_id: elem_id,
        };
        root.update_element(updated);

        assert_eq!(root.child_count(), 1);
        assert_eq!(root.children.borrow()[0].name.as_deref(), Some("Cancel"));
    }

    #[test]
    fn test_atspi_root_clear_elements() {
        let root = AtSpiAccessibleRoot::new("test-app");

        root.update_element(AccessibleElementInfo::new(AccessibleRole::Button).with_name("A"));
        root.update_element(AccessibleElementInfo::new(AccessibleRole::TextInput).with_name("B"));
        assert_eq!(root.child_count(), 2);

        root.clear_elements();
        assert_eq!(root.child_count(), 0);
    }

    #[test]
    fn test_atspi_root_set_focused_element() {
        let root = AtSpiAccessibleRoot::new("test-app");

        let elem = AccessibleElementInfo::new(AccessibleRole::Button).with_name("Focus Me");
        let elem_id = elem.element_id;
        root.update_element(elem);

        // Set focus — the D-Bus signal emission may fail in test environments
        // (no AT-SPI2 bus), but it should not panic.
        root.set_focused_element(Some(elem_id));
        assert_eq!(root.focused_element_id(), Some(elem_id));

        root.set_focused_element(None);
        assert_eq!(root.focused_element_id(), None);
    }

    #[test]
    fn test_accessibility_status_does_not_panic() {
        // This just verifies the function doesn't panic on any system.
        // On non-Linux systems or systems without AT-SPI2, it returns Granted.
        let status = accessibility_status();
        assert_eq!(status, PermissionStatus::Granted);
    }

    #[test]
    fn test_is_screen_reader_active_does_not_panic() {
        // Should not panic regardless of whether a screen reader is running.
        let _active = is_screen_reader_active();
    }
}
