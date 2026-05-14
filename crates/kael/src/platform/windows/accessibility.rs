#![allow(non_upper_case_globals)]

use std::cell::RefCell;
use std::sync::atomic::{AtomicU32, Ordering};

use windows::Win32::Foundation::*;
use windows::Win32::System::Com::*;
use windows::Win32::System::Ole::*;
use windows::Win32::System::Variant::*;
use windows::Win32::UI::Accessibility::*;
use windows::Win32::UI::WindowsAndMessaging::{GetWindowRect, SetForegroundWindow};
use windows::core::*;

/// Roles that GPUI elements can expose to the accessibility tree.
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
    /// Map a GPUI accessible role to a UIA control type ID.
    pub fn to_uia_control_type(&self) -> UIA_CONTROLTYPE_ID {
        match self {
            AccessibleRole::Window => UIA_WindowControlTypeId,
            AccessibleRole::Button => UIA_ButtonControlTypeId,
            AccessibleRole::TextInput => UIA_EditControlTypeId,
            AccessibleRole::StaticText => UIA_TextControlTypeId,
            AccessibleRole::Group => UIA_GroupControlTypeId,
            AccessibleRole::List => UIA_ListControlTypeId,
            AccessibleRole::ListItem => UIA_ListItemControlTypeId,
            AccessibleRole::ScrollBar => UIA_ScrollBarControlTypeId,
            AccessibleRole::Image => UIA_ImageControlTypeId,
            AccessibleRole::Link => UIA_HyperlinkControlTypeId,
            AccessibleRole::Menu => UIA_MenuControlTypeId,
            AccessibleRole::MenuItem => UIA_MenuItemControlTypeId,
            AccessibleRole::Tab => UIA_TabItemControlTypeId,
            AccessibleRole::TabPanel => UIA_TabControlTypeId,
            AccessibleRole::Toolbar => UIA_ToolBarControlTypeId,
            AccessibleRole::TreeItem => UIA_TreeItemControlTypeId,
            AccessibleRole::CheckBox => UIA_CheckBoxControlTypeId,
            AccessibleRole::RadioButton => UIA_RadioButtonControlTypeId,
            AccessibleRole::Slider => UIA_SliderControlTypeId,
            AccessibleRole::ProgressBar => UIA_ProgressBarControlTypeId,
            AccessibleRole::Separator => UIA_SeparatorControlTypeId,
            AccessibleRole::Pane => UIA_PaneControlTypeId,
            AccessibleRole::Unknown => UIA_CustomControlTypeId,
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

/// The root UIA provider for a GPUI window. Implements `IRawElementProviderSimple`
/// and `IRawElementProviderFragment` to expose the window to screen readers.
#[implement(
    IRawElementProviderSimple,
    IRawElementProviderFragment,
    IRawElementProviderFragmentRoot
)]
pub struct GpuiUiaProvider {
    hwnd: HWND,
    pub(crate) info: RefCell<AccessibleElementInfo>,
    pub(crate) children: RefCell<Vec<ComObject<GpuiElementProvider>>>,
    pub(crate) focused_child_id: RefCell<Option<u32>>,
}

impl GpuiUiaProvider {
    pub fn new(hwnd: HWND) -> ComObject<Self> {
        ComObject::new(Self {
            hwnd,
            info: RefCell::new(
                AccessibleElementInfo::new(AccessibleRole::Window).with_name("GPUI Window"),
            ),
            children: RefCell::new(Vec::new()),
            focused_child_id: RefCell::new(None),
        })
    }

    /// Update the focused element and fire UIA focus changed event.
    pub fn set_focused_element(&self, element_id: Option<u32>) {
        *self.focused_child_id.borrow_mut() = element_id;

        #[cfg(not(test))]
        if let Some(id) = element_id {
            let children = self.children.borrow();
            if let Some(child) = children.iter().find(|c| c.info.borrow().element_id == id) {
                let provider: IRawElementProviderSimple = child.to_interface();
                unsafe {
                    let _ = UiaRaiseAutomationEvent(&provider, UIA_AutomationFocusChangedEventId);
                }
            }
        }
    }

    /// Add or update a child element in the accessibility tree.
    pub fn update_element(&self, info: AccessibleElementInfo) {
        let mut children = self.children.borrow_mut();
        if let Some(existing) = children
            .iter()
            .find(|c| c.info.borrow().element_id == info.element_id)
        {
            *existing.info.borrow_mut() = info;
        } else {
            children.push(GpuiElementProvider::new(self.hwnd, info));
        }
    }

    /// Remove all children (e.g., on re-render).
    pub fn clear_elements(&self) {
        self.children.borrow_mut().clear();
    }
}

impl IRawElementProviderSimple_Impl for GpuiUiaProvider_Impl {
    fn ProviderOptions(&self) -> Result<ProviderOptions> {
        Ok(ProviderOptions_ServerSideProvider)
    }

    fn GetPatternProvider(&self, _pattern_id: UIA_PATTERN_ID) -> Result<IUnknown> {
        Err(Error::empty())
    }

    fn GetPropertyValue(&self, property_id: UIA_PROPERTY_ID) -> Result<VARIANT> {
        let info = self.info.borrow();
        match property_id {
            UIA_ControlTypePropertyId => {
                let ct = info.role.to_uia_control_type();
                Ok(VARIANT::from(ct.0))
            }
            UIA_NamePropertyId => {
                if let Some(ref name) = info.name {
                    Ok(VARIANT::from(BSTR::from(name.as_str())))
                } else {
                    Err(Error::empty())
                }
            }
            UIA_IsKeyboardFocusablePropertyId => Ok(VARIANT::from(true)),
            UIA_IsContentElementPropertyId => Ok(VARIANT::from(true)),
            UIA_IsControlElementPropertyId => Ok(VARIANT::from(true)),
            UIA_NativeWindowHandlePropertyId => Ok(VARIANT::from(self.hwnd.0 as i64)),
            _ => Err(Error::empty()),
        }
    }

    fn HostRawElementProvider(&self) -> Result<IRawElementProviderSimple> {
        #[cfg(not(test))]
        return unsafe { UiaHostProviderFromHwnd(self.hwnd) };
        #[cfg(test)]
        Err(Error::empty())
    }
}

impl IRawElementProviderFragment_Impl for GpuiUiaProvider_Impl {
    fn Navigate(&self, direction: NavigateDirection) -> Result<IRawElementProviderFragment> {
        match direction {
            NavigateDirection_FirstChild => {
                let children = self.children.borrow();
                children
                    .first()
                    .map(|c| c.to_interface())
                    .ok_or_else(Error::empty)
            }
            NavigateDirection_LastChild => {
                let children = self.children.borrow();
                children
                    .last()
                    .map(|c| c.to_interface())
                    .ok_or_else(Error::empty)
            }
            _ => Err(Error::empty()),
        }
    }

    fn GetRuntimeId(&self) -> Result<*mut SAFEARRAY> {
        let info = self.info.borrow();
        let runtime_id: [i32; 2] = [UiaAppendRuntimeId as i32, info.element_id as i32];
        unsafe {
            let sa = SafeArrayCreateVector(VT_I4, 0, 2);
            if sa.is_null() {
                return Err(Error::from(E_OUTOFMEMORY));
            }
            for (i, val) in runtime_id.iter().enumerate() {
                SafeArrayPutElement(sa, &(i as i32), val as *const i32 as *const _)?;
            }
            Ok(sa)
        }
    }

    fn BoundingRectangle(&self) -> Result<UiaRect> {
        let mut rect = RECT::default();
        unsafe { GetWindowRect(self.hwnd, &mut rect)? };
        Ok(UiaRect {
            left: rect.left as f64,
            top: rect.top as f64,
            width: (rect.right - rect.left) as f64,
            height: (rect.bottom - rect.top) as f64,
        })
    }

    fn GetEmbeddedFragmentRoots(&self) -> Result<*mut SAFEARRAY> {
        Err(Error::empty())
    }

    fn SetFocus(&self) -> Result<()> {
        unsafe {
            let _ = SetForegroundWindow(self.hwnd);
        }
        Ok(())
    }

    fn FragmentRoot(&self) -> Result<IRawElementProviderFragmentRoot> {
        // The root provider is itself the fragment root.
        Err(Error::empty())
    }
}

impl IRawElementProviderFragmentRoot_Impl for GpuiUiaProvider_Impl {
    fn ElementProviderFromPoint(&self, _x: f64, _y: f64) -> Result<IRawElementProviderFragment> {
        // For now, return the root element. Hit-testing into children can be added later.
        Err(Error::empty())
    }

    fn GetFocus(&self) -> Result<IRawElementProviderFragment> {
        let focused_id = self.focused_child_id.borrow();
        if let Some(id) = *focused_id {
            let children = self.children.borrow();
            if let Some(child) = children.iter().find(|c| c.info.borrow().element_id == id) {
                return Ok(child.to_interface());
            }
        }
        Err(Error::empty())
    }
}

/// UIA provider for individual GPUI elements (children of the root window provider).
#[implement(IRawElementProviderSimple, IRawElementProviderFragment)]
pub struct GpuiElementProvider {
    #[allow(dead_code)]
    hwnd: HWND,
    pub(crate) info: RefCell<AccessibleElementInfo>,
}

impl GpuiElementProvider {
    fn new(hwnd: HWND, info: AccessibleElementInfo) -> ComObject<Self> {
        ComObject::new(Self {
            hwnd,
            info: RefCell::new(info),
        })
    }
}

impl IRawElementProviderSimple_Impl for GpuiElementProvider_Impl {
    fn ProviderOptions(&self) -> Result<ProviderOptions> {
        Ok(ProviderOptions_ServerSideProvider)
    }

    fn GetPatternProvider(&self, _pattern_id: UIA_PATTERN_ID) -> Result<IUnknown> {
        Err(Error::empty())
    }

    fn GetPropertyValue(&self, property_id: UIA_PROPERTY_ID) -> Result<VARIANT> {
        let info = self.info.borrow();
        match property_id {
            UIA_ControlTypePropertyId => {
                let ct = info.role.to_uia_control_type();
                Ok(VARIANT::from(ct.0))
            }
            UIA_NamePropertyId => {
                if let Some(ref name) = info.name {
                    Ok(VARIANT::from(BSTR::from(name.as_str())))
                } else {
                    Err(Error::empty())
                }
            }
            UIA_ValueValuePropertyId => {
                if let Some(ref value) = info.value {
                    Ok(VARIANT::from(BSTR::from(value.as_str())))
                } else {
                    Err(Error::empty())
                }
            }
            UIA_IsKeyboardFocusablePropertyId => Ok(VARIANT::from(true)),
            UIA_IsContentElementPropertyId => Ok(VARIANT::from(true)),
            UIA_IsControlElementPropertyId => Ok(VARIANT::from(true)),
            UIA_AutomationIdPropertyId => Ok(VARIANT::from(BSTR::from(format!(
                "gpui-element-{}",
                info.element_id
            )))),
            _ => Err(Error::empty()),
        }
    }

    fn HostRawElementProvider(&self) -> Result<IRawElementProviderSimple> {
        Err(Error::empty())
    }
}

impl IRawElementProviderFragment_Impl for GpuiElementProvider_Impl {
    fn Navigate(&self, _direction: NavigateDirection) -> Result<IRawElementProviderFragment> {
        // Sibling/parent navigation can be added later for full tree traversal.
        Err(Error::empty())
    }

    fn GetRuntimeId(&self) -> Result<*mut SAFEARRAY> {
        let info = self.info.borrow();
        let runtime_id: [i32; 2] = [UiaAppendRuntimeId as i32, info.element_id as i32];
        unsafe {
            let sa = SafeArrayCreateVector(VT_I4, 0, 2);
            if sa.is_null() {
                return Err(Error::from(E_OUTOFMEMORY));
            }
            for (i, val) in runtime_id.iter().enumerate() {
                SafeArrayPutElement(sa, &(i as i32), val as *const i32 as *const _)?;
            }
            Ok(sa)
        }
    }

    fn BoundingRectangle(&self) -> Result<UiaRect> {
        // Individual element bounds would come from the GPUI layout system.
        // For now, return an empty rect; this will be populated when elements
        // report their bounds during layout.
        Ok(UiaRect {
            left: 0.0,
            top: 0.0,
            width: 0.0,
            height: 0.0,
        })
    }

    fn GetEmbeddedFragmentRoots(&self) -> Result<*mut SAFEARRAY> {
        Err(Error::empty())
    }

    fn SetFocus(&self) -> Result<()> {
        Ok(())
    }

    fn FragmentRoot(&self) -> Result<IRawElementProviderFragmentRoot> {
        Err(Error::empty())
    }
}

/// Handle `WM_GETOBJECT` message to return the UIA provider for the window.
/// Returns `Some(lresult)` if handled, `None` to pass to `DefWindowProc`.
/// Handle `WM_GETOBJECT` message to return the UIA provider for the window.
/// Returns `Some(lresult)` if handled, `None` to pass to `DefWindowProc`.
pub fn handle_wm_getobject(
    hwnd: HWND,
    wparam: WPARAM,
    lparam: LPARAM,
    provider: &ComObject<GpuiUiaProvider>,
) -> Option<isize> {
    #[cfg(not(test))]
    {
        let objid = lparam.0 as i32;
        if objid == UiaRootObjectId {
            let provider_simple: IRawElementProviderSimple = provider.to_interface();
            let result = unsafe {
                UiaReturnRawElementProvider(hwnd, wparam, LPARAM(lparam.0), &provider_simple)
            };
            return Some(result.0);
        }
    }
    #[cfg(test)]
    let _ = (hwnd, wparam, lparam, provider);
    None
}

/// Check whether UIA is running (i.e., a screen reader or automation tool is active).
#[allow(dead_code)]
pub fn is_uia_running() -> bool {
    #[cfg(not(test))]
    return unsafe { UiaClientsAreListening().as_bool() };
    #[cfg(test)]
    false
}
