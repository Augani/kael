//! Acceptance against the real Editor's native UIA TextPattern.
use anyhow::{Result, ensure};
use std::{
    thread,
    time::{Duration, Instant},
};
use windows::{
    Win32::{
        Foundation::POINT,
        System::{
            Com::SAFEARRAY,
            Ole::{
                SafeArrayDestroy, SafeArrayGetDim, SafeArrayGetElement, SafeArrayGetLBound,
                SafeArrayGetUBound, SafeArrayGetVartype,
            },
            Variant::{VARIANT, VT_R8},
        },
        UI::{
            Accessibility::*,
            WindowsAndMessaging::{FindWindowW, GetWindowThreadProcessId},
        },
    },
    core::{BSTR, Interface, PCWSTR, w},
};

const DOCUMENT: &str =
    include_str!("../../../kael_ui/examples/fixtures/native_unicode_document.txt");
const REPLACEMENT: &str =
    include_str!("../../../kael_ui/examples/fixtures/native_unicode_replacement.txt");
const NEEDLE: &str = "日本語 👩🏽‍💻 café";
const OFFSCREEN: &str = "KAEL_TEXT_OFFSCREEN_END";

fn wait<T>(mut read: impl FnMut() -> Result<T>) -> Result<T> {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        match read() {
            Ok(value) => return Ok(value),
            Err(error) if Instant::now() >= deadline => return Err(error),
            Err(_) => thread::sleep(Duration::from_millis(100)),
        }
    }
}

fn named(
    uia: &IUIAutomation,
    root: &IUIAutomationElement,
    name: &str,
) -> Result<IUIAutomationElement> {
    let condition =
        unsafe { uia.CreatePropertyCondition(UIA_NamePropertyId, &VARIANT::from(name)) }?;
    Ok(unsafe { root.FindFirst(TreeScope_Descendants, &condition) }?)
}

#[derive(Clone, Copy, Debug)]
struct Rect {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}
impl Rect {
    fn intersection(self, other: Self) -> Option<Self> {
        let x = self.x.max(other.x);
        let y = self.y.max(other.y);
        let right = (self.x + self.width).min(other.x + other.width);
        let bottom = (self.y + self.height).min(other.y + other.height);
        (right > x && bottom > y).then_some(Self {
            x,
            y,
            width: right - x,
            height: bottom - y,
        })
    }
}

fn rectangles(range: &IUIAutomationTextRange) -> Result<Vec<Rect>> {
    let array = unsafe { range.GetBoundingRectangles() }?;
    ensure!(
        !array.is_null(),
        "TextPattern returned a null rectangle array"
    );
    struct Guard(*mut SAFEARRAY);
    impl Drop for Guard {
        fn drop(&mut self) {
            let _ = unsafe { SafeArrayDestroy(self.0) };
        }
    }
    let _guard = Guard(array);
    ensure!(
        unsafe { SafeArrayGetDim(array) } == 1,
        "rectangle array must be one-dimensional"
    );
    ensure!(
        unsafe { SafeArrayGetVartype(array) }? == VT_R8,
        "rectangle array must contain doubles"
    );
    let first = unsafe { SafeArrayGetLBound(array, 1) }?;
    let last = unsafe { SafeArrayGetUBound(array, 1) }?;
    let length = i64::from(last) - i64::from(first) + 1;
    ensure!(
        (0..=16_384).contains(&length) && length % 4 == 0,
        "invalid rectangle array length"
    );
    let mut values = Vec::with_capacity(length as usize);
    for index in first..=last {
        let mut value = 0.0_f64;
        unsafe { SafeArrayGetElement(array, &index, (&mut value as *mut f64).cast()) }?;
        ensure!(value.is_finite(), "non-finite text geometry");
        values.push(value);
    }
    values
        .chunks_exact(4)
        .map(|v| {
            ensure!(v[2] >= 0.0 && v[3] >= 0.0, "negative text rectangle size");
            Ok(Rect {
                x: v[0],
                y: v[1],
                width: v[2],
                height: v[3],
            })
        })
        .collect()
}

fn selected(pattern: &IUIAutomationTextPattern) -> Result<IUIAutomationTextRange> {
    let ranges = unsafe { pattern.GetSelection() }?;
    ensure!(
        unsafe { ranges.Length() }? == 1,
        "Editor must expose exactly one selection"
    );
    Ok(unsafe { ranges.GetElement(0) }?)
}

fn visible(
    pattern: &IUIAutomationTextPattern,
    editor: Rect,
) -> Result<Vec<IUIAutomationTextRange>> {
    let ranges = unsafe { pattern.GetVisibleRanges() }?;
    let count = unsafe { ranges.Length() }?;
    ensure!(
        (1..=4_096).contains(&count),
        "visible Editor has no bounded native visible ranges"
    );
    let mut result = Vec::new();
    for index in 0..count {
        let range = unsafe { ranges.GetElement(index) }?;
        ensure!(
            !unsafe { range.GetText(-1) }?.is_empty(),
            "visible Editor range is degenerate"
        );
        let boxes = rectangles(&range)?;
        ensure!(
            !boxes.is_empty() && boxes.iter().all(|rect| rect.intersection(editor).is_some()),
            "visible range includes unpainted/offscreen geometry"
        );
        result.push(range);
    }
    Ok(result)
}

pub(super) fn run(uia: &IUIAutomation, process_id: u32, app_log: &str) -> Result<()> {
    let window = wait(|| {
        let hwnd = unsafe { FindWindowW(PCWSTR::null(), w!("Kael native text accessibility")) }?;
        let mut actual = 0;
        unsafe { GetWindowThreadProcessId(hwnd, Some(&mut actual)) };
        ensure!(
            actual == process_id,
            "text title matched an unrelated application"
        );
        Ok(unsafe { uia.ElementFromHandle(hwnd) }?)
    })?;
    let editor = wait(|| named(uia, &window, "Native Unicode document"))?;
    let protected = wait(|| named(uia, &window, "Read-only Unicode document"))?;
    let pattern: IUIAutomationTextPattern =
        wait(|| Ok(unsafe { editor.GetCurrentPatternAs(UIA_TextPatternId) }?))?;
    let document = wait(|| {
        let range = unsafe { pattern.DocumentRange() }?;
        let text = unsafe { range.GetText(-1) }?.to_string();
        ensure!(
            text == DOCUMENT,
            "native full document differs: {} bytes vs {}",
            text.len(),
            DOCUMENT.len()
        );
        Ok(range)
    })?;
    ensure!(
        unsafe { pattern.SupportedTextSelection() }? == SupportedTextSelection_Single,
        "Editor does not expose native text selection"
    );
    let bounds = unsafe { editor.CurrentBoundingRectangle() }?;
    let bounds = Rect {
        x: bounds.left as f64,
        y: bounds.top as f64,
        width: (bounds.right - bounds.left) as f64,
        height: (bounds.bottom - bounds.top) as f64,
    };
    let initial_visible = wait(|| visible(&pattern, bounds))?;
    ensure!(
        initial_visible
            .iter()
            .all(|range| unsafe { range.GetText(-1) }
                .is_ok_and(|text| !text.to_string().contains(OFFSCREEN))),
        "offscreen tail leaked into initial visible ranges"
    );
    let missing = BSTR::from("KAEL_TEXT_MISSING_SENTINEL");
    let mut missing_range = std::ptr::null_mut();
    let status = unsafe {
        (document.vtable().FindText)(
            document.as_raw(),
            missing.as_ptr().cast_mut().cast(),
            false.into(),
            false.into(),
            &mut missing_range,
        )
    };
    ensure!(
        status.0 == 0 && missing_range.is_null(),
        "no-match must return S_OK and a null native range"
    );
    let first = unsafe { document.FindText(&BSTR::from(NEEDLE), false, false) }?;
    let last = unsafe { document.FindText(&BSTR::from(NEEDLE), true, false) }?;
    ensure!(
        unsafe { first.GetText(-1) }? == NEEDLE && unsafe { last.GetText(-1) }? == NEEDLE,
        "native Unicode search returned an inexact range"
    );
    ensure!(
        unsafe {
            first.CompareEndpoints(
                TextPatternRangeEndpoint_Start,
                &last,
                TextPatternRangeEndpoint_Start,
            )
        }? < 0,
        "backward search failed to return the last occurrence"
    );
    let folded = unsafe { document.FindText(&BSTR::from("CAFÉ"), false, true) }?;
    ensure!(
        unsafe { folded.GetText(-1) }? == "café",
        "case-insensitive Unicode search failed"
    );
    let crlf = unsafe { document.FindText(&BSTR::from("\r\n"), false, false) }?;
    unsafe { crlf.ExpandToEnclosingUnit(TextUnit_Character) }?;
    ensure!(
        unsafe { crlf.GetText(-1) }? == "\r\n",
        "native character navigation split CRLF"
    );
    let tail = unsafe { document.FindText(&BSTR::from(OFFSCREEN), false, false) }?;
    ensure!(
        rectangles(&tail)?
            .iter()
            .all(|rect| rect.intersection(bounds).is_none()),
        "tail must begin offscreen"
    );

    unsafe { editor.SetFocus() }?;
    thread::sleep(Duration::from_secs(2));
    unsafe { tail.ScrollIntoView(true) }?;
    wait(|| {
        let ranges = visible(&pattern, bounds)?;
        ensure!(
            ranges.iter().any(|range| unsafe { range.GetText(-1) }
                .is_ok_and(|text| text.to_string().contains(OFFSCREEN))),
            "idle text reveal did not expose the offscreen tail"
        );
        Ok(())
    })?;
    unsafe { last.Select() }?;
    wait(|| {
        ensure!(
            unsafe { selected(&pattern)?.GetText(-1) }? == NEEDLE,
            "native Unicode selection did not reach Editor"
        );
        Ok(())
    })?;
    let caret = unsafe { last.Clone() }?;
    unsafe {
        caret.MoveEndpointByRange(
            TextPatternRangeEndpoint_Start,
            &last,
            TextPatternRangeEndpoint_End,
        )
    }?;
    unsafe { caret.Select() }?;
    wait(|| {
        let selected = selected(&pattern)?;
        ensure!(
            unsafe { selected.GetText(-1) }?.is_empty(),
            "caret selection is not degenerate"
        );
        ensure!(
            unsafe {
                selected.CompareEndpoints(
                    TextPatternRangeEndpoint_Start,
                    &last,
                    TextPatternRangeEndpoint_End,
                )
            }? == 0,
            "Unicode caret offset is incorrect"
        );
        Ok(())
    })?;
    let box_in_view = rectangles(&last)?
        .into_iter()
        .find_map(|rect| rect.intersection(bounds));
    let box_in_view = box_in_view
        .ok_or_else(|| anyhow::anyhow!("revealed Unicode needle has no native glyph geometry"))?;
    let point = POINT {
        x: (box_in_view.x + box_in_view.width * 0.5).round() as i32,
        y: (box_in_view.y + box_in_view.height * 0.5).round() as i32,
    };
    let hit = unsafe { pattern.RangeFromPoint(point) }?;
    unsafe { hit.ExpandToEnclosingUnit(TextUnit_Character) }?;
    ensure!(
        !unsafe { hit.GetText(-1) }?.is_empty()
            && unsafe {
                hit.CompareEndpoints(
                    TextPatternRangeEndpoint_Start,
                    &last,
                    TextPatternRangeEndpoint_Start,
                )
            }? >= 0
            && unsafe {
                hit.CompareEndpoints(
                    TextPatternRangeEndpoint_End,
                    &last,
                    TextPatternRangeEndpoint_End,
                )
            }? <= 0,
        "screen point did not resolve to the painted Unicode range"
    );

    let readonly: IUIAutomationValuePattern =
        unsafe { protected.GetCurrentPatternAs(UIA_ValuePatternId) }?;
    ensure!(
        unsafe { readonly.CurrentIsReadOnly() }?.as_bool(),
        "protected Editor omitted read-only state"
    );
    let readonly_before = unsafe { readonly.CurrentValue() }?.to_string();
    let rejected = unsafe { readonly.SetValue(&BSTR::from("forbidden native mutation")) };
    ensure!(
        rejected.is_err_and(|error| error.code().0 as u32 == UIA_E_INVALIDOPERATION),
        "native read-only mutation must return UIA_E_INVALIDOPERATION"
    );
    let replace = named(uia, &window, "Replace document")?;
    let invoke: IUIAutomationInvokePattern =
        unsafe { replace.GetCurrentPatternAs(UIA_InvokePatternId) }?;
    unsafe { invoke.Invoke() }?;
    wait(|| {
        ensure!(
            unsafe { pattern.DocumentRange()?.GetText(-1) }? == REPLACEMENT,
            "real Editor replacement did not publish a new native document"
        );
        Ok(())
    })?;
    ensure!(
        unsafe { readonly.CurrentValue() }? == readonly_before.as_str(),
        "read-only mutation changed the protected Editor"
    );
    let stale = unsafe { last.Select() };
    ensure!(
        stale.is_err_and(|error| error.code().0 as u32 == UIA_E_ELEMENTNOTAVAILABLE),
        "old revision selection did not reject its stale native range"
    );
    ensure!(
        std::fs::read_to_string(app_log)?.contains("NATIVE_TEXT_STATE"),
        "foreground fixture evidence is missing"
    );
    let disabled = named(uia, &window, "Toggle disabled document")?;
    let disabled: IUIAutomationInvokePattern =
        unsafe { disabled.GetCurrentPatternAs(UIA_InvokePatternId) }?;
    unsafe { disabled.Invoke() }?;
    wait(|| {
        ensure!(
            !unsafe { editor.CurrentIsEnabled() }?.as_bool(),
            "disabled Editor state is missing"
        );
        Ok(())
    })?;
    let disabled_document = unsafe { pattern.DocumentRange() }?;
    ensure!(
        unsafe { disabled_document.GetText(-1) }? == REPLACEMENT,
        "disabled text stopped being readable"
    );
    for result in [
        unsafe { disabled_document.Select() },
        unsafe { disabled_document.ScrollIntoView(true) },
        unsafe { editor.SetFocus() },
    ] {
        ensure!(
            result.is_err_and(|error| error.code().0 as u32 == UIA_E_ELEMENTNOTENABLED),
            "disabled native text action must return UIA_E_ELEMENTNOTENABLED"
        );
    }
    unsafe { disabled.Invoke() }?;
    wait(|| {
        ensure!(
            unsafe { editor.CurrentIsEnabled() }?.as_bool(),
            "Editor did not re-enable"
        );
        Ok(())
    })?;
    println!(
        "NATIVE_TEXT_ACCESSIBILITY_RUNTIME_OK: backend=uia bytes={} utf16={} lines={} unicode_search=true visible_ranges=true idle_reveal=true selection=true caret=true point=true readonly=true disabled=true stale=true",
        DOCUMENT.len(),
        DOCUMENT.encode_utf16().count(),
        DOCUMENT.lines().count()
    );
    let finish = named(uia, &window, "Native text checks complete")?;
    let finish: IUIAutomationInvokePattern =
        unsafe { finish.GetCurrentPatternAs(UIA_InvokePatternId) }?;
    unsafe { finish.Invoke() }?;
    Ok(())
}
