//! External UIA client for the production virtual-tree and Unicode editor examples. It uses no
//! synthetic keyboard/pointer input and never substitutes model-only tests.

#[cfg(target_os = "windows")]
#[path = "windows_accessibility_client/text.rs"]
mod text_client;

#[cfg(target_os = "windows")]
fn main() -> anyhow::Result<()> {
    use anyhow::{Context as _, ensure};
    use std::{
        fs, thread,
        time::{Duration, Instant},
    };
    use windows::{
        Win32::{
            System::{
                Com::{
                    CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx,
                    CoUninitialize,
                },
                Variant::VARIANT,
            },
            UI::{
                Accessibility::*,
                WindowsAndMessaging::{FindWindowW, GetForegroundWindow, GetWindowThreadProcessId},
            },
        },
        core::{PCWSTR, w},
    };

    fn wait<T>(mut read: impl FnMut() -> anyhow::Result<T>) -> anyhow::Result<T> {
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            match read() {
                Ok(result) => return Ok(result),
                Err(error) if Instant::now() >= deadline => return Err(error),
                Err(_) => thread::sleep(Duration::from_millis(100)),
            }
        }
    }
    fn named(
        uia: &IUIAutomation,
        root: &IUIAutomationElement,
        name: &str,
    ) -> anyhow::Result<IUIAutomationElement> {
        let condition =
            unsafe { uia.CreatePropertyCondition(UIA_NamePropertyId, &VARIANT::from(name)) }?;
        Ok(unsafe { root.FindFirst(TreeScope_Descendants, &condition) }?)
    }
    fn children(
        uia: &IUIAutomation,
        root: &IUIAutomationElement,
    ) -> anyhow::Result<IUIAutomationElementArray> {
        let condition = unsafe { uia.CreateTrueCondition() }?;
        let result = unsafe { root.FindAll(TreeScope_Children, &condition) }?;
        ensure!(
            unsafe { result.Length() }? <= 4_000,
            "native hierarchy exceeded the fixture bound"
        );
        Ok(result)
    }
    let process_id: u32 = std::env::args()
        .nth(1)
        .context("expected owned app PID")?
        .parse()?;
    let app_log = std::env::args().nth(2).context("expected owned app log")?;
    unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.ok()?;
    struct ComGuard;
    impl Drop for ComGuard {
        fn drop(&mut self) {
            unsafe { CoUninitialize() };
        }
    }
    let _com = ComGuard;
    let uia: IUIAutomation =
        unsafe { CoCreateInstance(&CUIAutomation8, None, CLSCTX_INPROC_SERVER) }?;
    if std::env::args().nth(3).as_deref() == Some("--text") {
        return text_client::run(&uia, process_id, &app_log);
    }
    let window = wait(|| {
        let hwnd = unsafe { FindWindowW(PCWSTR::null(), w!("Kael virtual tree")) }?;
        let mut actual = 0;
        unsafe { GetWindowThreadProcessId(hwnd, Some(&mut actual)) };
        ensure!(
            actual == process_id,
            "title matched an unrelated application"
        );
        Ok(unsafe { uia.ElementFromHandle(hwnd) }?)
    })?;
    let tree = wait(|| named(&uia, &window, "Project files"))?;
    let walker = unsafe { uia.ControlViewWalker() }?;
    let projects = wait(|| {
        let projects = children(&uia, &tree)?;
        ensure!(
            unsafe { projects.Length() }? == 25,
            "expected 25 native project roots"
        );
        Ok(projects)
    })?;
    let project = unsafe { projects.GetElement(24) }?;
    ensure!(
        unsafe { project.CurrentName() }? == "Project 25",
        "native project order changed"
    );
    let mut rows = 25;
    for index in 0..25 {
        let project = unsafe { projects.GetElement(index) }?;
        rows += wait(|| {
            let count = unsafe { children(&uia, &project)?.Length() }?;
            ensure!(
                count == 4_000,
                "expected 4,000 native descendants for project {index}"
            );
            Ok(count)
        })?;
    }
    ensure!(rows == 100_025, "full native tree omitted offscreen rows");
    let last = unsafe { children(&uia, &project)?.GetElement(3_999) }?;
    ensure!(
        unsafe { last.CurrentName() }? == "document_4000.rs",
        "last offscreen native row missing"
    );
    let parent = unsafe { walker.GetParentElement(&last) }?;
    ensure!(
        unsafe { uia.CompareElements(&parent, &project) }?.as_bool(),
        "offscreen parent mismatch"
    );
    let expand: IUIAutomationExpandCollapsePattern =
        unsafe { project.GetCurrentPatternAs(UIA_ExpandCollapsePatternId) }?;
    // An idle interval has no injected input or application polling timer.
    thread::sleep(Duration::from_secs(2));
    unsafe { expand.Collapse() }?;
    wait(|| {
        ensure!(
            unsafe { children(&uia, &project)?.Length() }? == 0,
            "idle collapse was not executed"
        );
        Ok(())
    })?;
    unsafe { expand.Expand() }?;
    let restored = wait(|| {
        let documents = children(&uia, &project)?;
        ensure!(
            unsafe { documents.Length() }? == 4_000,
            "idle expand was not executed"
        );
        Ok(unsafe { documents.GetElement(3_999) }?)
    })?;
    ensure!(
        unsafe { uia.CompareElements(&last, &restored) }?.as_bool(),
        "surviving row lost native identity"
    );
    thread::sleep(Duration::from_secs(2));
    unsafe { restored.SetFocus() }?;
    wait(|| {
        let foreground = unsafe { GetForegroundWindow() };
        let mut foreground_pid = 0;
        unsafe { GetWindowThreadProcessId(foreground, Some(&mut foreground_pid)) };
        let focused = unsafe { uia.GetFocusedElement() }.with_context(|| {
            format!("desktop UIA focus missing: foreground_pid={foreground_pid} owned_pid={process_id} owned_node_keyboard_focus={:?}",
                    unsafe { restored.CurrentHasKeyboardFocus() })
        })?;
        ensure!(
            unsafe { uia.CompareElements(&focused, &restored) }?.as_bool(),
            "offscreen idle focus was not executed"
        );
        Ok(())
    })?;
    let invoke: IUIAutomationInvokePattern =
        unsafe { restored.GetCurrentPatternAs(UIA_InvokePatternId) }?;
    unsafe { invoke.Invoke() }?;
    wait(|| {
        let log = fs::read_to_string(&app_log)?;
        for marker in [
            "NATIVE_ACCESSIBILITY_MODEL: rows=100025",
            "NATIVE_ACCESSIBILITY_MODEL: rows=96025",
            "NATIVE_ACCESSIBILITY_DISCLOSURE: id=96024 expanded=false",
            "NATIVE_ACCESSIBILITY_DISCLOSURE: id=96024 expanded=true",
            "NATIVE_ACCESSIBILITY_SELECT: id=100024",
        ] {
            ensure!(log.contains(marker), "missing foreground marker {marker}");
        }
        Ok(())
    })?;
    println!(
        "NATIVE_ACCESSIBILITY_RUNTIME_OK: backend=uia rows={rows} projects=25 children=4000 last=100024 idle_disclosure=true idle_focus=true idle_select=true"
    );
    Ok(())
}

#[cfg(not(target_os = "windows"))]
fn main() -> anyhow::Result<()> {
    anyhow::bail!("windows_accessibility_client requires native Windows UI Automation")
}
