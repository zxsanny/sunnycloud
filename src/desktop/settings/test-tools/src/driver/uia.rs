//! Windows backend: UI Automation, the API Narrator and NVDA read. COM objects live on the
//! thread that created them, so the backend (and the session using it) stays on one thread.

use std::fmt::Display;

use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx,
};
use windows::Win32::UI::Accessibility::{
    CUIAutomation, IUIAutomation, IUIAutomationElement, IUIAutomationInvokePattern,
    IUIAutomationRangeValuePattern, IUIAutomationSelectionItemPattern, IUIAutomationTogglePattern,
    IUIAutomationValuePattern, ToggleState_On, TreeScope_Children, UIA_ButtonControlTypeId,
    UIA_CheckBoxControlTypeId, UIA_ComboBoxControlTypeId, UIA_DocumentControlTypeId,
    UIA_EditControlTypeId, UIA_GroupControlTypeId, UIA_InvokePatternId,
    UIA_IsInvokePatternAvailablePropertyId, UIA_IsRangeValuePatternAvailablePropertyId,
    UIA_IsSelectionItemPatternAvailablePropertyId, UIA_IsTogglePatternAvailablePropertyId,
    UIA_IsValuePatternAvailablePropertyId, UIA_ListControlTypeId, UIA_ListItemControlTypeId,
    UIA_PaneControlTypeId, UIA_RadioButtonControlTypeId, UIA_RangeValuePatternId,
    UIA_SelectionItemPatternId, UIA_SpinnerControlTypeId, UIA_TabControlTypeId,
    UIA_TabItemControlTypeId, UIA_TableControlTypeId, UIA_TextControlTypeId, UIA_TogglePatternId,
    UIA_ValuePatternId, UIA_WindowControlTypeId,
};
use windows::core::BSTR;

use super::{Backend, DriverError, NodeInfo, Role};

pub struct UiaBackend {
    automation: IUIAutomation,
}

fn call(error: impl Display) -> DriverError {
    DriverError::Call(error.to_string())
}

impl UiaBackend {
    pub async fn connect() -> Result<Self, DriverError> {
        // S_FALSE (already initialised on this thread) is a success; a conflicting apartment
        // model is an error and is reported.
        // SAFETY: plain COM initialisation of the calling thread.
        unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }
            .ok()
            .map_err(|e| DriverError::Unavailable(e.to_string()))?;
        // SAFETY: CUIAutomation is the documented in-process class of IUIAutomation.
        let automation = unsafe { CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER) }
            .map_err(|e| DriverError::Unavailable(e.to_string()))?;
        Ok(Self { automation })
    }

    fn all_children(
        &self,
        node: &IUIAutomationElement,
    ) -> Result<Vec<IUIAutomationElement>, DriverError> {
        // SAFETY: both calls operate on live COM objects owned by this backend.
        unsafe {
            let all = self.automation.CreateTrueCondition().map_err(call)?;
            let found = node.FindAll(TreeScope_Children, &all).map_err(call)?;
            let count = found.Length().map_err(call)?;
            (0..count)
                .map(|i| found.GetElement(i).map_err(call))
                .collect()
        }
    }
}

/// Whether the element supports a pattern, read from its `Is<Pattern>Available` property.
///
/// # Safety
/// `node` must be a live element.
unsafe fn available(
    node: &IUIAutomationElement,
    property: windows::Win32::UI::Accessibility::UIA_PROPERTY_ID,
) -> Result<bool, DriverError> {
    // SAFETY: the caller guarantees a live element.
    let value = unsafe { node.GetCurrentPropertyValue(property) }.map_err(call)?;
    bool::try_from(&value).map_err(call)
}

fn map_role(control_type: i32) -> Role {
    match control_type {
        t if t == UIA_WindowControlTypeId.0 => Role::Window,
        t if t == UIA_ButtonControlTypeId.0 => Role::Button,
        t if t == UIA_CheckBoxControlTypeId.0 => Role::CheckBox,
        t if t == UIA_RadioButtonControlTypeId.0 => Role::RadioButton,
        t if t == UIA_EditControlTypeId.0 || t == UIA_DocumentControlTypeId.0 => Role::TextInput,
        t if t == UIA_SpinnerControlTypeId.0 => Role::SpinButton,
        t if t == UIA_ComboBoxControlTypeId.0 => Role::ComboBox,
        t if t == UIA_TabItemControlTypeId.0 => Role::Tab,
        t if t == UIA_TabControlTypeId.0 => Role::TabList,
        t if t == UIA_ListControlTypeId.0 => Role::List,
        t if t == UIA_ListItemControlTypeId.0 => Role::ListItem,
        t if t == UIA_TextControlTypeId.0 => Role::Label,
        t if t == UIA_TableControlTypeId.0 => Role::Table,
        t if t == UIA_GroupControlTypeId.0 || t == UIA_PaneControlTypeId.0 => Role::Group,
        _ => Role::Other,
    }
}

impl Backend for UiaBackend {
    type Node = IUIAutomationElement;

    async fn application(&self, pid: u32) -> Result<Option<IUIAutomationElement>, DriverError> {
        // SAFETY: GetRootElement returns the desktop element of this automation object.
        let desktop = unsafe { self.automation.GetRootElement() }.map_err(call)?;
        for window in self.all_children(&desktop)? {
            // SAFETY: `window` is a live element.
            let owner = unsafe { window.CurrentProcessId() }.map_err(call)?;
            if u32::try_from(owner) == Ok(pid) {
                return Ok(Some(window));
            }
        }
        Ok(None)
    }

    async fn children(
        &self,
        node: &IUIAutomationElement,
    ) -> Result<Vec<IUIAutomationElement>, DriverError> {
        self.all_children(node)
    }

    async fn info(&self, node: &IUIAutomationElement) -> Result<NodeInfo, DriverError> {
        // SAFETY: property reads and pattern queries on a live element.
        unsafe {
            let name = node.CurrentName().map_err(call)?.to_string();
            let control_type = node.CurrentControlType().map_err(call)?;
            let native_role = node
                .CurrentLocalizedControlType()
                .map_err(call)?
                .to_string();
            let role = map_role(control_type.0);
            let value = if available(node, UIA_IsTogglePatternAvailablePropertyId)? {
                let toggle: IUIAutomationTogglePattern = node
                    .GetCurrentPatternAs(UIA_TogglePatternId)
                    .map_err(call)?;
                let on = toggle.CurrentToggleState().map_err(call)? == ToggleState_On;
                Some(if on { "checked" } else { "unchecked" }.to_owned())
            } else if available(node, UIA_IsValuePatternAvailablePropertyId)? {
                let pattern: IUIAutomationValuePattern =
                    node.GetCurrentPatternAs(UIA_ValuePatternId).map_err(call)?;
                Some(pattern.CurrentValue().map_err(call)?.to_string())
            } else if available(node, UIA_IsRangeValuePatternAvailablePropertyId)? {
                let pattern: IUIAutomationRangeValuePattern = node
                    .GetCurrentPatternAs(UIA_RangeValuePatternId)
                    .map_err(call)?;
                Some(pattern.CurrentValue().map_err(call)?.to_string())
            } else {
                None
            };
            Ok(NodeInfo {
                name,
                role,
                native_role,
                value,
            })
        }
    }

    async fn click(&self, node: &IUIAutomationElement) -> Result<(), DriverError> {
        // SAFETY: pattern queries and invocation on a live element.
        unsafe {
            if available(node, UIA_IsInvokePatternAvailablePropertyId)? {
                let pattern: IUIAutomationInvokePattern = node
                    .GetCurrentPatternAs(UIA_InvokePatternId)
                    .map_err(call)?;
                return pattern.Invoke().map_err(call);
            }
            if available(node, UIA_IsTogglePatternAvailablePropertyId)? {
                let pattern: IUIAutomationTogglePattern = node
                    .GetCurrentPatternAs(UIA_TogglePatternId)
                    .map_err(call)?;
                return pattern.Toggle().map_err(call);
            }
            if available(node, UIA_IsSelectionItemPatternAvailablePropertyId)? {
                let pattern: IUIAutomationSelectionItemPattern = node
                    .GetCurrentPatternAs(UIA_SelectionItemPatternId)
                    .map_err(call)?;
                return pattern.Select().map_err(call);
            }
        }
        Err(DriverError::Unsupported("invoke, toggle or select"))
    }

    async fn type_text(&self, node: &IUIAutomationElement, text: &str) -> Result<(), DriverError> {
        // SAFETY: pattern query and SetValue on a live element.
        unsafe {
            if !available(node, UIA_IsValuePatternAvailablePropertyId)? {
                return Err(DriverError::Unsupported("the Value pattern"));
            }
            let pattern: IUIAutomationValuePattern =
                node.GetCurrentPatternAs(UIA_ValuePatternId).map_err(call)?;
            if pattern.CurrentIsReadOnly().map_err(call)?.as_bool() {
                return Err(DriverError::Call("the field is read-only".to_owned()));
            }
            pattern.SetValue(&BSTR::from(text)).map_err(call)
        }
    }
}
