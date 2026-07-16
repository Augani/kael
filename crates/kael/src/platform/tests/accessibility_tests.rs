//! Unit tests for the Windows UIA accessibility provider.
//!
//! These tests validate the AccessibleRole → UIA control type mapping
//! and the AccessibleElementInfo builder. The UIA COM integration
//! tests require a Windows environment and are gated behind
//! `#[cfg(target_os = "windows")]`.

#[cfg(target_os = "windows")]
mod windows_tests {
    use crate::platform::windows::accessibility::*;
    use windows::Win32::UI::Accessibility::*;

    #[test]
    fn test_role_to_uia_control_type_mapping() {
        assert_eq!(
            AccessibleRole::Window.to_uia_control_type(),
            UIA_WindowControlTypeId
        );
        assert_eq!(
            AccessibleRole::Button.to_uia_control_type(),
            UIA_ButtonControlTypeId
        );
        assert_eq!(
            AccessibleRole::TextInput.to_uia_control_type(),
            UIA_EditControlTypeId
        );
        assert_eq!(
            AccessibleRole::StaticText.to_uia_control_type(),
            UIA_TextControlTypeId
        );
        assert_eq!(
            AccessibleRole::Group.to_uia_control_type(),
            UIA_GroupControlTypeId
        );
        assert_eq!(
            AccessibleRole::List.to_uia_control_type(),
            UIA_ListControlTypeId
        );
        assert_eq!(
            AccessibleRole::ListItem.to_uia_control_type(),
            UIA_ListItemControlTypeId
        );
        assert_eq!(
            AccessibleRole::ScrollBar.to_uia_control_type(),
            UIA_ScrollBarControlTypeId
        );
        assert_eq!(
            AccessibleRole::Image.to_uia_control_type(),
            UIA_ImageControlTypeId
        );
        assert_eq!(
            AccessibleRole::Link.to_uia_control_type(),
            UIA_HyperlinkControlTypeId
        );
        assert_eq!(
            AccessibleRole::Menu.to_uia_control_type(),
            UIA_MenuControlTypeId
        );
        assert_eq!(
            AccessibleRole::MenuItem.to_uia_control_type(),
            UIA_MenuItemControlTypeId
        );
        assert_eq!(
            AccessibleRole::Tab.to_uia_control_type(),
            UIA_TabItemControlTypeId
        );
        assert_eq!(
            AccessibleRole::TabPanel.to_uia_control_type(),
            UIA_TabControlTypeId
        );
        assert_eq!(
            AccessibleRole::Toolbar.to_uia_control_type(),
            UIA_ToolBarControlTypeId
        );
        assert_eq!(
            AccessibleRole::TreeItem.to_uia_control_type(),
            UIA_TreeItemControlTypeId
        );
        assert_eq!(
            AccessibleRole::CheckBox.to_uia_control_type(),
            UIA_CheckBoxControlTypeId
        );
        assert_eq!(
            AccessibleRole::RadioButton.to_uia_control_type(),
            UIA_RadioButtonControlTypeId
        );
        assert_eq!(
            AccessibleRole::Slider.to_uia_control_type(),
            UIA_SliderControlTypeId
        );
        assert_eq!(
            AccessibleRole::ProgressBar.to_uia_control_type(),
            UIA_ProgressBarControlTypeId
        );
        assert_eq!(
            AccessibleRole::Separator.to_uia_control_type(),
            UIA_SeparatorControlTypeId
        );
        assert_eq!(
            AccessibleRole::Pane.to_uia_control_type(),
            UIA_PaneControlTypeId
        );
        assert_eq!(
            AccessibleRole::Unknown.to_uia_control_type(),
            UIA_CustomControlTypeId
        );
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
    fn test_uia_provider_creation() {
        use windows::Win32::Foundation::HWND;

        let hwnd = HWND(std::ptr::null_mut());
        let provider = GpuiUiaProvider::new(hwnd);

        assert_eq!(provider.info.borrow().role, AccessibleRole::Window);
        assert_eq!(provider.info.borrow().name.as_deref(), Some("GPUI Window"));
        assert!(provider.children.borrow().is_empty());
    }

    #[test]
    fn test_uia_provider_update_element() {
        use windows::Win32::Foundation::HWND;

        let hwnd = HWND(std::ptr::null_mut());
        let provider = GpuiUiaProvider::new(hwnd);

        let elem = AccessibleElementInfo::new(AccessibleRole::Button).with_name("Submit");
        let elem_id = elem.element_id;
        provider.update_element(elem);

        assert_eq!(provider.children.borrow().len(), 1);
        assert_eq!(
            provider.children.borrow()[0].info.borrow().name.as_deref(),
            Some("Submit")
        );

        let mut updated = AccessibleElementInfo::new(AccessibleRole::Button).with_name("Cancel");
        updated.element_id = elem_id;
        provider.update_element(updated);

        assert_eq!(provider.children.borrow().len(), 1);
        assert_eq!(
            provider.children.borrow()[0].info.borrow().name.as_deref(),
            Some("Cancel")
        );
    }

    #[test]
    fn test_uia_provider_clear_elements() {
        use windows::Win32::Foundation::HWND;

        let hwnd = HWND(std::ptr::null_mut());
        let provider = GpuiUiaProvider::new(hwnd);

        provider.update_element(AccessibleElementInfo::new(AccessibleRole::Button).with_name("A"));
        provider
            .update_element(AccessibleElementInfo::new(AccessibleRole::TextInput).with_name("B"));
        assert_eq!(provider.children.borrow().len(), 2);

        provider.clear_elements();
        assert!(provider.children.borrow().is_empty());
    }

    #[test]
    fn test_uia_provider_set_focused_element() {
        use windows::Win32::Foundation::HWND;

        let hwnd = HWND(std::ptr::null_mut());
        let provider = GpuiUiaProvider::new(hwnd);

        let elem = AccessibleElementInfo::new(AccessibleRole::Button).with_name("Focus Me");
        let elem_id = elem.element_id;
        provider.update_element(elem);

        provider.set_focused_element(Some(elem_id));
        assert_eq!(*provider.focused_child_id.borrow(), Some(elem_id));

        provider.set_focused_element(None);
        assert_eq!(*provider.focused_child_id.borrow(), None);
    }
}

#[cfg(target_os = "windows")]
mod tree_tests {
    use crate::{AccessibilityNode, AccessibilityRole, AccessibilityState, AccessibilityTree};

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
}

#[test]
fn test_test_window_receives_accessibility_tree() {
    use crate::platform::test::{TestDisplay, TestPlatform, TestWindow};
    use crate::{
        AccessibilityNode, AccessibilityRole, AccessibilityTree, BackgroundExecutor, Bounds,
        EmptyView, ForegroundExecutor, Point, Size, TestDispatcher, WindowHandle, WindowId,
        WindowKind, WindowParams,
    };
    use rand::{SeedableRng, rngs::StdRng};
    use std::rc::Rc;
    use std::sync::Arc;

    let dispatcher = Arc::new(TestDispatcher::new(StdRng::seed_from_u64(0)));
    let background_executor = BackgroundExecutor::new(dispatcher.clone());
    let foreground_executor = ForegroundExecutor::new(dispatcher);
    let platform = TestPlatform::new(background_executor, foreground_executor);
    let id: WindowId = slotmap::KeyData::from_ffi(1).into();
    let handle = WindowHandle::<EmptyView>::new(id).into();
    let window = TestWindow::new(
        handle,
        WindowParams {
            bounds: Bounds::new(Point::default(), Size::default()),
            titlebar: None,
            kind: WindowKind::Normal,
            is_movable: true,
            is_resizable: true,
            is_minimizable: true,
            display_id: None,
            window_min_size: None,
            #[cfg(target_os = "macos")]
            tabbing_identifier: None,
            focus: true,
            show: true,
            mouse_passthrough: false,
            parent: None,
        },
        Rc::downgrade(&platform),
        Rc::new(TestDisplay::new()),
    );

    let mut tree = AccessibilityTree::new(AccessibilityNode::new(AccessibilityRole::Window));
    let button = AccessibilityNode::new(AccessibilityRole::Button).with_label("Submit");
    let button_id = button.id;
    tree.insert(button);
    tree.set_parent(button_id, tree.root);

    let mut window_ref = window;
    crate::PlatformWindow::update_accessibility_tree(&mut window_ref, &tree);

    let state = window_ref.0.lock();
    let stored_tree = state.accessibility_tree.as_ref().unwrap();
    assert_eq!(stored_tree.nodes.len(), 2);
    assert_eq!(
        stored_tree.get(button_id).unwrap().label.as_deref(),
        Some("Submit")
    );
}
