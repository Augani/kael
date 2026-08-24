#![allow(non_upper_case_globals)]

use std::cell::RefCell;
use std::rc::Rc;
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
    Heading,
    Group,
    List,
    ListItem,
    Table,
    Grid,
    Row,
    Cell,
    ColumnHeader,
    RowHeader,
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
            AccessibleRole::Heading => UIA_TextControlTypeId,
            AccessibleRole::Group => UIA_GroupControlTypeId,
            AccessibleRole::List => UIA_ListControlTypeId,
            AccessibleRole::ListItem => UIA_ListItemControlTypeId,
            AccessibleRole::Table | AccessibleRole::Grid => UIA_DataGridControlTypeId,
            AccessibleRole::Row => UIA_DataItemControlTypeId,
            AccessibleRole::Cell => UIA_DataItemControlTypeId,
            AccessibleRole::ColumnHeader | AccessibleRole::RowHeader => UIA_HeaderItemControlTypeId,
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
            crate::AccessibilityRole::Heading => AccessibleRole::Heading,
            crate::AccessibilityRole::Group => AccessibleRole::Group,
            crate::AccessibilityRole::List => AccessibleRole::List,
            crate::AccessibilityRole::ListItem => AccessibleRole::ListItem,
            crate::AccessibilityRole::Table => AccessibleRole::Table,
            crate::AccessibilityRole::Grid => AccessibleRole::Grid,
            crate::AccessibilityRole::Row => AccessibleRole::Row,
            crate::AccessibilityRole::Cell => AccessibleRole::Cell,
            crate::AccessibilityRole::ColumnHeader => AccessibleRole::ColumnHeader,
            crate::AccessibilityRole::RowHeader => AccessibleRole::RowHeader,
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
    pub node_id: crate::AccessibilityId,
    pub actions: Vec<crate::AccessibilityAction>,
    pub toggle_value: Option<bool>,
    pub range_value: Option<AccessibleRangeValue>,
    pub text_value: Option<String>,
}

/// Numeric range metadata for a UIA range-value provider.
#[derive(Debug, Clone, Copy)]
pub struct AccessibleRangeValue {
    pub current: f64,
    pub min: f64,
    pub max: f64,
    pub step: Option<f64>,
}

static NEXT_ELEMENT_ID: AtomicU32 = AtomicU32::new(1);

impl AccessibleElementInfo {
    pub fn new(role: AccessibleRole) -> Self {
        let element_id = NEXT_ELEMENT_ID.fetch_add(1, Ordering::Relaxed);
        Self {
            role,
            name: None,
            value: None,
            element_id,
            node_id: crate::AccessibilityId(element_id as u64),
            actions: Vec::new(),
            toggle_value: None,
            range_value: None,
            text_value: None,
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

    pub fn with_actions(mut self, actions: Vec<crate::AccessibilityAction>) -> Self {
        self.actions = actions;
        self
    }

    pub fn with_node_id(mut self, node_id: crate::AccessibilityId) -> Self {
        self.node_id = node_id;
        self
    }

    pub fn with_toggle_value(mut self, toggle_value: bool) -> Self {
        self.toggle_value = Some(toggle_value);
        self
    }

    pub fn with_range_value(mut self, current: f64, min: f64, max: f64, step: Option<f64>) -> Self {
        self.range_value = Some(AccessibleRangeValue {
            current,
            min,
            max,
            step,
        });
        self
    }

    pub fn with_text_value(mut self, value: impl Into<String>) -> Self {
        self.text_value = Some(value.into());
        self
    }

    fn supports_action(&self, action: crate::AccessibilityAction) -> bool {
        self.actions.contains(&action)
    }
}

type PendingActionQueue = Rc<RefCell<Vec<crate::AccessibilityActionRequest>>>;

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
    pending_actions: PendingActionQueue,
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
            pending_actions: Rc::new(RefCell::new(Vec::new())),
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
            children.push(GpuiElementProvider::new(
                self.hwnd,
                info,
                self.pending_actions.clone(),
            ));
        }
    }

    /// Remove all children (e.g., on re-render).
    pub fn clear_elements(&self) {
        self.children.borrow_mut().clear();
    }

    /// Drain normalized UIA action requests that child providers received.
    pub fn drain_actions(&self) -> Vec<crate::AccessibilityActionRequest> {
        self.pending_actions.borrow_mut().drain(..).collect()
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
#[implement(
    IRawElementProviderSimple,
    IRawElementProviderFragment,
    IInvokeProvider,
    IToggleProvider,
    IExpandCollapseProvider,
    IRangeValueProvider,
    IValueProvider
)]
pub struct GpuiElementProvider {
    #[allow(dead_code)]
    hwnd: HWND,
    pub(crate) info: RefCell<AccessibleElementInfo>,
    pending_actions: PendingActionQueue,
}

impl GpuiElementProvider {
    fn new(
        hwnd: HWND,
        info: AccessibleElementInfo,
        pending_actions: PendingActionQueue,
    ) -> ComObject<Self> {
        ComObject::new(Self {
            hwnd,
            info: RefCell::new(info),
            pending_actions,
        })
    }
}

impl IRawElementProviderSimple_Impl for GpuiElementProvider_Impl {
    fn ProviderOptions(&self) -> Result<ProviderOptions> {
        Ok(ProviderOptions_ServerSideProvider)
    }

    fn GetPatternProvider(&self, pattern_id: UIA_PATTERN_ID) -> Result<IUnknown> {
        let info = self.info.borrow();
        match pattern_id {
            UIA_InvokePatternId
                if info.supports_action(crate::AccessibilityAction::Click)
                    || info.supports_action(crate::AccessibilityAction::ShowMenu)
                    || info.supports_action(crate::AccessibilityAction::Dismiss) =>
            {
                let provider: IInvokeProvider = self.to_interface();
                Ok(provider.into())
            }
            UIA_TogglePatternId if info.supports_action(crate::AccessibilityAction::Toggle) => {
                let provider: IToggleProvider = self.to_interface();
                Ok(provider.into())
            }
            UIA_ExpandCollapsePatternId
                if info.supports_action(crate::AccessibilityAction::Expand)
                    || info.supports_action(crate::AccessibilityAction::Collapse) =>
            {
                let provider: IExpandCollapseProvider = self.to_interface();
                Ok(provider.into())
            }
            UIA_RangeValuePatternId
                if info.supports_action(crate::AccessibilityAction::SetValue)
                    && info.range_value.is_some() =>
            {
                let provider: IRangeValueProvider = self.to_interface();
                Ok(provider.into())
            }
            UIA_ValuePatternId
                if info.supports_action(crate::AccessibilityAction::SetValue)
                    && info.text_value.is_some() =>
            {
                let provider: IValueProvider = self.to_interface();
                Ok(provider.into())
            }
            _ => Err(Error::empty()),
        }
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
        self.record_action(crate::AccessibilityAction::Focus);
        Ok(())
    }

    fn FragmentRoot(&self) -> Result<IRawElementProviderFragmentRoot> {
        Err(Error::empty())
    }
}

impl GpuiElementProvider_Impl {
    fn record_action(&self, action: crate::AccessibilityAction) -> bool {
        let info = self.info.borrow();
        if !info.supports_action(action) {
            return false;
        }
        self.pending_actions
            .borrow_mut()
            .push(crate::AccessibilityActionRequest::new(info.node_id, action));
        true
    }

    fn record_action_with_payload(
        &self,
        action: crate::AccessibilityAction,
        payload: crate::AccessibilityActionPayload,
    ) -> bool {
        let info = self.info.borrow();
        if !info.supports_action(action) {
            return false;
        }
        self.pending_actions
            .borrow_mut()
            .push(crate::AccessibilityActionRequest::with_payload(
                info.node_id,
                action,
                payload,
            ));
        true
    }

    fn invoke_action(&self) -> Option<crate::AccessibilityAction> {
        let info = self.info.borrow();
        [
            crate::AccessibilityAction::Click,
            crate::AccessibilityAction::ShowMenu,
            crate::AccessibilityAction::Dismiss,
        ]
        .into_iter()
        .find(|action| info.supports_action(*action))
    }
}

impl IInvokeProvider_Impl for GpuiElementProvider_Impl {
    fn Invoke(&self) -> Result<()> {
        if let Some(action) = self.invoke_action() {
            self.record_action(action);
            Ok(())
        } else {
            Err(Error::empty())
        }
    }
}

impl IToggleProvider_Impl for GpuiElementProvider_Impl {
    fn Toggle(&self) -> Result<()> {
        if self.record_action(crate::AccessibilityAction::Toggle) {
            Ok(())
        } else {
            Err(Error::empty())
        }
    }

    fn ToggleState(&self) -> Result<ToggleState> {
        Ok(match self.info.borrow().toggle_value {
            Some(true) => ToggleState_On,
            Some(false) => ToggleState_Off,
            None => ToggleState_Indeterminate,
        })
    }
}

impl IExpandCollapseProvider_Impl for GpuiElementProvider_Impl {
    fn Expand(&self) -> Result<()> {
        if self.record_action(crate::AccessibilityAction::Expand) {
            Ok(())
        } else {
            Err(Error::empty())
        }
    }

    fn Collapse(&self) -> Result<()> {
        if self.record_action(crate::AccessibilityAction::Collapse) {
            Ok(())
        } else {
            Err(Error::empty())
        }
    }

    fn ExpandCollapseState(&self) -> Result<ExpandCollapseState> {
        Ok(ExpandCollapseState_LeafNode)
    }
}

impl IRangeValueProvider_Impl for GpuiElementProvider_Impl {
    fn SetValue(&self, val: f64) -> Result<()> {
        if self.record_action_with_payload(
            crate::AccessibilityAction::SetValue,
            crate::AccessibilityActionPayload::NumericValue(val),
        ) {
            Ok(())
        } else {
            Err(Error::empty())
        }
    }

    fn Value(&self) -> Result<f64> {
        self.info
            .borrow()
            .range_value
            .map(|range| range.current)
            .ok_or_else(Error::empty)
    }

    fn IsReadOnly(&self) -> Result<BOOL> {
        Ok(BOOL(0))
    }

    fn Maximum(&self) -> Result<f64> {
        self.info
            .borrow()
            .range_value
            .map(|range| range.max)
            .ok_or_else(Error::empty)
    }

    fn Minimum(&self) -> Result<f64> {
        self.info
            .borrow()
            .range_value
            .map(|range| range.min)
            .ok_or_else(Error::empty)
    }

    fn LargeChange(&self) -> Result<f64> {
        Ok(self
            .info
            .borrow()
            .range_value
            .and_then(|range| range.step)
            .unwrap_or(10.0))
    }

    fn SmallChange(&self) -> Result<f64> {
        Ok(self
            .info
            .borrow()
            .range_value
            .and_then(|range| range.step)
            .unwrap_or(1.0))
    }
}

impl IValueProvider_Impl for GpuiElementProvider_Impl {
    fn SetValue(&self, val: &PCWSTR) -> Result<()> {
        let value = unsafe { val.to_string()? };
        if self.record_action_with_payload(
            crate::AccessibilityAction::SetValue,
            crate::AccessibilityActionPayload::Value(value),
        ) {
            Ok(())
        } else {
            Err(Error::empty())
        }
    }

    fn Value(&self) -> Result<BSTR> {
        self.info
            .borrow()
            .text_value
            .as_ref()
            .map(|value| BSTR::from(value.as_str()))
            .ok_or_else(Error::empty)
    }

    fn IsReadOnly(&self) -> Result<BOOL> {
        Ok(BOOL(0))
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
