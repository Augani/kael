use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    ffi::OsStr,
    mem::ManuallyDrop,
    os::windows::ffi::OsStrExt,
    path::{Path, PathBuf},
    rc::{Rc, Weak},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use ::util::{ResultExt, paths::SanitizedPath};
use anyhow::{Context as _, Result, anyhow};
use async_task::Runnable;
use futures::channel::oneshot::{self, Receiver};
use itertools::Itertools;
use parking_lot::RwLock;
use smallvec::SmallVec;
use windows::{
    UI::ViewManagement::UISettings,
    Win32::{
        Foundation::*,
        Graphics::{Direct3D11::ID3D11Device, Gdi::*},
        Security::Credentials::*,
        System::{Com::*, LibraryLoader::*, Ole::*, SystemInformation::*},
        UI::{Input::KeyboardAndMouse::*, Shell::*, WindowsAndMessaging::*},
    },
    core::*,
};

use crate::platform::tab_manager::{TabManagerState, WindowTabManager};
use crate::*;
use std::sync::Mutex;

const MAX_CREDENTIAL_BLOB_BYTES: usize = 256 * 1024;

struct CredentialAllocation(*mut CREDENTIALW);

impl Drop for CredentialAllocation {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { CredFree(self.0.cast()) };
        }
    }
}

pub(crate) struct WindowsPlatform {
    inner: Rc<WindowsPlatformInner>,
    raw_window_handles: Arc<RwLock<SmallVec<[SafeHwnd; 4]>>>,
    // The below members will never change throughout the entire lifecycle of the app.
    icon: HICON,
    background_executor: BackgroundExecutor,
    foreground_executor: ForegroundExecutor,
    text_system: Arc<DirectWriteTextSystem>,
    windows_version: WindowsVersion,
    drop_target_helper: IDropTargetHelper,
    handle: HWND,
    disable_direct_composition: bool,
    tab_manager_state: Arc<Mutex<TabManagerState>>,
}

struct WindowsPlatformInner {
    state: RefCell<WindowsPlatformState>,
    raw_window_handles: std::sync::Weak<RwLock<SmallVec<[SafeHwnd; 4]>>>,
    keep_alive_without_windows: AtomicBool,
    // The below members will never change throughout the entire lifecycle of the app.
    validation_number: usize,
    main_receiver: crossbeam_channel::Receiver<Runnable>,
}

pub(crate) struct WindowsPlatformState {
    callbacks: PlatformCallbacks,
    menus: Vec<OwnedMenu>,
    jump_list: JumpList,
    pub(crate) tray: Option<WindowsTray>,
    // NOTE: standard cursor handles don't need to close.
    pub(crate) current_cursor: Option<HCURSOR>,
    directx_devices: ManuallyDrop<DirectXDevices>,
    power_save_blockers: HashMap<u32, windows::Win32::System::Power::EXECUTION_STATE>,
    next_blocker_id: u32,
    context_menu_command_map: HashMap<u32, SharedString>,
    flashing_hwnd: Option<HWND>,
    /// Maps registered hotkey IDs to their virtual key codes for WM_KEYUP tracking.
    hotkey_vk_map: HashMap<u32, VIRTUAL_KEY>,
    /// Tracks hotkey IDs that are currently pressed (received WM_HOTKEY but not yet WM_KEYUP).
    pressed_hotkeys: HashSet<u32>,
    /// Tracks foreign windows that were minimized by `hide_other_apps` so they can be restored.
    hidden_app_windows: Vec<HWND>,
    /// Pending notification action IDs received from the COM thread, to be dispatched on the main thread.
    pending_notification_actions: Arc<Mutex<Vec<String>>>,
}

#[derive(Default)]
struct PlatformCallbacks {
    open_urls: Option<Box<dyn FnMut(Vec<String>)>>,
    quit: Option<Box<dyn FnMut()>>,
    reopen: Option<Box<dyn FnMut()>>,
    app_menu_action: Option<Box<dyn FnMut(&dyn Action)>>,
    will_open_app_menu: Option<Box<dyn FnMut()>>,
    validate_app_menu_command: Option<Box<dyn FnMut(&dyn Action) -> bool>>,
    keyboard_layout_change: Option<Box<dyn FnMut()>>,
    tray_icon_event: Option<Box<dyn FnMut(TrayIconEvent)>>,
    tray_menu_action: Option<Box<dyn FnMut(SharedString)>>,
    global_hotkey: Option<Box<dyn FnMut(u32)>>,
    global_hotkey_up: Option<Box<dyn FnMut(u32)>>,
    system_power: Option<Box<dyn FnMut(SystemPowerEvent)>>,
    network_status_change: Option<Box<dyn FnMut(NetworkStatus)>>,
    media_key: Option<Box<dyn FnMut(MediaKeyEvent)>>,
    context_menu: Option<Box<dyn FnMut(SharedString)>>,
    notification_action: Option<Box<dyn FnMut(String)>>,
}

impl WindowsPlatformState {
    fn new(directx_devices: DirectXDevices) -> Self {
        let callbacks = PlatformCallbacks::default();
        let jump_list = JumpList::new();
        let current_cursor = load_cursor(CursorStyle::Arrow);
        let directx_devices = ManuallyDrop::new(directx_devices);

        Self {
            callbacks,
            jump_list,
            tray: None,
            current_cursor,
            directx_devices,
            menus: Vec::new(),
            power_save_blockers: HashMap::new(),
            next_blocker_id: 1,
            context_menu_command_map: HashMap::new(),
            flashing_hwnd: None,
            hotkey_vk_map: HashMap::new(),
            pressed_hotkeys: HashSet::new(),
            hidden_app_windows: Vec::new(),
            pending_notification_actions: Arc::new(Mutex::new(Vec::new())),
        }
    }
}

impl WindowsPlatform {
    pub(crate) fn new() -> Result<Self> {
        unsafe {
            OleInitialize(None).context("unable to initialize Windows OLE")?;
        }
        let directx_devices = DirectXDevices::new().context("Creating DirectX devices")?;
        let (main_sender, main_receiver) = crossbeam_channel::unbounded::<Runnable>();
        let validation_number = if usize::BITS == 64 {
            rand::random::<u64>() as usize
        } else {
            rand::random::<u32>() as usize
        };
        let raw_window_handles = Arc::new(RwLock::new(SmallVec::new()));
        let text_system = Arc::new(
            DirectWriteTextSystem::new(&directx_devices)
                .context("Error creating DirectWriteTextSystem")?,
        );
        register_platform_window_class();
        let mut context = PlatformWindowCreateContext {
            inner: None,
            raw_window_handles: Arc::downgrade(&raw_window_handles),
            validation_number,
            main_receiver: Some(main_receiver),
            directx_devices: Some(directx_devices),
        };
        let result = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE(0),
                PLATFORM_WINDOW_CLASS_NAME,
                None,
                WINDOW_STYLE(0),
                0,
                0,
                0,
                0,
                Some(HWND_MESSAGE),
                None,
                None,
                Some(&context as *const _ as *const _),
            )
        };
        let inner = context
            .inner
            .take()
            .ok_or_else(|| anyhow!("platform window creation did not initialize its state"))??;
        let handle = result?;

        unsafe {
            let _ = windows::Win32::System::RemoteDesktop::WTSRegisterSessionNotification(
                handle,
                windows::Win32::System::RemoteDesktop::NOTIFY_FOR_THIS_SESSION,
            );
        }

        let dispatcher = Arc::new(WindowsDispatcher::new(
            main_sender,
            handle,
            validation_number,
        ));
        let disable_direct_composition = std::env::var(DISABLE_DIRECT_COMPOSITION)
            .is_ok_and(|value| value == "true" || value == "1");
        let background_executor = BackgroundExecutor::new(dispatcher.clone());
        let foreground_executor = ForegroundExecutor::new(dispatcher);

        let drop_target_helper: IDropTargetHelper = unsafe {
            CoCreateInstance(&CLSID_DragDropHelper, None, CLSCTX_INPROC_SERVER)
                .context("Error creating drop target helper.")?
        };
        let icon = load_icon().unwrap_or_default();
        let windows_version = WindowsVersion::new().context("Error retrieve windows version")?;

        Ok(Self {
            inner,
            handle,
            raw_window_handles,
            icon,
            background_executor,
            foreground_executor,
            text_system,
            disable_direct_composition,
            windows_version,
            drop_target_helper,
            tab_manager_state: WindowTabManager::shared_state(),
        })
    }

    pub fn window_from_hwnd(&self, hwnd: HWND) -> Option<Rc<WindowsWindowInner>> {
        self.raw_window_handles
            .read()
            .iter()
            .find(|entry| entry.as_raw() == hwnd)
            .and_then(|hwnd| window_from_hwnd(hwnd.as_raw()))
    }

    pub fn window_from_handle(&self, handle: AnyWindowHandle) -> Option<Rc<WindowsWindowInner>> {
        self.raw_window_handles
            .read()
            .iter()
            .filter_map(|entry| window_from_hwnd(entry.as_raw()))
            .find(|window| window.handle == handle)
    }

    #[inline]
    fn post_message(&self, message: u32, wparam: WPARAM, lparam: LPARAM) {
        self.raw_window_handles
            .read()
            .iter()
            .for_each(|handle| unsafe {
                PostMessageW(Some(handle.as_raw()), message, wparam, lparam).log_err();
            });
    }

    fn generate_creation_info(&self) -> WindowCreationInfo {
        WindowCreationInfo {
            icon: self.icon,
            executor: self.foreground_executor.clone(),
            current_cursor: self.inner.state.borrow().current_cursor,
            windows_version: self.windows_version,
            drop_target_helper: self.drop_target_helper.clone(),
            validation_number: self.inner.validation_number,
            main_receiver: self.inner.main_receiver.clone(),
            platform_window_handle: self.handle,
            disable_direct_composition: self.disable_direct_composition,
            directx_devices: (*self.inner.state.borrow().directx_devices).clone(),
            tab_manager_state: self.tab_manager_state.clone(),
            owner_window: None,
        }
    }

    fn set_dock_menus(&self, menus: Vec<MenuItem>) {
        let mut actions = Vec::new();
        menus.into_iter().for_each(|menu| {
            if let Some(dock_menu) = DockMenuItem::new(menu).log_err() {
                actions.push(dock_menu);
            }
        });
        let mut lock = self.inner.state.borrow_mut();
        lock.jump_list.dock_menus = actions;
        update_jump_list(&lock.jump_list).log_err();
    }

    fn update_jump_list(
        &self,
        menus: Vec<MenuItem>,
        entries: Vec<SmallVec<[PathBuf; 2]>>,
    ) -> Vec<SmallVec<[PathBuf; 2]>> {
        let mut actions = Vec::new();
        menus.into_iter().for_each(|menu| {
            if let Some(dock_menu) = DockMenuItem::new(menu).log_err() {
                actions.push(dock_menu);
            }
        });
        let mut lock = self.inner.state.borrow_mut();
        lock.jump_list.dock_menus = actions;
        lock.jump_list.recent_workspaces = entries;
        update_jump_list(&lock.jump_list)
            .log_err()
            .unwrap_or_default()
    }

    fn find_current_active_window(&self) -> Option<HWND> {
        let active_window_hwnd = unsafe { GetActiveWindow() };
        if active_window_hwnd.is_invalid() {
            return None;
        }
        self.raw_window_handles
            .read()
            .iter()
            .find(|hwnd| hwnd.as_raw() == active_window_hwnd)
            .map(|hwnd| hwnd.as_raw())
    }

    fn begin_vsync_thread(&self) {
        let mut directx_device = (*self.inner.state.borrow().directx_devices).clone();
        let platform_window: SafeHwnd = self.handle.into();
        let validation_number = self.inner.validation_number;
        let all_windows = Arc::downgrade(&self.raw_window_handles);
        let text_system = Arc::downgrade(&self.text_system);
        if let Err(error) = std::thread::Builder::new()
            .name("VSyncProvider".to_owned())
            .spawn(move || {
                let mut vsync_provider = VSyncProvider::new();
                loop {
                    vsync_provider.wait_for_vsync();
                    if check_device_lost(&directx_device.device) {
                        handle_gpu_device_lost(
                            &mut directx_device,
                            platform_window.as_raw(),
                            validation_number,
                            &all_windows,
                            &text_system,
                        );
                    }
                    let Some(all_windows) = all_windows.upgrade() else {
                        break;
                    };
                    for hwnd in all_windows.read().iter() {
                        unsafe {
                            let _ = RedrawWindow(Some(hwnd.as_raw()), None, None, RDW_INVALIDATE);
                        }
                    }
                }
            })
        {
            log::error!("failed to start Windows vsync provider: {error}");
        }
    }
}

impl Platform for WindowsPlatform {
    fn background_executor(&self) -> BackgroundExecutor {
        self.background_executor.clone()
    }

    fn foreground_executor(&self) -> ForegroundExecutor {
        self.foreground_executor.clone()
    }

    fn text_system(&self) -> Arc<dyn PlatformTextSystem> {
        self.text_system.clone()
    }

    fn keyboard_layout(&self) -> Box<dyn PlatformKeyboardLayout> {
        Box::new(
            WindowsKeyboardLayout::new()
                .log_err()
                .unwrap_or(WindowsKeyboardLayout::unknown()),
        )
    }

    fn keyboard_mapper(&self) -> Rc<dyn PlatformKeyboardMapper> {
        Rc::new(WindowsKeyboardMapper::new())
    }

    fn on_keyboard_layout_change(&self, callback: Box<dyn FnMut()>) {
        self.inner
            .state
            .borrow_mut()
            .callbacks
            .keyboard_layout_change = Some(callback);
    }

    fn run(&self, on_finish_launching: Box<dyn 'static + FnOnce()>) {
        super::catch_platform_callback("finish launching", (), on_finish_launching);
        self.begin_vsync_thread();

        let mut msg = MSG::default();
        unsafe {
            while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                DispatchMessageW(&msg);
            }
        }

        if let Some(ref mut callback) = self.inner.state.borrow_mut().callbacks.quit {
            super::catch_platform_callback("application quit", (), callback);
        }
    }

    fn quit(&self) {
        self.foreground_executor()
            .spawn(async { unsafe { PostQuitMessage(0) } })
            .detach();
    }

    fn restart(&self, binary_path: Option<PathBuf>) {
        let pid = std::process::id();
        let Some(app_path) = binary_path.or(self.app_path().log_err()) else {
            return;
        };
        let script = format!(
            r#"
            $pidToWaitFor = {}
            $exePath = "{}"

            while ($true) {{
                $process = Get-Process -Id $pidToWaitFor -ErrorAction SilentlyContinue
                if (-not $process) {{
                    Start-Process -FilePath $exePath
                    break
                }}
                Start-Sleep -Seconds 0.1
            }}
            "#,
            pid,
            app_path.display(),
        );

        #[allow(
            clippy::disallowed_methods,
            reason = "We are restarting ourselves, using std command thus is fine"
        )]
        let restart_process = util::command::new_std_command("powershell.exe")
            .arg("-command")
            .arg(script)
            .spawn();

        match restart_process {
            Ok(_) => self.quit(),
            Err(e) => log::error!("failed to spawn restart script: {:?}", e),
        }
    }

    fn activate(&self, _ignoring_other_apps: bool) {}

    fn hide(&self) {}

    fn hide_other_apps(&self) {
        let current_pid = unsafe { windows::Win32::System::Threading::GetCurrentProcessId() };
        let mut minimized: Vec<HWND> = Vec::new();

        unsafe {
            let _ = EnumWindows(
                Some(enum_minimize_foreign_windows),
                LPARAM(&mut (current_pid, &mut minimized) as *mut (u32, &mut Vec<HWND>) as isize),
            );
        }

        self.inner.state.borrow_mut().hidden_app_windows = minimized;
    }

    fn unhide_other_apps(&self) {
        let windows_to_restore =
            std::mem::take(&mut self.inner.state.borrow_mut().hidden_app_windows);

        for hwnd in windows_to_restore {
            unsafe {
                if IsWindow(Some(hwnd)).as_bool() {
                    let _ = ShowWindow(hwnd, SW_RESTORE);
                }
            }
        }
    }

    fn displays(&self) -> Vec<Rc<dyn PlatformDisplay>> {
        WindowsDisplay::displays()
    }

    fn primary_display(&self) -> Option<Rc<dyn PlatformDisplay>> {
        WindowsDisplay::primary_monitor().map(|display| Rc::new(display) as Rc<dyn PlatformDisplay>)
    }

    #[cfg(feature = "screen-capture")]
    fn is_screen_capture_supported(&self) -> bool {
        true
    }

    #[cfg(feature = "screen-capture")]
    fn screen_capture_sources(
        &self,
    ) -> oneshot::Receiver<Result<Vec<Rc<dyn ScreenCaptureSource>>>> {
        crate::platform::scap_screen_capture::scap_screen_sources(&self.foreground_executor)
    }

    fn active_window(&self) -> Option<AnyWindowHandle> {
        let active_window_hwnd = unsafe { GetActiveWindow() };
        self.window_from_hwnd(active_window_hwnd)
            .map(|inner| inner.handle)
    }

    fn open_window(
        &self,
        handle: AnyWindowHandle,
        options: WindowParams,
    ) -> Result<Box<dyn PlatformWindow>> {
        let owner_window = options.parent.and_then(|parent| {
            let owner = self.window_from_handle(parent).map(|window| window.hwnd);
            if owner.is_none() {
                log::warn!(
                    "Windows: unable to resolve explicit parent window {:?}",
                    parent
                );
            }
            owner
        });
        let mut creation_info = self.generate_creation_info();
        creation_info.owner_window = owner_window;

        let window = WindowsWindow::new(handle, options, creation_info)?;
        let handle = window.get_raw_handle();
        self.raw_window_handles.write().push(handle.into());

        Ok(Box::new(window))
    }

    fn window_appearance(&self) -> WindowAppearance {
        system_appearance().log_err().unwrap_or_default()
    }

    fn open_url(&self, url: &str) {
        if url.is_empty() {
            return;
        }
        let url_string = url.to_string();
        self.background_executor()
            .spawn(async move {
                open_target(&url_string)
                    .with_context(|| format!("Opening url: {}", url_string))
                    .log_err();
            })
            .detach();
    }

    fn on_open_urls(&self, callback: Box<dyn FnMut(Vec<String>)>) {
        self.inner.state.borrow_mut().callbacks.open_urls = Some(callback);
    }

    fn prompt_for_paths(
        &self,
        options: PathPromptOptions,
    ) -> Receiver<Result<Option<Vec<PathBuf>>>> {
        let (tx, rx) = oneshot::channel();
        let window = self.find_current_active_window();
        self.foreground_executor()
            .spawn(async move {
                let _ = tx.send(file_open_dialog(options, window));
            })
            .detach();

        rx
    }

    fn prompt_for_new_path(
        &self,
        directory: &Path,
        suggested_name: Option<&str>,
    ) -> Receiver<Result<Option<PathBuf>>> {
        let directory = directory.to_owned();
        let suggested_name = suggested_name.map(|s| s.to_owned());
        let (tx, rx) = oneshot::channel();
        let window = self.find_current_active_window();
        self.foreground_executor()
            .spawn(async move {
                let _ = tx.send(file_save_dialog(directory, suggested_name, window));
            })
            .detach();

        rx
    }

    fn can_select_mixed_files_and_dirs(&self) -> bool {
        // The FOS_PICKFOLDERS flag toggles between "only files" and "only folders".
        false
    }

    fn reveal_path(&self, path: &Path) {
        if path.as_os_str().is_empty() {
            return;
        }
        let path = path.to_path_buf();
        self.background_executor()
            .spawn(async move {
                open_target_in_explorer(&path)
                    .with_context(|| format!("Revealing path {} in explorer", path.display()))
                    .log_err();
            })
            .detach();
    }

    fn open_with_system(&self, path: &Path) {
        if path.as_os_str().is_empty() {
            return;
        }
        let path = path.to_path_buf();
        self.background_executor()
            .spawn(async move {
                open_target(&path)
                    .with_context(|| format!("Opening {} with system", path.display()))
                    .log_err();
            })
            .detach();
    }

    fn move_path_to_trash(&self, path: &Path) -> Result<()> {
        move_path_to_recycle_bin(path)
    }

    fn on_quit(&self, callback: Box<dyn FnMut()>) {
        self.inner.state.borrow_mut().callbacks.quit = Some(callback);
    }

    fn on_reopen(&self, callback: Box<dyn FnMut()>) {
        self.inner.state.borrow_mut().callbacks.reopen = Some(callback);
    }

    fn set_menus(&self, menus: Vec<Menu>, _keymap: &Keymap) {
        self.inner.state.borrow_mut().menus = menus.into_iter().map(|menu| menu.owned()).collect();
    }

    fn get_menus(&self) -> Option<Vec<OwnedMenu>> {
        Some(self.inner.state.borrow().menus.clone())
    }

    fn set_dock_menu(&self, menus: Vec<MenuItem>, _keymap: &Keymap) {
        self.set_dock_menus(menus);
    }

    fn dock_menu_action_count(&self) -> Option<usize> {
        Some(self.inner.state.borrow().jump_list.dock_menus.len())
    }

    fn add_recent_document(&self, path: &Path) {
        let wide_path: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        unsafe {
            SHAddToRecentDocs(SHARD_PATHW.0 as u32, Some(wide_path.as_ptr() as *const _));
        }
    }

    fn clear_recent_documents(&self) {
        unsafe {
            SHAddToRecentDocs(SHARD_PATHW.0 as u32, None);
        }
    }

    fn set_dock_badge(&self, label: Option<&str>) {
        let Some(hwnd) = self.find_current_active_window() else {
            return;
        };
        unsafe {
            let taskbar: std::result::Result<ITaskbarList3, windows::core::Error> =
                CoCreateInstance(&TaskbarList, None, CLSCTX_INPROC_SERVER);
            let Ok(taskbar) = taskbar else { return };
            if taskbar.HrInit().is_err() {
                return;
            }

            match label {
                Some(text) if !text.is_empty() => {
                    if let Some(icon) = create_badge_icon(text) {
                        let _ = taskbar.SetOverlayIcon(hwnd, icon, None);
                        let _ = DestroyIcon(icon);
                    }
                }
                _ => {
                    // Clear the overlay icon by passing a default (empty) HICON
                    let _ = taskbar.SetOverlayIcon(hwnd, HICON::default(), None);
                }
            }
        }
    }

    fn on_app_menu_action(&self, callback: Box<dyn FnMut(&dyn Action)>) {
        self.inner.state.borrow_mut().callbacks.app_menu_action = Some(callback);
    }

    fn on_will_open_app_menu(&self, callback: Box<dyn FnMut()>) {
        self.inner.state.borrow_mut().callbacks.will_open_app_menu = Some(callback);
    }

    fn on_validate_app_menu_command(&self, callback: Box<dyn FnMut(&dyn Action) -> bool>) {
        self.inner
            .state
            .borrow_mut()
            .callbacks
            .validate_app_menu_command = Some(callback);
    }

    fn app_path(&self) -> Result<PathBuf> {
        Ok(std::env::current_exe()?)
    }

    fn path_for_auxiliary_executable(&self, name: &str) -> Result<PathBuf> {
        // Search the application's directory first
        if let Ok(app_path) = self.app_path() {
            if let Some(app_dir) = app_path.parent() {
                let candidate = app_dir.join(name);
                if candidate.exists() {
                    return Ok(candidate);
                }
                // Try with .exe extension if not already present
                if !name.ends_with(".exe") {
                    let candidate_exe = app_dir.join(format!("{name}.exe"));
                    if candidate_exe.exists() {
                        return Ok(candidate_exe);
                    }
                }
            }
        }

        // Fall back to PATH environment variable lookup
        if let Some(path_var) = std::env::var_os("PATH") {
            for dir in std::env::split_paths(&path_var) {
                let candidate = dir.join(name);
                if candidate.exists() {
                    return Ok(candidate);
                }
                if !name.ends_with(".exe") {
                    let candidate_exe = dir.join(format!("{name}.exe"));
                    if candidate_exe.exists() {
                        return Ok(candidate_exe);
                    }
                }
            }
        }

        anyhow::bail!("could not find auxiliary executable '{name}'")
    }

    fn set_cursor_style(&self, style: CursorStyle) {
        let hcursor = load_cursor(style);
        let mut lock = self.inner.state.borrow_mut();
        if lock.current_cursor.map(|c| c.0) != hcursor.map(|c| c.0) {
            self.post_message(
                WM_GPUI_CURSOR_STYLE_CHANGED,
                WPARAM(0),
                LPARAM(hcursor.map_or(0, |c| c.0 as isize)),
            );
            lock.current_cursor = hcursor;
        }
    }

    fn should_auto_hide_scrollbars(&self) -> bool {
        should_auto_hide_scrollbars().log_err().unwrap_or(false)
    }

    fn write_to_clipboard(&self, item: ClipboardItem) {
        write_to_clipboard(item);
    }

    fn clear_clipboard(&self) {
        clear_clipboard();
    }

    fn read_from_clipboard(&self) -> Option<ClipboardItem> {
        read_from_clipboard()
    }

    fn write_credentials(&self, url: &str, username: &str, password: &[u8]) -> Task<Result<()>> {
        let mut password = password.to_vec();
        let mut username = username.encode_utf16().chain(Some(0)).collect_vec();
        let mut target_name = windows_credentials_target_name(url)
            .encode_utf16()
            .chain(Some(0))
            .collect_vec();
        self.foreground_executor().spawn(async move {
            let credentials = CREDENTIALW {
                LastWritten: unsafe { GetSystemTimeAsFileTime() },
                Flags: CRED_FLAGS(0),
                Type: CRED_TYPE_GENERIC,
                TargetName: PWSTR::from_raw(target_name.as_mut_ptr()),
                CredentialBlobSize: password.len() as u32,
                CredentialBlob: password.as_ptr() as *mut _,
                Persist: CRED_PERSIST_LOCAL_MACHINE,
                UserName: PWSTR::from_raw(username.as_mut_ptr()),
                ..CREDENTIALW::default()
            };
            unsafe { CredWriteW(&credentials, 0) }?;
            Ok(())
        })
    }

    fn read_credentials(&self, url: &str) -> Task<Result<Option<(String, Vec<u8>)>>> {
        let mut target_name = windows_credentials_target_name(url)
            .encode_utf16()
            .chain(Some(0))
            .collect_vec();
        self.foreground_executor().spawn(async move {
            let mut credentials: *mut CREDENTIALW = std::ptr::null_mut();
            unsafe {
                CredReadW(
                    PCWSTR::from_raw(target_name.as_ptr()),
                    CRED_TYPE_GENERIC,
                    None,
                    &mut credentials,
                )?
            };

            if credentials.is_null() {
                Ok(None)
            } else {
                let credentials = CredentialAllocation(credentials);
                let credential = unsafe { &*credentials.0 };
                let username: String = unsafe { credential.UserName.to_string()? };
                let blob_size = credential.CredentialBlobSize as usize;
                anyhow::ensure!(
                    blob_size <= MAX_CREDENTIAL_BLOB_BYTES,
                    "credential blob exceeds {MAX_CREDENTIAL_BLOB_BYTES} bytes"
                );
                let password = if blob_size == 0 {
                    Vec::new()
                } else {
                    anyhow::ensure!(
                        !credential.CredentialBlob.is_null(),
                        "credential blob pointer is null"
                    );
                    unsafe {
                        std::slice::from_raw_parts(credential.CredentialBlob, blob_size).to_vec()
                    }
                };
                Ok(Some((username, password)))
            }
        })
    }

    fn delete_credentials(&self, url: &str) -> Task<Result<()>> {
        let mut target_name = windows_credentials_target_name(url)
            .encode_utf16()
            .chain(Some(0))
            .collect_vec();
        self.foreground_executor().spawn(async move {
            unsafe {
                CredDeleteW(
                    PCWSTR::from_raw(target_name.as_ptr()),
                    CRED_TYPE_GENERIC,
                    None,
                )?
            };
            Ok(())
        })
    }

    fn register_url_scheme(&self, scheme: &str) -> Task<anyhow::Result<()>> {
        let scheme = scheme.to_string();
        Task::ready((|| {
            let exe_path = std::env::current_exe()
                .context("Failed to determine application executable path")?;
            let exe_str = exe_path.to_string_lossy();

            let classes_key_path = format!("Software\\Classes\\{scheme}");

            // Create or open the scheme key under HKCU\Software\Classes\{scheme}
            let scheme_key = windows_registry::CURRENT_USER
                .create(&classes_key_path)
                .with_context(|| {
                    format!(
                        "Failed to create registry key HKCU\\{classes_key_path}. \
                         Ensure the current user has write access to HKEY_CURRENT_USER."
                    )
                })?;

            // Set the default value to describe the protocol
            scheme_key
                .set_string("", &format!("URL:{scheme} Protocol"))
                .with_context(|| {
                    format!("Failed to set default value for HKCU\\{classes_key_path}")
                })?;

            // Mark this key as a URL protocol handler
            scheme_key.set_string("URL Protocol", "").with_context(|| {
                format!("Failed to set URL Protocol value for HKCU\\{classes_key_path}")
            })?;

            // Create shell\open\command subkey with the executable path
            let command_key_path = format!("{classes_key_path}\\shell\\open\\command");
            let command_key = windows_registry::CURRENT_USER
                .create(&command_key_path)
                .with_context(|| {
                    format!("Failed to create registry key HKCU\\{command_key_path}")
                })?;

            // Set the command to launch the app with the URL as an argument
            command_key
                .set_string("", &format!("\"{exe_str}\" \"%1\""))
                .with_context(|| {
                    format!("Failed to set command value for HKCU\\{command_key_path}")
                })?;

            Ok(())
        })())
    }

    fn perform_dock_menu_action(&self, action: usize) {
        unsafe {
            PostMessageW(
                Some(self.handle),
                WM_GPUI_DOCK_MENU_ACTION,
                WPARAM(self.inner.validation_number),
                LPARAM(action as isize),
            )
            .log_err();
        }
    }

    fn update_jump_list(
        &self,
        menus: Vec<MenuItem>,
        entries: Vec<SmallVec<[PathBuf; 2]>>,
    ) -> Vec<SmallVec<[PathBuf; 2]>> {
        self.update_jump_list(menus, entries)
    }

    fn set_keep_alive_without_windows(&self, keep_alive: bool) {
        self.inner
            .keep_alive_without_windows
            .store(keep_alive, Ordering::Release);
    }

    fn set_tray_icon(&self, icon: Option<&[u8]>) {
        let mut state = self.inner.state.borrow_mut();
        if let Some(ref mut tray) = state.tray {
            tray.set_icon(icon, self.handle);
        } else if icon.is_some() {
            let mut tray = WindowsTray::new(self.handle);
            tray.set_icon(icon, self.handle);
            state.tray = Some(tray);
        }
    }

    fn set_tray_menu(&self, menu: Vec<TrayMenuItem>) {
        let mut state = self.inner.state.borrow_mut();
        if let Some(ref mut tray) = state.tray {
            tray.menu_items = menu;
        }
    }

    fn set_tray_tooltip(&self, tooltip: &str) {
        let mut state = self.inner.state.borrow_mut();
        if let Some(ref mut tray) = state.tray {
            tray.set_tooltip(tooltip, self.handle);
        }
    }

    fn set_tray_panel_mode(&self, enabled: bool) {
        let mut state = self.inner.state.borrow_mut();
        if state.tray.is_none() {
            state.tray = Some(WindowsTray::new(self.handle));
        }
        if let Some(ref mut tray) = state.tray {
            tray.set_panel_mode(enabled);
        }
    }

    fn get_tray_icon_bounds(&self) -> Option<Bounds<Pixels>> {
        let state = self.inner.state.borrow();
        state.tray.as_ref().and_then(|tray| tray.get_icon_bounds())
    }

    fn on_tray_icon_event(&self, callback: Box<dyn FnMut(TrayIconEvent)>) {
        let mut state = self.inner.state.borrow_mut();
        state.callbacks.tray_icon_event = Some(callback);
    }

    fn on_tray_menu_action(&self, callback: Box<dyn FnMut(SharedString)>) {
        let mut state = self.inner.state.borrow_mut();
        state.callbacks.tray_menu_action = Some(callback);
    }

    fn register_global_hotkey(&self, id: u32, keystroke: &Keystroke) -> Result<()> {
        if let Ok(vk) = super::global_hotkey::virtual_key_for_keystroke(keystroke) {
            self.inner.state.borrow_mut().hotkey_vk_map.insert(id, vk);
        }
        super::global_hotkey::register(self.handle, id, keystroke)
    }

    fn unregister_global_hotkey(&self, id: u32) {
        {
            let mut state = self.inner.state.borrow_mut();
            state.hotkey_vk_map.remove(&id);
            state.pressed_hotkeys.remove(&id);
        }
        super::global_hotkey::unregister(self.handle, id);
    }

    fn on_global_hotkey(&self, callback: Box<dyn FnMut(u32)>) {
        self.inner.state.borrow_mut().callbacks.global_hotkey = Some(callback);
    }

    fn on_global_hotkey_up(&self, callback: Box<dyn FnMut(u32)>) {
        self.inner.state.borrow_mut().callbacks.global_hotkey_up = Some(callback);
    }

    fn focused_window_info(&self) -> Option<FocusedWindowInfo> {
        super::active_window::get_focused_window_info()
    }

    fn set_auto_launch(&self, app_id: &str, enabled: bool) -> Result<()> {
        super::auto_launch::set_auto_launch(app_id, enabled)
    }

    fn is_auto_launch_enabled(&self, app_id: &str) -> bool {
        super::auto_launch::is_auto_launch_enabled(app_id)
    }

    fn show_notification(&self, title: &str, body: &str) -> Result<()> {
        let mut state = self.inner.state.borrow_mut();
        if state.tray.is_none() {
            let tray = WindowsTray::new(self.handle);
            state.tray = Some(tray);
        }
        if let Some(ref tray) = state.tray {
            tray.show_balloon(title, body, self.handle)
        } else {
            Err(anyhow!("Failed to create tray for notification"))
        }
    }

    fn show_notification_with_actions(
        &self,
        title: &str,
        body: &str,
        actions: &[NotificationAction],
        callback: Box<dyn FnMut(String)>,
    ) -> Result<()> {
        use windows::Data::Xml::Dom::XmlDocument;
        use windows::UI::Notifications::{
            ToastActivatedEventArgs, ToastNotification, ToastNotificationManager,
        };

        // Build the toast XML template with action buttons
        let mut xml = String::from(
            "<toast activationType=\"foreground\">\
             <visual><binding template=\"ToastGeneric\">\
             <text></text>\
             <text></text>\
             </binding></visual>",
        );
        if !actions.is_empty() {
            xml.push_str("<actions>");
            for action in actions {
                xml.push_str(&format!(
                    "<action content=\"{}\" arguments=\"{}\" activationType=\"foreground\"/>",
                    escape_xml(&action.label),
                    escape_xml(&action.id),
                ));
            }
            xml.push_str("</actions>");
        }
        xml.push_str("</toast>");

        let xml_doc = XmlDocument::new()?;
        let hxml: HSTRING = xml.into();
        xml_doc.LoadXml(&hxml)?;

        // Set title and body text via the text elements
        let text_nodes = xml_doc.GetElementsByTagName(&HSTRING::from("text"))?;
        if let Ok(title_node) = text_nodes.Item(0) {
            title_node.SetInnerText(&HSTRING::from(title))?;
        }
        if let Ok(body_node) = text_nodes.Item(1) {
            body_node.SetInnerText(&HSTRING::from(body))?;
        }

        let toast = ToastNotification::CreateToastNotification(&xml_doc)?;

        // Store the callback for later invocation on the main thread
        self.inner.state.borrow_mut().callbacks.notification_action = Some(callback);

        // The Activated handler runs on a COM thread, so we push the action ID
        // into a shared queue and post a message to the main thread's HWND.
        let pending = self
            .inner
            .state
            .borrow()
            .pending_notification_actions
            .clone();
        let hwnd_raw = self.handle.0 as usize;
        let validation_number = self.inner.validation_number;

        toast.Activated(&windows::Foundation::TypedEventHandler::<
            windows::UI::Notifications::ToastNotification,
            windows_core::IInspectable,
        >::new(move |_sender, args| {
            if let Some(inspectable) = args.as_ref() {
                if let Ok(activated_args) = inspectable.cast::<ToastActivatedEventArgs>() {
                    if let Ok(arguments) = activated_args.Arguments() {
                        let action_id = arguments.to_string();
                        if let Ok(mut queue) = pending.lock() {
                            queue.push(action_id);
                        }
                        unsafe {
                            let _ = PostMessageW(
                                Some(HWND(hwnd_raw as *mut _)),
                                WM_GPUI_NOTIFICATION_ACTION,
                                WPARAM(validation_number),
                                LPARAM(0),
                            );
                        }
                    }
                }
            }
            Ok(())
        }))?;

        // Use a generic app ID for the toast notifier
        let notifier = ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(
            "GPUI.Application",
        ))?;
        notifier.Show(&toast)?;

        Ok(())
    }

    fn on_system_power_event(&self, callback: Box<dyn FnMut(SystemPowerEvent)>) {
        self.inner.state.borrow_mut().callbacks.system_power = Some(callback);
    }

    fn start_power_save_blocker(&self, kind: PowerSaveBlockerKind) -> Option<u32> {
        let mut state = self.inner.state.borrow_mut();
        let id = state.next_blocker_id;
        state.next_blocker_id += 1;
        let flags = super::power::power_save_flags(kind);
        state.power_save_blockers.insert(id, flags);
        super::power::apply_combined_power_state(&state.power_save_blockers);
        Some(id)
    }

    fn stop_power_save_blocker(&self, id: u32) {
        let mut state = self.inner.state.borrow_mut();
        state.power_save_blockers.remove(&id);
        super::power::apply_combined_power_state(&state.power_save_blockers);
    }

    fn power_mode(&self) -> PowerMode {
        super::power::power_mode()
    }

    fn system_idle_time(&self) -> Option<Duration> {
        super::power::system_idle_time()
    }

    fn network_status(&self) -> NetworkStatus {
        super::network::query_network_status()
    }

    fn on_network_status_change(&self, callback: Box<dyn FnMut(NetworkStatus)>) {
        self.inner
            .state
            .borrow_mut()
            .callbacks
            .network_status_change = Some(callback);
        super::network::start_network_monitoring(self.handle, self.inner.validation_number);
    }

    fn on_media_key_event(&self, callback: Box<dyn FnMut(MediaKeyEvent)>) {
        self.inner.state.borrow_mut().callbacks.media_key = Some(callback);
    }

    fn request_user_attention(&self, attention_type: AttentionType) {
        let hwnd = self
            .find_current_active_window()
            .or_else(|| self.raw_window_handles.read().first().map(|h| h.as_raw()));
        let Some(hwnd) = hwnd else { return };

        let (flags, count) = match attention_type {
            AttentionType::Informational => (FLASHW_TRAY | FLASHW_TIMERNOFG, 3u32),
            AttentionType::Critical => (FLASHW_ALL | FLASHW_TIMER, 0u32),
        };
        let mut fi = FLASHWINFO {
            cbSize: std::mem::size_of::<FLASHWINFO>() as u32,
            hwnd,
            dwFlags: flags,
            uCount: count,
            dwTimeout: 0,
        };
        unsafe {
            let _ = FlashWindowEx(&mut fi);
        }
        self.inner.state.borrow_mut().flashing_hwnd = Some(hwnd);
    }

    fn cancel_user_attention(&self) {
        let hwnd = self.inner.state.borrow_mut().flashing_hwnd.take();
        if let Some(hwnd) = hwnd {
            let mut fi = FLASHWINFO {
                cbSize: std::mem::size_of::<FLASHWINFO>() as u32,
                hwnd,
                dwFlags: FLASHW_STOP,
                uCount: 0,
                dwTimeout: 0,
            };
            unsafe {
                let _ = FlashWindowEx(&mut fi);
            }
        }
    }

    fn show_context_menu(
        &self,
        position: Point<Pixels>,
        items: Vec<TrayMenuItem>,
        callback: Box<dyn FnMut(SharedString)>,
    ) {
        {
            let mut state = self.inner.state.borrow_mut();
            state.callbacks.context_menu = Some(callback);
            state.context_menu_command_map.clear();
        }

        unsafe {
            let hmenu = match CreatePopupMenu() {
                Ok(m) => m,
                Err(_) => return,
            };

            {
                let mut state = self.inner.state.borrow_mut();
                let mut counter: u32 = 10000;
                WindowsTray::build_menu(
                    hmenu,
                    &items,
                    &mut counter,
                    &mut state.context_menu_command_map,
                );
            }

            let screen_x = position.x.0 as i32;
            let screen_y = position.y.0 as i32;

            let _ = SetForegroundWindow(self.handle);
            let result = TrackPopupMenu(
                hmenu,
                TPM_LEFTALIGN | TPM_TOPALIGN | TPM_RETURNCMD,
                screen_x,
                screen_y,
                None,
                self.handle,
                None,
            );
            let _ = DestroyMenu(hmenu);

            if result.as_bool() {
                if result.0 != 0 {
                    PostMessageW(
                        Some(self.handle),
                        WM_GPUI_CONTEXT_MENU_ACTION,
                        WPARAM(self.inner.validation_number),
                        LPARAM(result.0 as isize),
                    )
                    .log_err();
                }
            }
        }
    }

    fn show_dialog(&self, options: DialogOptions) -> oneshot::Receiver<usize> {
        let (tx, rx) = oneshot::channel();
        let hwnd = self.find_current_active_window().unwrap_or(self.handle);
        self.foreground_executor()
            .spawn(async move {
                let _ = tx.send(super::dialog::show_dialog_sync(hwnd, options));
            })
            .detach();
        rx
    }

    fn os_info(&self) -> OsInfo {
        super::os_info::get_os_info()
    }

    fn accessibility_status(&self) -> PermissionStatus {
        // On Windows, accessibility (UIA) is always available without special permissions.
        // Screen readers and automation tools can connect to any UIA provider.
        PermissionStatus::Granted
    }

    fn biometric_status(&self) -> BiometricStatus {
        super::biometric::biometric_status()
    }

    fn authenticate_biometric(&self, reason: &str, callback: Box<dyn FnOnce(bool) + Send>) {
        let reason = reason.to_string();
        self.background_executor()
            .spawn(async move {
                super::biometric::authenticate_biometric(&reason, callback);
            })
            .detach();
    }

    fn microphone_status(&self) -> PermissionStatus {
        super::microphone::microphone_status()
    }

    fn request_microphone_permission(&self, callback: Box<dyn FnOnce(bool)>) {
        // request_microphone_permission triggers the same MediaCapture initialization
        // flow. On Windows, the OS consent prompt (if needed) appears automatically
        // during initialization. We call it synchronously since the trait callback
        // is not Send.
        let status = super::microphone::microphone_status();
        super::catch_platform_callback("microphone permission", (), || {
            callback(status == PermissionStatus::Granted)
        });
    }
}

impl WindowsPlatformInner {
    fn new(context: &mut PlatformWindowCreateContext) -> Result<Rc<Self>> {
        let directx_devices = context
            .directx_devices
            .take()
            .ok_or_else(|| anyhow!("platform window DirectX state is missing"))?;
        let main_receiver = context
            .main_receiver
            .take()
            .ok_or_else(|| anyhow!("platform window task receiver is missing"))?;
        let state = RefCell::new(WindowsPlatformState::new(directx_devices));
        Ok(Rc::new(Self {
            state,
            raw_window_handles: context.raw_window_handles.clone(),
            keep_alive_without_windows: AtomicBool::new(false),
            validation_number: context.validation_number,
            main_receiver,
        }))
    }

    fn handle_msg(
        self: &Rc<Self>,
        handle: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        let handled = match msg {
            WM_GPUI_CLOSE_ONE_WINDOW
            | WM_GPUI_TASK_DISPATCHED_ON_MAIN_THREAD
            | WM_GPUI_DOCK_MENU_ACTION
            | WM_GPUI_KEYBOARD_LAYOUT_CHANGED
            | WM_GPUI_GPU_DEVICE_LOST => self.handle_gpui_events(msg, wparam, lparam),
            WM_GPUI_TRAY_ICON => self.handle_tray_icon_event(handle, lparam),
            WM_COMMAND => self.handle_tray_menu_command(wparam),
            WM_HOTKEY => self.handle_global_hotkey(wparam),
            WM_KEYUP | WM_SYSKEYUP => self.handle_global_hotkey_up(wparam),
            WM_POWERBROADCAST => self.handle_power_broadcast(wparam),
            super::events::WM_WTSSESSION_CHANGE => self.handle_session_change(wparam),
            WM_GPUI_NETWORK_CHANGE => self.handle_network_change(),
            WM_GPUI_MEDIA_KEY => self.handle_media_key(wparam, lparam),
            WM_GPUI_CONTEXT_MENU_ACTION => self.handle_context_menu_action(wparam, lparam),
            WM_GPUI_NOTIFICATION_ACTION => self.handle_notification_action(wparam),
            _ => None,
        };
        if let Some(result) = handled {
            LRESULT(result)
        } else {
            unsafe { DefWindowProcW(handle, msg, wparam, lparam) }
        }
    }

    fn handle_gpui_events(&self, message: u32, wparam: WPARAM, lparam: LPARAM) -> Option<isize> {
        if wparam.0 != self.validation_number {
            log::error!("Wrong validation number while processing message: {message}");
            return None;
        }
        match message {
            WM_GPUI_CLOSE_ONE_WINDOW => {
                if self.close_one_window(HWND(lparam.0 as _)) {
                    unsafe { PostQuitMessage(0) };
                }
                Some(0)
            }
            WM_GPUI_TASK_DISPATCHED_ON_MAIN_THREAD => self.run_foreground_task(),
            WM_GPUI_DOCK_MENU_ACTION => self.handle_dock_action_event(lparam.0 as _),
            WM_GPUI_KEYBOARD_LAYOUT_CHANGED => self.handle_keyboard_layout_change(),
            WM_GPUI_GPU_DEVICE_LOST => self.handle_device_lost(lparam),
            _ => {
                log::warn!("ignoring unknown internal Windows platform message: {message}");
                None
            }
        }
    }

    fn close_one_window(&self, target_window: HWND) -> bool {
        let Some(all_windows) = self.raw_window_handles.upgrade() else {
            log::error!("Failed to upgrade raw window handles");
            return false;
        };
        let mut lock = all_windows.write();
        let Some(index) = lock
            .iter()
            .position(|handle| handle.as_raw() == target_window)
        else {
            log::warn!("ignoring close notification for an unknown Windows window");
            return false;
        };
        lock.remove(index);

        lock.is_empty() && !self.keep_alive_without_windows.load(Ordering::Acquire)
    }

    #[inline]
    fn run_foreground_task(&self) -> Option<isize> {
        for runnable in self.main_receiver.try_iter() {
            super::catch_platform_callback("foreground task", (), || {
                runnable.run();
            });
        }
        Some(0)
    }

    fn handle_dock_action_event(&self, action_idx: usize) -> Option<isize> {
        let mut lock = self.state.borrow_mut();
        let mut callback = lock.callbacks.app_menu_action.take()?;
        let Some(action) = lock
            .jump_list
            .dock_menus
            .get(action_idx)
            .map(|dock_menu| dock_menu.action.boxed_clone())
        else {
            lock.callbacks.app_menu_action = Some(callback);
            log::error!("Dock menu for index {action_idx} not found");
            return Some(1);
        };
        drop(lock);
        super::catch_platform_callback("dock menu action", (), || callback(&*action));
        self.state.borrow_mut().callbacks.app_menu_action = Some(callback);
        Some(0)
    }

    fn handle_keyboard_layout_change(&self) -> Option<isize> {
        let mut callback = self
            .state
            .borrow_mut()
            .callbacks
            .keyboard_layout_change
            .take()?;
        super::catch_platform_callback("keyboard layout change", (), &mut callback);
        self.state.borrow_mut().callbacks.keyboard_layout_change = Some(callback);
        Some(0)
    }

    fn handle_tray_icon_event(&self, handle: HWND, lparam: LPARAM) -> Option<isize> {
        let event = match (lparam.0 & 0xFFFF) as u32 {
            WM_LBUTTONUP => Some(TrayIconEvent::LeftClick),
            WM_RBUTTONUP => Some(TrayIconEvent::RightClick),
            WM_LBUTTONDBLCLK => Some(TrayIconEvent::DoubleClick),
            _ => None,
        };
        if let Some(event) = event {
            let is_panel_mode = self
                .state
                .borrow()
                .tray
                .as_ref()
                .map_or(false, |t| t.is_panel_mode());

            // In panel mode, skip showing the context menu on right-click;
            // the app handles positioning a panel window instead.
            if event == TrayIconEvent::RightClick && !is_panel_mode {
                let mut tray = self.state.borrow_mut().tray.take();
                if let Some(ref mut t) = tray {
                    t.show_context_menu(handle);
                }
                self.state.borrow_mut().tray = tray;
            }
            let mut callback = self.state.borrow_mut().callbacks.tray_icon_event.take();
            if let Some(ref mut cb) = callback {
                super::catch_platform_callback("tray icon", (), || cb(event));
            }
            self.state.borrow_mut().callbacks.tray_icon_event = callback;
        }
        Some(0)
    }

    fn handle_tray_menu_command(&self, wparam: WPARAM) -> Option<isize> {
        let cmd_id = (wparam.0 & 0xFFFF) as u32;
        let item_id = {
            let state = self.state.borrow();
            state
                .tray
                .as_ref()
                .and_then(|tray| tray.command_id_map.get(&cmd_id).cloned())
        };
        let Some(item_id) = item_id else {
            return None;
        };
        let mut callback = self.state.borrow_mut().callbacks.tray_menu_action.take();
        if let Some(ref mut cb) = callback {
            super::catch_platform_callback("tray menu", (), || cb(item_id));
        }
        self.state.borrow_mut().callbacks.tray_menu_action = callback;
        Some(0)
    }

    fn handle_global_hotkey(&self, wparam: WPARAM) -> Option<isize> {
        let hotkey_id = wparam.0 as u32;
        self.state.borrow_mut().pressed_hotkeys.insert(hotkey_id);
        let mut callback = self.state.borrow_mut().callbacks.global_hotkey.take();
        if let Some(ref mut cb) = callback {
            super::catch_platform_callback("global hotkey down", (), || cb(hotkey_id));
        }
        self.state.borrow_mut().callbacks.global_hotkey = callback;
        Some(0)
    }

    fn handle_global_hotkey_up(&self, wparam: WPARAM) -> Option<isize> {
        let vk = VIRTUAL_KEY(wparam.0 as u16);
        // Find which pressed hotkey ID(s) match this virtual key code.
        let mut released_ids = Vec::new();
        {
            let state = self.state.borrow();
            for &hotkey_id in &state.pressed_hotkeys {
                if let Some(&registered_vk) = state.hotkey_vk_map.get(&hotkey_id) {
                    if registered_vk == vk {
                        released_ids.push(hotkey_id);
                    }
                }
            }
        }
        if released_ids.is_empty() {
            return None;
        }
        for hotkey_id in &released_ids {
            self.state.borrow_mut().pressed_hotkeys.remove(hotkey_id);
        }
        let mut callback = self.state.borrow_mut().callbacks.global_hotkey_up.take();
        if let Some(ref mut cb) = callback {
            for hotkey_id in released_ids {
                super::catch_platform_callback("global hotkey up", (), || cb(hotkey_id));
            }
        }
        self.state.borrow_mut().callbacks.global_hotkey_up = callback;
        Some(0)
    }

    fn handle_device_lost(&self, lparam: LPARAM) -> Option<isize> {
        if lparam.0 == 0 {
            return None;
        }
        let mut lock = self.state.borrow_mut();
        let directx_devices = lparam.0 as *const DirectXDevices;
        let directx_devices = unsafe { &*directx_devices };
        unsafe {
            ManuallyDrop::drop(&mut lock.directx_devices);
        }
        lock.directx_devices = ManuallyDrop::new(directx_devices.clone());

        Some(0)
    }

    fn handle_power_broadcast(&self, wparam: WPARAM) -> Option<isize> {
        const PBT_APMSUSPEND: u32 = 0x0004;
        const PBT_POWERSTATUSCHANGE: u32 = 0x000A;
        const PBT_APMRESUMEAUTOMATIC: u32 = 0x0012;

        let event = match wparam.0 as u32 {
            PBT_APMSUSPEND => Some(SystemPowerEvent::Suspend),
            PBT_POWERSTATUSCHANGE => Some(SystemPowerEvent::PowerModeChanged),
            PBT_APMRESUMEAUTOMATIC => Some(SystemPowerEvent::Resume),
            _ => None,
        };
        if let Some(event) = event {
            let mut lock = self.state.borrow_mut();
            let mut callback = lock.callbacks.system_power.take();
            drop(lock);
            if let Some(ref mut cb) = callback {
                super::catch_platform_callback("system power", (), || cb(event));
            }
            self.state.borrow_mut().callbacks.system_power = callback;
        }
        Some(1)
    }

    fn handle_session_change(&self, wparam: WPARAM) -> Option<isize> {
        const WTS_SESSION_LOCK: u32 = 0x7;
        const WTS_SESSION_UNLOCK: u32 = 0x8;

        let event = match wparam.0 as u32 {
            WTS_SESSION_LOCK => Some(SystemPowerEvent::LockScreen),
            WTS_SESSION_UNLOCK => Some(SystemPowerEvent::UnlockScreen),
            _ => None,
        };
        if let Some(event) = event {
            let mut lock = self.state.borrow_mut();
            let mut callback = lock.callbacks.system_power.take();
            drop(lock);
            if let Some(ref mut cb) = callback {
                super::catch_platform_callback("session power", (), || cb(event));
            }
            self.state.borrow_mut().callbacks.system_power = callback;
        }
        Some(0)
    }

    fn handle_network_change(&self) -> Option<isize> {
        let status = super::network::query_network_status();
        let mut lock = self.state.borrow_mut();
        let mut callback = lock.callbacks.network_status_change.take();
        drop(lock);
        if let Some(ref mut cb) = callback {
            super::catch_platform_callback("network change", (), || cb(status));
        }
        self.state.borrow_mut().callbacks.network_status_change = callback;
        Some(0)
    }

    fn handle_media_key(&self, wparam: WPARAM, lparam: LPARAM) -> Option<isize> {
        if wparam.0 != self.validation_number {
            return None;
        }
        const APPCOMMAND_MEDIA_NEXTTRACK: u32 = 11;
        const APPCOMMAND_MEDIA_PREVIOUSTRACK: u32 = 12;
        const APPCOMMAND_MEDIA_STOP: u32 = 13;
        const APPCOMMAND_MEDIA_PLAY_PAUSE: u32 = 14;
        const APPCOMMAND_MEDIA_PLAY: u32 = 46;
        const APPCOMMAND_MEDIA_PAUSE: u32 = 47;

        let cmd = lparam.0 as u32;
        let event = match cmd {
            APPCOMMAND_MEDIA_PLAY_PAUSE => MediaKeyEvent::PlayPause,
            APPCOMMAND_MEDIA_NEXTTRACK => MediaKeyEvent::NextTrack,
            APPCOMMAND_MEDIA_PREVIOUSTRACK => MediaKeyEvent::PreviousTrack,
            APPCOMMAND_MEDIA_STOP => MediaKeyEvent::Stop,
            APPCOMMAND_MEDIA_PLAY => MediaKeyEvent::Play,
            APPCOMMAND_MEDIA_PAUSE => MediaKeyEvent::Pause,
            _ => return None,
        };
        let mut lock = self.state.borrow_mut();
        let mut callback = lock.callbacks.media_key.take();
        drop(lock);
        if let Some(ref mut cb) = callback {
            super::catch_platform_callback("media key", (), || cb(event));
        }
        self.state.borrow_mut().callbacks.media_key = callback;
        Some(0)
    }

    fn handle_context_menu_action(&self, wparam: WPARAM, lparam: LPARAM) -> Option<isize> {
        if wparam.0 != self.validation_number {
            return None;
        }
        let cmd_id = lparam.0 as u32;
        let item_id = self
            .state
            .borrow()
            .context_menu_command_map
            .get(&cmd_id)
            .cloned();
        let Some(item_id) = item_id else {
            return None;
        };
        let mut lock = self.state.borrow_mut();
        let mut callback = lock.callbacks.context_menu.take();
        drop(lock);
        if let Some(ref mut cb) = callback {
            super::catch_platform_callback("context menu", (), || cb(item_id));
        }
        self.state.borrow_mut().callbacks.context_menu = callback;
        Some(0)
    }

    fn handle_notification_action(&self, wparam: WPARAM) -> Option<isize> {
        if wparam.0 != self.validation_number {
            return None;
        }
        // Drain all pending action IDs from the shared queue
        let action_ids: Vec<String> = {
            let pending = self.state.borrow().pending_notification_actions.clone();
            let mut queue = pending.lock().ok()?;
            queue.drain(..).collect()
        };
        for action_id in action_ids {
            let mut lock = self.state.borrow_mut();
            let mut callback = lock.callbacks.notification_action.take();
            drop(lock);
            if let Some(ref mut cb) = callback {
                super::catch_platform_callback("notification action", (), || cb(action_id));
            }
            self.state.borrow_mut().callbacks.notification_action = callback;
        }
        Some(0)
    }
}

impl Drop for WindowsPlatform {
    fn drop(&mut self) {
        unsafe {
            let _ = windows::Win32::System::RemoteDesktop::WTSUnRegisterSessionNotification(
                self.handle,
            );
            DestroyWindow(self.handle)
                .context("Destroying platform window")
                .log_err();
            OleUninitialize();
        }
    }
}

impl Drop for WindowsPlatformState {
    fn drop(&mut self) {
        if !self.power_save_blockers.is_empty() {
            unsafe {
                windows::Win32::System::Power::SetThreadExecutionState(
                    windows::Win32::System::Power::ES_CONTINUOUS,
                );
            }
        }
        unsafe {
            ManuallyDrop::drop(&mut self.directx_devices);
        }
    }
}

pub(crate) struct WindowCreationInfo {
    pub(crate) icon: HICON,
    pub(crate) executor: ForegroundExecutor,
    pub(crate) current_cursor: Option<HCURSOR>,
    pub(crate) windows_version: WindowsVersion,
    pub(crate) drop_target_helper: IDropTargetHelper,
    pub(crate) validation_number: usize,
    pub(crate) main_receiver: crossbeam_channel::Receiver<Runnable>,
    pub(crate) platform_window_handle: HWND,
    pub(crate) disable_direct_composition: bool,
    pub(crate) directx_devices: DirectXDevices,
    pub(crate) tab_manager_state: Arc<Mutex<TabManagerState>>,
    pub(crate) owner_window: Option<HWND>,
}

struct PlatformWindowCreateContext {
    inner: Option<Result<Rc<WindowsPlatformInner>>>,
    raw_window_handles: std::sync::Weak<RwLock<SmallVec<[SafeHwnd; 4]>>>,
    validation_number: usize,
    main_receiver: Option<crossbeam_channel::Receiver<Runnable>>,
    directx_devices: Option<DirectXDevices>,
}

fn open_target(target: impl AsRef<OsStr>) -> Result<()> {
    let target = target.as_ref();
    let ret = unsafe {
        ShellExecuteW(
            None,
            windows::core::w!("open"),
            &HSTRING::from(target),
            None,
            None,
            SW_SHOWDEFAULT,
        )
    };
    if ret.0 as isize <= 32 {
        Err(anyhow::anyhow!(
            "Unable to open target: {}",
            std::io::Error::last_os_error()
        ))
    } else {
        Ok(())
    }
}

fn move_path_to_recycle_bin(target: &Path) -> Result<()> {
    let operation = if target.is_dir() {
        "DeleteDirectory"
    } else {
        "DeleteFile"
    };
    let script = format!(
        "Add-Type -AssemblyName Microsoft.VisualBasic; \
         [Microsoft.VisualBasic.FileIO.FileSystem]::{operation}(\
         $args[0], \
         [Microsoft.VisualBasic.FileIO.UIOption]::OnlyErrorDialogs, \
         [Microsoft.VisualBasic.FileIO.RecycleOption]::SendToRecycleBin)"
    );
    let status = std::process::Command::new("powershell.exe")
        .arg("-NoProfile")
        .arg("-NonInteractive")
        .arg("-Command")
        .arg(script)
        .arg(target)
        .status()
        .context("invoking PowerShell recycle-bin command")?;
    anyhow::ensure!(status.success(), "Recycle Bin command failed with {status}");
    Ok(())
}

fn open_target_in_explorer(target: &Path) -> Result<()> {
    let dir = target.parent().context("No parent folder found")?;
    let desktop = unsafe { SHGetDesktopFolder()? };

    let mut dir_item = std::ptr::null_mut();
    unsafe {
        desktop.ParseDisplayName(
            HWND::default(),
            None,
            &HSTRING::from(dir),
            None,
            &mut dir_item,
            std::ptr::null_mut(),
        )?;
    }

    let mut file_item = std::ptr::null_mut();
    unsafe {
        desktop.ParseDisplayName(
            HWND::default(),
            None,
            &HSTRING::from(target),
            None,
            &mut file_item,
            std::ptr::null_mut(),
        )?;
    }

    let highlight = [file_item as *const _];
    unsafe { SHOpenFolderAndSelectItems(dir_item as _, Some(&highlight), 0) }.or_else(|err| {
        if err.code().0 == ERROR_FILE_NOT_FOUND.0 as i32 {
            // On some systems, the above call mysteriously fails with "file not
            // found" even though the file is there.  In these cases, ShellExecute()
            // seems to work as a fallback (although it won't select the file).
            open_target(dir).context("Opening target parent folder")
        } else {
            Err(anyhow::anyhow!("Can not open target path: {}", err))
        }
    })
}

fn file_open_dialog(
    options: PathPromptOptions,
    window: Option<HWND>,
) -> Result<Option<Vec<PathBuf>>> {
    let folder_dialog: IFileOpenDialog =
        unsafe { CoCreateInstance(&FileOpenDialog, None, CLSCTX_ALL)? };

    let mut dialog_options = FOS_FILEMUSTEXIST;
    if options.multiple {
        dialog_options |= FOS_ALLOWMULTISELECT;
    }
    if options.directories {
        dialog_options |= FOS_PICKFOLDERS;
    }

    unsafe {
        folder_dialog.SetOptions(dialog_options)?;

        if let Some(prompt) = options.prompt {
            let prompt: &str = &prompt;
            folder_dialog.SetOkButtonLabel(&HSTRING::from(prompt))?;
        }

        if folder_dialog.Show(window).is_err() {
            // User cancelled
            return Ok(None);
        }
    }

    let results = unsafe { folder_dialog.GetResults()? };
    let file_count = unsafe { results.GetCount()? };
    if file_count == 0 {
        return Ok(None);
    }

    let mut paths = Vec::with_capacity(file_count as usize);
    for i in 0..file_count {
        let item = unsafe { results.GetItemAt(i)? };
        let path = unsafe { item.GetDisplayName(SIGDN_FILESYSPATH)?.to_string()? };
        paths.push(PathBuf::from(path));
    }

    Ok(Some(paths))
}

fn file_save_dialog(
    directory: PathBuf,
    suggested_name: Option<String>,
    window: Option<HWND>,
) -> Result<Option<PathBuf>> {
    let dialog: IFileSaveDialog = unsafe { CoCreateInstance(&FileSaveDialog, None, CLSCTX_ALL)? };
    if !directory.to_string_lossy().is_empty()
        && let Some(full_path) = directory.canonicalize().log_err()
    {
        let full_path = SanitizedPath::new(&full_path);
        let full_path_string = full_path.to_string();
        let path_item: IShellItem =
            unsafe { SHCreateItemFromParsingName(&HSTRING::from(full_path_string), None)? };
        unsafe { dialog.SetFolder(&path_item).log_err() };
    }

    if let Some(suggested_name) = suggested_name {
        unsafe { dialog.SetFileName(&HSTRING::from(suggested_name)).log_err() };
    }

    unsafe {
        dialog.SetFileTypes(&[Common::COMDLG_FILTERSPEC {
            pszName: windows::core::w!("All files"),
            pszSpec: windows::core::w!("*.*"),
        }])?;
        if dialog.Show(window).is_err() {
            // User cancelled
            return Ok(None);
        }
    }
    let shell_item = unsafe { dialog.GetResult()? };
    let file_path_string = unsafe {
        let pwstr = shell_item.GetDisplayName(SIGDN_FILESYSPATH)?;
        let string = pwstr.to_string()?;
        CoTaskMemFree(Some(pwstr.0 as _));
        string
    };
    Ok(Some(PathBuf::from(file_path_string)))
}

fn load_icon() -> Result<HICON> {
    let module = unsafe { GetModuleHandleW(None).context("unable to get module handle")? };
    let handle = unsafe {
        LoadImageW(
            Some(module.into()),
            windows::core::PCWSTR(1 as _),
            IMAGE_ICON,
            0,
            0,
            LR_DEFAULTSIZE | LR_SHARED,
        )
        .context("unable to load icon file")?
    };
    Ok(HICON(handle.0))
}

#[inline]
fn should_auto_hide_scrollbars() -> Result<bool> {
    let ui_settings = UISettings::new()?;
    Ok(ui_settings.AutoHideScrollBars()?)
}

fn check_device_lost(device: &ID3D11Device) -> bool {
    let device_state = unsafe { device.GetDeviceRemovedReason() };
    match device_state {
        Ok(_) => false,
        Err(err) => {
            log::error!("DirectX device lost detected: {:?}", err);
            true
        }
    }
}

fn handle_gpu_device_lost(
    directx_devices: &mut DirectXDevices,
    platform_window: HWND,
    validation_number: usize,
    all_windows: &std::sync::Weak<RwLock<SmallVec<[SafeHwnd; 4]>>>,
    text_system: &std::sync::Weak<DirectWriteTextSystem>,
) {
    // Here we wait a bit to ensure the system has time to recover from the device lost state.
    // If we don't wait, the final drawing result will be blank.
    std::thread::sleep(std::time::Duration::from_millis(350));

    try_to_recover_from_device_lost(
        || {
            DirectXDevices::new()
                .context("Failed to recreate new DirectX devices after device lost")
        },
        |new_devices| *directx_devices = new_devices,
        || {
            log::error!("Failed to recover DirectX devices after multiple attempts.");
            // Do something here?
            // At this point, the device loss is considered unrecoverable.
            // std::process::exit(1);
        },
    );
    log::info!("DirectX devices successfully recreated.");

    unsafe {
        SendMessageW(
            platform_window,
            WM_GPUI_GPU_DEVICE_LOST,
            Some(WPARAM(validation_number)),
            Some(LPARAM(directx_devices as *const _ as _)),
        );
    }

    if let Some(text_system) = text_system.upgrade() {
        text_system.handle_gpu_lost(&directx_devices);
    }
    if let Some(all_windows) = all_windows.upgrade() {
        for window in all_windows.read().iter() {
            unsafe {
                SendMessageW(
                    window.as_raw(),
                    WM_GPUI_GPU_DEVICE_LOST,
                    Some(WPARAM(validation_number)),
                    Some(LPARAM(directx_devices as *const _ as _)),
                );
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(200));
        for window in all_windows.read().iter() {
            unsafe {
                SendMessageW(
                    window.as_raw(),
                    WM_GPUI_FORCE_UPDATE_WINDOW,
                    Some(WPARAM(validation_number)),
                    None,
                );
            }
        }
    }
}

/// Callback for `EnumWindows` used by `hide_other_apps`.
/// Minimizes visible top-level windows that belong to other processes.
/// The `lparam` carries a `*mut (u32, &mut Vec<HWND>)` where the `u32` is the
/// current process ID and the `Vec<HWND>` collects the windows that were minimized.
unsafe extern "system" fn enum_minimize_foreign_windows(hwnd: HWND, lparam: LPARAM) -> BOOL {
    if lparam.0 == 0 {
        return BOOL(0);
    }
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
        let data = &mut *(lparam.0 as *mut (u32, &mut Vec<HWND>));
        let (current_pid, ref mut minimized) = *data;

        let mut window_pid: u32 = 0;
        GetWindowThreadProcessId(hwnd, Some(&mut window_pid));

        if window_pid != current_pid && IsWindowVisible(hwnd).as_bool() && !IsIconic(hwnd).as_bool()
        {
            let _ = ShowWindow(hwnd, SW_MINIMIZE);
            minimized.push(hwnd);
        }

        BOOL(1)
    })) {
        Ok(result) => result,
        Err(_) => {
            log::error!("foreign-window enumeration panicked at the Windows ABI boundary");
            BOOL(0)
        }
    }
}

const PLATFORM_WINDOW_CLASS_NAME: PCWSTR = w!("Kael::PlatformWindow");

fn register_platform_window_class() {
    let wc = WNDCLASSW {
        lpfnWndProc: Some(window_procedure),
        lpszClassName: PCWSTR(PLATFORM_WINDOW_CLASS_NAME.as_ptr()),
        ..Default::default()
    };
    unsafe { RegisterClassW(&wc) };
}

unsafe extern "system" fn window_procedure(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
        window_procedure_impl(hwnd, msg, wparam, lparam)
    })) {
        Ok(result) => result,
        Err(_) => {
            log::error!(
                "platform window procedure panicked; containing panic at the Windows ABI boundary"
            );
            if msg == WM_NCDESTROY {
                let ptr = unsafe { get_window_long(hwnd, GWLP_USERDATA) }
                    as *mut Weak<WindowsPlatformInner>;
                if !ptr.is_null() {
                    unsafe { set_window_long(hwnd, GWLP_USERDATA, 0) };
                    unsafe { drop(Box::from_raw(ptr)) };
                }
            }
            LRESULT(0)
        }
    }
}

unsafe fn window_procedure_impl(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if msg == WM_NCCREATE {
        let params = lparam.0 as *const CREATESTRUCTW;
        if params.is_null() {
            return LRESULT(0);
        }
        let params = unsafe { &*params };
        let creation_context = params.lpCreateParams as *mut PlatformWindowCreateContext;
        if creation_context.is_null() {
            return LRESULT(0);
        }
        let creation_context = unsafe { &mut *creation_context };
        return match WindowsPlatformInner::new(creation_context) {
            Ok(inner) => {
                let weak = Box::new(Rc::downgrade(&inner));
                unsafe { set_window_long(hwnd, GWLP_USERDATA, Box::into_raw(weak) as isize) };
                creation_context.inner = Some(Ok(inner));
                unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
            }
            Err(error) => {
                creation_context.inner = Some(Err(error));
                LRESULT(0)
            }
        };
    }

    let ptr = unsafe { get_window_long(hwnd, GWLP_USERDATA) } as *mut Weak<WindowsPlatformInner>;
    if ptr.is_null() {
        return unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) };
    }
    let inner = unsafe { &*ptr };
    let result = if let Some(inner) = inner.upgrade() {
        inner.handle_msg(hwnd, msg, wparam, lparam)
    } else {
        unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
    };

    if msg == WM_NCDESTROY {
        unsafe { set_window_long(hwnd, GWLP_USERDATA, 0) };
        unsafe { drop(Box::from_raw(ptr)) };
    }

    result
}

/// Creates a small 16x16 HICON with the given text rendered as a badge.
/// Used for taskbar overlay icons via `ITaskbarList3::SetOverlayIcon`.
fn create_badge_icon(text: &str) -> Option<HICON> {
    unsafe {
        let hdc_screen = GetDC(None);
        let hdc = CreateCompatibleDC(Some(hdc_screen));
        let hbm = CreateCompatibleBitmap(hdc_screen, 16, 16);
        let old_bm = SelectObject(hdc, hbm.into());

        // Fill background with red circle
        let brush = CreateSolidBrush(COLORREF(0x000000FF)); // Red in BGR
        let old_brush = SelectObject(hdc, brush.into());
        let pen = CreatePen(PS_SOLID, 0, COLORREF(0x000000FF));
        let old_pen = SelectObject(hdc, pen.into());
        let _ = Ellipse(hdc, 0, 0, 16, 16);

        // Draw white text
        SetBkMode(hdc, TRANSPARENT);
        SetTextColor(hdc, COLORREF(0x00FFFFFF)); // White in BGR

        // Create a small font for the badge
        let font = CreateFontW(
            12,               // height
            0,                // width (auto)
            0,                // escapement
            0,                // orientation
            FW_BOLD.0 as i32, // weight
            0,                // italic
            0,                // underline
            0,                // strikeout
            DEFAULT_CHARSET,
            OUT_DEFAULT_PRECIS,
            CLIP_DEFAULT_PRECIS,
            CLEARTYPE_QUALITY,
            (FF_SWISS.0 | VARIABLE_PITCH.0) as u32,
            w!("Segoe UI"),
        );
        let old_font = SelectObject(hdc, font.into());

        let mut wide_text: Vec<u16> = text.encode_utf16().collect();
        let rect = &mut RECT {
            left: 0,
            top: 0,
            right: 16,
            bottom: 16,
        };
        DrawTextW(
            hdc,
            &mut wide_text,
            rect,
            DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOCLIP,
        );

        // Create mask bitmap (all zeros = opaque where badge is)
        let hbm_mask = CreateCompatibleBitmap(hdc_screen, 16, 16);
        let hdc_mask = CreateCompatibleDC(Some(hdc_screen));
        let old_mask_bm = SelectObject(hdc_mask, hbm_mask.into());

        // Fill mask with white (transparent) first
        let white_brush = GetStockObject(WHITE_BRUSH);
        let mask_rect = RECT {
            left: 0,
            top: 0,
            right: 16,
            bottom: 16,
        };
        FillRect(hdc_mask, &mask_rect, HBRUSH(white_brush.0));

        // Draw black ellipse on mask (opaque area)
        let black_brush = GetStockObject(BLACK_BRUSH);
        let old_mask_brush = SelectObject(hdc_mask, HGDIOBJ(black_brush.0));
        let black_pen = GetStockObject(BLACK_PEN);
        let old_mask_pen = SelectObject(hdc_mask, HGDIOBJ(black_pen.0));
        let _ = Ellipse(hdc_mask, 0, 0, 16, 16);

        // Create the icon
        let icon_info = ICONINFO {
            fIcon: TRUE,
            xHotspot: 0,
            yHotspot: 0,
            hbmMask: hbm_mask,
            hbmColor: hbm,
        };
        let hicon = CreateIconIndirect(&icon_info).ok();

        // Cleanup
        SelectObject(hdc_mask, old_mask_pen);
        SelectObject(hdc_mask, old_mask_brush);
        SelectObject(hdc_mask, old_mask_bm);
        let _ = DeleteDC(hdc_mask);
        let _ = DeleteObject(hbm_mask.into());

        SelectObject(hdc, old_font);
        let _ = DeleteObject(font.into());
        SelectObject(hdc, old_pen);
        let _ = DeleteObject(pen.into());
        SelectObject(hdc, old_brush);
        let _ = DeleteObject(brush.into());
        SelectObject(hdc, old_bm);
        let _ = DeleteObject(hbm.into());
        let _ = DeleteDC(hdc);
        ReleaseDC(None, hdc_screen);

        hicon
    }
}

/// Escapes special XML characters in a string for use in toast notification XML templates.
fn escape_xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

#[cfg(test)]
mod tests {
    use crate::{ClipboardItem, read_from_clipboard, write_to_clipboard};

    #[test]
    fn test_clipboard() {
        let item = ClipboardItem::new_string("你好，我是张小白".to_string());
        write_to_clipboard(item.clone());
        assert_eq!(read_from_clipboard(), Some(item));

        let item = ClipboardItem::new_string("12345".to_string());
        write_to_clipboard(item.clone());
        assert_eq!(read_from_clipboard(), Some(item));

        let item = ClipboardItem::new_string_with_json_metadata("abcdef".to_string(), vec![3, 4]);
        write_to_clipboard(item.clone());
        assert_eq!(read_from_clipboard(), Some(item));
    }
}
