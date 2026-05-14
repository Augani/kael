//! Unit tests for the Linux AT-SPI2 accessibility provider.
//!
//! These tests validate the AccessibleRole → AT-SPI2 role mapping,
//! the AccessibleElementInfo builder, and the AtSpiAccessibleRoot
//! tree management. The D-Bus integration tests require a Linux
//! environment with AT-SPI2 and are gated behind `#[cfg(target_os = "linux")]`.

#[cfg(target_os = "linux")]
mod linux_tests {
    use crate::PermissionStatus;
    use crate::platform::linux::accessibility::*;

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
        assert_eq!(root.child_count(), 0);
        assert_eq!(root.focused_element_id(), None);
        assert!(!root.is_registered());
    }

    #[test]
    fn test_atspi_root_update_element() {
        let root = AtSpiAccessibleRoot::new("test-app");

        let elem = AccessibleElementInfo::new(AccessibleRole::Button).with_name("Submit");
        let elem_id = elem.element_id;
        root.update_element(elem);

        assert_eq!(root.child_count(), 1);

        // Update existing element.
        let updated = AccessibleElementInfo {
            role: AccessibleRole::Button,
            name: Some("Cancel".to_string()),
            value: None,
            element_id: elem_id,
        };
        root.update_element(updated);
        assert_eq!(root.child_count(), 1);
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

        root.set_focused_element(Some(elem_id));
        assert_eq!(root.focused_element_id(), Some(elem_id));

        root.set_focused_element(None);
        assert_eq!(root.focused_element_id(), None);
    }

    #[test]
    fn test_accessibility_status_returns_granted() {
        // On Linux, accessibility doesn't require special permissions.
        let status = accessibility_status();
        assert_eq!(status, PermissionStatus::Granted);
    }
}
