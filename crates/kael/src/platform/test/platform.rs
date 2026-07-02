use crate::{
    AnyWindowHandle, AttentionType, BackgroundExecutor, BiometricStatus, ClipboardItem,
    CursorStyle, DevicePixels, DialogOptions, DummyKeyboardMapper, ForegroundExecutor, Keymap,
    MediaKeyEvent, NetworkStatus, NoopTextSystem, PathPromptOptions, Platform, PlatformDisplay,
    PlatformKeyboardLayout, PlatformKeyboardMapper, PlatformTextSystem, PowerMode,
    PowerSaveBlockerKind, PromptButton, ScreenCaptureFrame, ScreenCaptureSource,
    ScreenCaptureStream, SourceMetadata, SystemPowerEvent, Task, TestDisplay, TestWindow,
    TrayMenuItem, WindowAppearance, WindowParams, size,
};
use anyhow::Result;
use collections::VecDeque;
use futures::channel::oneshot;
use parking_lot::Mutex;
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    path::{Path, PathBuf},
    rc::{Rc, Weak},
    sync::Arc,
};
#[cfg(target_os = "windows")]
use windows::Win32::{
    Graphics::Imaging::{CLSID_WICImagingFactory, IWICImagingFactory},
    System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance},
};

/// TestPlatform implements the Platform trait for use in tests.
pub(crate) struct TestPlatform {
    background_executor: BackgroundExecutor,
    foreground_executor: ForegroundExecutor,

    pub(crate) active_window: RefCell<Option<TestWindow>>,
    active_display: Rc<dyn PlatformDisplay>,
    active_cursor: Mutex<CursorStyle>,
    current_clipboard_item: Mutex<Option<ClipboardItem>>,
    credentials: RefCell<HashMap<String, (String, Vec<u8>)>>,
    #[cfg(any(target_os = "linux", target_os = "freebsd"))]
    current_primary_item: Mutex<Option<ClipboardItem>>,
    pub(crate) prompts: RefCell<TestPrompts>,
    screen_capture_sources: RefCell<Vec<TestScreenCaptureSource>>,
    registered_url_schemes: RefCell<Vec<String>>,
    recent_documents: RefCell<Vec<PathBuf>>,
    tray_menu: RefCell<Vec<TrayMenuItem>>,
    tray_tooltip: RefCell<String>,
    tray_panel_mode: Cell<bool>,
    keep_alive_without_windows: Cell<bool>,
    auto_launch: RefCell<HashMap<String, bool>>,
    pub opened_url: RefCell<Option<String>>,
    pub text_system: Arc<dyn PlatformTextSystem>,
    power_mode: Cell<PowerMode>,
    next_power_save_blocker_id: Cell<u32>,
    power_save_blockers: RefCell<Vec<(u32, PowerSaveBlockerKind)>>,
    user_attention: Cell<Option<AttentionType>>,
    user_attention_cancel_count: Cell<usize>,
    network_status: Cell<NetworkStatus>,
    biometric_status: Cell<BiometricStatus>,
    biometric_auth_success: Cell<bool>,
    biometric_auth_reasons: RefCell<Vec<String>>,
    reduce_motion: Cell<bool>,
    open_urls_callback: RefCell<Option<Box<dyn FnMut(Vec<String>)>>>,
    system_power_callback: RefCell<Option<Box<dyn FnMut(SystemPowerEvent)>>>,
    media_key_callback: RefCell<Option<Box<dyn FnMut(MediaKeyEvent)>>>,
    network_status_callback: RefCell<Option<Box<dyn FnMut(NetworkStatus)>>>,
    #[cfg(target_os = "windows")]
    bitmap_factory: std::mem::ManuallyDrop<IWICImagingFactory>,
    weak: Weak<Self>,
}

#[derive(Clone)]
/// A fake screen capture source, used for testing.
pub struct TestScreenCaptureSource {}

/// A fake screen capture stream, used for testing.
pub struct TestScreenCaptureStream {}

impl ScreenCaptureSource for TestScreenCaptureSource {
    fn metadata(&self) -> Result<SourceMetadata> {
        Ok(SourceMetadata {
            id: 0,
            is_main: None,
            label: None,
            resolution: size(DevicePixels(1), DevicePixels(1)),
        })
    }

    fn stream(
        &self,
        _foreground_executor: &ForegroundExecutor,
        _frame_callback: Box<dyn Fn(ScreenCaptureFrame) + Send>,
    ) -> oneshot::Receiver<Result<Box<dyn ScreenCaptureStream>>> {
        let (mut tx, rx) = oneshot::channel();
        let stream = TestScreenCaptureStream {};
        tx.send(Ok(Box::new(stream) as Box<dyn ScreenCaptureStream>))
            .ok();
        rx
    }
}

impl ScreenCaptureStream for TestScreenCaptureStream {
    fn metadata(&self) -> Result<SourceMetadata> {
        TestScreenCaptureSource {}.metadata()
    }
}

struct TestPrompt {
    msg: String,
    detail: Option<String>,
    answers: Vec<String>,
    tx: oneshot::Sender<usize>,
}

#[derive(Default)]
pub(crate) struct TestPrompts {
    multiple_choice: VecDeque<TestPrompt>,
    paths: VecDeque<(
        PathPromptOptions,
        oneshot::Sender<Result<Option<Vec<PathBuf>>>>,
    )>,
    new_path: VecDeque<(PathBuf, oneshot::Sender<Result<Option<PathBuf>>>)>,
}

impl TestPlatform {
    pub fn new(executor: BackgroundExecutor, foreground_executor: ForegroundExecutor) -> Rc<Self> {
        #[cfg(target_os = "windows")]
        let bitmap_factory = unsafe {
            windows::Win32::System::Ole::OleInitialize(None)
                .expect("unable to initialize Windows OLE");
            std::mem::ManuallyDrop::new(
                CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)
                    .expect("Error creating bitmap factory."),
            )
        };

        let text_system = Arc::new(NoopTextSystem);

        Rc::new_cyclic(|weak| TestPlatform {
            background_executor: executor,
            foreground_executor,
            prompts: Default::default(),
            screen_capture_sources: Default::default(),
            registered_url_schemes: Default::default(),
            recent_documents: Default::default(),
            tray_menu: Default::default(),
            tray_tooltip: Default::default(),
            tray_panel_mode: Cell::new(false),
            keep_alive_without_windows: Cell::new(false),
            auto_launch: Default::default(),
            active_cursor: Default::default(),
            active_display: Rc::new(TestDisplay::new()),
            active_window: Default::default(),
            current_clipboard_item: Mutex::new(None),
            credentials: Default::default(),
            #[cfg(any(target_os = "linux", target_os = "freebsd"))]
            current_primary_item: Mutex::new(None),
            weak: weak.clone(),
            opened_url: Default::default(),
            power_mode: Cell::new(PowerMode::Performance),
            next_power_save_blocker_id: Cell::new(1),
            power_save_blockers: Default::default(),
            user_attention: Cell::new(None),
            user_attention_cancel_count: Cell::new(0),
            network_status: Cell::new(NetworkStatus::Online),
            biometric_status: Cell::new(BiometricStatus::Unavailable),
            biometric_auth_success: Cell::new(false),
            biometric_auth_reasons: Default::default(),
            reduce_motion: Cell::new(false),
            open_urls_callback: RefCell::new(None),
            system_power_callback: RefCell::new(None),
            media_key_callback: RefCell::new(None),
            network_status_callback: RefCell::new(None),
            #[cfg(target_os = "windows")]
            bitmap_factory,
            text_system,
        })
    }

    pub(crate) fn simulate_new_path_selection(
        &self,
        select_path: impl FnOnce(&std::path::Path) -> Option<std::path::PathBuf>,
    ) {
        let (path, tx) = self
            .prompts
            .borrow_mut()
            .new_path
            .pop_front()
            .expect("no pending new path prompt");
        self.background_executor().set_waiting_hint(None);
        tx.send(Ok(select_path(&path))).ok();
    }

    pub(crate) fn simulate_path_selection(
        &self,
        select_paths: impl FnOnce(&PathPromptOptions) -> Option<Vec<PathBuf>>,
    ) {
        let (options, tx) = self
            .prompts
            .borrow_mut()
            .paths
            .pop_front()
            .expect("no pending path prompt");
        self.background_executor().set_waiting_hint(None);
        tx.send(Ok(select_paths(&options))).ok();
    }

    #[track_caller]
    pub(crate) fn simulate_prompt_answer(&self, response: &str) {
        let prompt = self
            .prompts
            .borrow_mut()
            .multiple_choice
            .pop_front()
            .expect("no pending multiple choice prompt");
        self.background_executor().set_waiting_hint(None);
        let Some(ix) = prompt.answers.iter().position(|a| a == response) else {
            panic!(
                "PROMPT: {}\n{:?}\n{:?}\nCannot respond with {}",
                prompt.msg, prompt.detail, prompt.answers, response
            )
        };
        prompt.tx.send(ix).ok();
    }

    pub(crate) fn has_pending_prompt(&self) -> bool {
        !self.prompts.borrow().multiple_choice.is_empty()
    }

    pub(crate) fn pending_prompt(&self) -> Option<(String, String)> {
        let prompts = self.prompts.borrow();
        let prompt = prompts.multiple_choice.front()?;
        Some((
            prompt.msg.clone(),
            prompt.detail.clone().unwrap_or_default(),
        ))
    }

    pub(crate) fn set_screen_capture_sources(&self, sources: Vec<TestScreenCaptureSource>) {
        *self.screen_capture_sources.borrow_mut() = sources;
    }

    pub(crate) fn registered_url_schemes(&self) -> Vec<String> {
        self.registered_url_schemes.borrow().clone()
    }

    pub(crate) fn recent_documents(&self) -> Vec<PathBuf> {
        self.recent_documents.borrow().clone()
    }

    pub(crate) fn prompt(
        &self,
        msg: &str,
        detail: Option<&str>,
        answers: &[PromptButton],
    ) -> oneshot::Receiver<usize> {
        let (tx, rx) = oneshot::channel();
        let answers: Vec<String> = answers.iter().map(|s| s.label().to_string()).collect();
        self.background_executor()
            .set_waiting_hint(Some(format!("PROMPT: {:?} {:?}", msg, detail)));
        self.prompts
            .borrow_mut()
            .multiple_choice
            .push_back(TestPrompt {
                msg: msg.to_string(),
                detail: detail.map(|s| s.to_string()),
                answers,
                tx,
            });
        rx
    }

    pub(crate) fn set_active_window(&self, window: Option<TestWindow>) {
        let executor = self.foreground_executor();
        let previous_window = self.active_window.borrow_mut().take();
        self.active_window.borrow_mut().clone_from(&window);

        executor
            .spawn(async move {
                if let Some(previous_window) = previous_window {
                    if let Some(window) = window.as_ref()
                        && Rc::ptr_eq(&previous_window.0, &window.0)
                    {
                        return;
                    }
                    previous_window.simulate_active_status_change(false);
                }
                if let Some(window) = window {
                    window.simulate_active_status_change(true);
                }
            })
            .detach();
    }

    pub(crate) fn did_prompt_for_new_path(&self) -> bool {
        !self.prompts.borrow().new_path.is_empty()
    }

    pub(crate) fn did_prompt_for_paths(&self) -> bool {
        !self.prompts.borrow().paths.is_empty()
    }

    pub(crate) fn set_power_mode(&self, power_mode: PowerMode) {
        self.power_mode.set(power_mode);
    }

    pub(crate) fn power_save_blockers(&self) -> Vec<(u32, PowerSaveBlockerKind)> {
        self.power_save_blockers.borrow().clone()
    }

    pub(crate) fn user_attention(&self) -> Option<AttentionType> {
        self.user_attention.get()
    }

    pub(crate) fn user_attention_cancel_count(&self) -> usize {
        self.user_attention_cancel_count.get()
    }

    pub(crate) fn set_reduce_motion(&self, reduce_motion: bool) {
        self.reduce_motion.set(reduce_motion);
    }

    pub(crate) fn network_status(&self) -> NetworkStatus {
        self.network_status.get()
    }

    pub(crate) fn simulate_network_status_change(&self, status: NetworkStatus) {
        self.network_status.set(status);
        let mut callback = self.network_status_callback.borrow_mut().take();
        if let Some(ref mut callback) = callback {
            callback(status);
        }
        *self.network_status_callback.borrow_mut() = callback;
    }

    pub(crate) fn set_biometric_status(&self, status: BiometricStatus) {
        self.biometric_status.set(status);
    }

    pub(crate) fn set_biometric_auth_success(&self, success: bool) {
        self.biometric_auth_success.set(success);
    }

    pub(crate) fn biometric_auth_reasons(&self) -> Vec<String> {
        self.biometric_auth_reasons.borrow().clone()
    }

    pub(crate) fn simulate_system_power_event(&self, event: SystemPowerEvent) {
        let mut callback = self.system_power_callback.borrow_mut().take();
        if let Some(ref mut callback) = callback {
            callback(event);
        }
        *self.system_power_callback.borrow_mut() = callback;
    }

    pub(crate) fn simulate_media_key_event(&self, event: MediaKeyEvent) {
        let mut callback = self.media_key_callback.borrow_mut().take();
        if let Some(ref mut callback) = callback {
            callback(event);
        }
        *self.media_key_callback.borrow_mut() = callback;
    }

    pub(crate) fn tray_menu(&self) -> Vec<TrayMenuItem> {
        self.tray_menu.borrow().clone()
    }

    pub(crate) fn tray_tooltip(&self) -> String {
        self.tray_tooltip.borrow().clone()
    }

    pub(crate) fn tray_panel_mode(&self) -> bool {
        self.tray_panel_mode.get()
    }

    pub(crate) fn keep_alive_without_windows(&self) -> bool {
        self.keep_alive_without_windows.get()
    }

    pub(crate) fn simulate_open_urls(&self, urls: Vec<String>) {
        let mut callback = self.open_urls_callback.borrow_mut().take();
        if let Some(ref mut callback) = callback {
            callback(urls);
        }
        *self.open_urls_callback.borrow_mut() = callback;
    }
}

impl Platform for TestPlatform {
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
        Box::new(TestKeyboardLayout)
    }

    fn keyboard_mapper(&self) -> Rc<dyn PlatformKeyboardMapper> {
        Rc::new(DummyKeyboardMapper)
    }

    fn on_keyboard_layout_change(&self, _: Box<dyn FnMut()>) {}

    fn run(&self, _on_finish_launching: Box<dyn FnOnce()>) {
        _on_finish_launching();
    }

    fn quit(&self) {}

    fn restart(&self, _: Option<PathBuf>) {}

    fn activate(&self, _ignoring_other_apps: bool) {}

    fn set_auto_launch(&self, app_id: &str, enabled: bool) -> Result<()> {
        self.auto_launch
            .borrow_mut()
            .insert(app_id.to_string(), enabled);
        Ok(())
    }

    fn is_auto_launch_enabled(&self, app_id: &str) -> bool {
        self.auto_launch
            .borrow()
            .get(app_id)
            .copied()
            .unwrap_or(false)
    }

    fn hide(&self) {}

    fn hide_other_apps(&self) {}

    fn unhide_other_apps(&self) {}

    fn displays(&self) -> Vec<std::rc::Rc<dyn crate::PlatformDisplay>> {
        vec![self.active_display.clone()]
    }

    fn primary_display(&self) -> Option<std::rc::Rc<dyn crate::PlatformDisplay>> {
        Some(self.active_display.clone())
    }

    fn show_dialog(&self, options: DialogOptions) -> oneshot::Receiver<usize> {
        let buttons = options
            .buttons
            .into_iter()
            .map(|button| {
                let lower = button.to_lowercase();
                if lower == "ok" {
                    PromptButton::ok(button)
                } else if lower == "cancel" {
                    PromptButton::cancel(button)
                } else {
                    PromptButton::new(button)
                }
            })
            .collect::<Vec<_>>();
        self.prompt(
            options.title.as_ref(),
            options.detail.as_ref().map(|detail| detail.as_ref()),
            &buttons,
        )
    }

    #[cfg(feature = "screen-capture")]
    fn is_screen_capture_supported(&self) -> bool {
        true
    }

    #[cfg(feature = "screen-capture")]
    fn screen_capture_sources(
        &self,
    ) -> oneshot::Receiver<Result<Vec<Rc<dyn ScreenCaptureSource>>>> {
        let (mut tx, rx) = oneshot::channel();
        tx.send(Ok(self
            .screen_capture_sources
            .borrow()
            .iter()
            .map(|source| Rc::new(source.clone()) as Rc<dyn ScreenCaptureSource>)
            .collect()))
            .ok();
        rx
    }

    fn active_window(&self) -> Option<crate::AnyWindowHandle> {
        self.active_window
            .borrow()
            .as_ref()
            .map(|window| window.0.lock().handle)
    }

    fn start_power_save_blocker(&self, kind: PowerSaveBlockerKind) -> Option<u32> {
        let id = self.next_power_save_blocker_id.get();
        self.next_power_save_blocker_id.set(id + 1);
        self.power_save_blockers.borrow_mut().push((id, kind));
        Some(id)
    }

    fn stop_power_save_blocker(&self, id: u32) {
        self.power_save_blockers
            .borrow_mut()
            .retain(|(blocker_id, _)| *blocker_id != id);
    }

    fn power_mode(&self) -> PowerMode {
        self.power_mode.get()
    }

    fn should_reduce_motion(&self) -> bool {
        self.reduce_motion.get()
    }

    fn network_status(&self) -> NetworkStatus {
        self.network_status.get()
    }

    fn on_network_status_change(&self, callback: Box<dyn FnMut(NetworkStatus)>) {
        *self.network_status_callback.borrow_mut() = Some(callback);
    }

    fn biometric_status(&self) -> BiometricStatus {
        self.biometric_status.get()
    }

    fn authenticate_biometric(&self, reason: &str, callback: Box<dyn FnOnce(bool) + Send>) {
        self.biometric_auth_reasons
            .borrow_mut()
            .push(reason.to_string());
        callback(self.biometric_auth_success.get());
    }

    fn on_system_power_event(&self, callback: Box<dyn FnMut(SystemPowerEvent)>) {
        *self.system_power_callback.borrow_mut() = Some(callback);
    }

    fn on_media_key_event(&self, callback: Box<dyn FnMut(MediaKeyEvent)>) {
        *self.media_key_callback.borrow_mut() = Some(callback);
    }

    fn request_user_attention(&self, attention_type: AttentionType) {
        self.user_attention.set(Some(attention_type));
    }

    fn cancel_user_attention(&self) {
        self.user_attention.set(None);
        self.user_attention_cancel_count
            .set(self.user_attention_cancel_count.get() + 1);
    }

    fn set_keep_alive_without_windows(&self, keep_alive: bool) {
        self.keep_alive_without_windows.set(keep_alive);
    }

    fn set_tray_menu(&self, menu: Vec<TrayMenuItem>) {
        *self.tray_menu.borrow_mut() = menu;
    }

    fn set_tray_tooltip(&self, tooltip: &str) {
        *self.tray_tooltip.borrow_mut() = tooltip.to_string();
    }

    fn set_tray_panel_mode(&self, enabled: bool) {
        self.tray_panel_mode.set(enabled);
    }

    fn open_window(
        &self,
        handle: AnyWindowHandle,
        params: WindowParams,
    ) -> anyhow::Result<Box<dyn crate::PlatformWindow>> {
        let window = TestWindow::new(
            handle,
            params,
            self.weak.clone(),
            self.active_display.clone(),
        );
        *self.active_window.borrow_mut() = Some(window.clone());
        Ok(Box::new(window))
    }

    fn window_appearance(&self) -> WindowAppearance {
        WindowAppearance::Light
    }

    fn open_url(&self, url: &str) {
        *self.opened_url.borrow_mut() = Some(url.to_string())
    }

    fn on_open_urls(&self, callback: Box<dyn FnMut(Vec<String>)>) {
        *self.open_urls_callback.borrow_mut() = Some(callback);
    }

    fn prompt_for_paths(
        &self,
        options: crate::PathPromptOptions,
    ) -> oneshot::Receiver<Result<Option<Vec<std::path::PathBuf>>>> {
        let (tx, rx) = oneshot::channel();
        self.background_executor()
            .set_waiting_hint(Some(format!("PROMPT FOR PATHS: {:?}", options)));
        self.prompts.borrow_mut().paths.push_back((options, tx));
        rx
    }

    fn prompt_for_new_path(
        &self,
        directory: &std::path::Path,
        _suggested_name: Option<&str>,
    ) -> oneshot::Receiver<Result<Option<std::path::PathBuf>>> {
        let (tx, rx) = oneshot::channel();
        self.background_executor()
            .set_waiting_hint(Some(format!("PROMPT FOR PATH: {:?}", directory)));
        self.prompts
            .borrow_mut()
            .new_path
            .push_back((directory.to_path_buf(), tx));
        rx
    }

    fn can_select_mixed_files_and_dirs(&self) -> bool {
        true
    }

    fn reveal_path(&self, _path: &std::path::Path) {}

    fn on_quit(&self, _callback: Box<dyn FnMut()>) {}

    fn on_reopen(&self, _callback: Box<dyn FnMut()>) {}

    fn set_menus(&self, _menus: Vec<crate::Menu>, _keymap: &Keymap) {}
    fn set_dock_menu(&self, _menu: Vec<crate::MenuItem>, _keymap: &Keymap) {}

    fn add_recent_document(&self, path: &Path) {
        self.recent_documents.borrow_mut().push(path.to_path_buf());
    }

    fn on_app_menu_action(&self, _callback: Box<dyn FnMut(&dyn crate::Action)>) {}

    fn on_will_open_app_menu(&self, _callback: Box<dyn FnMut()>) {}

    fn on_validate_app_menu_command(&self, _callback: Box<dyn FnMut(&dyn crate::Action) -> bool>) {}

    fn app_path(&self) -> Result<std::path::PathBuf> {
        std::env::current_exe().map_err(Into::into)
    }

    fn path_for_auxiliary_executable(&self, name: &str) -> Result<std::path::PathBuf> {
        let app_path = self.app_path()?;
        let parent = app_path.parent().ok_or_else(|| {
            anyhow::anyhow!("test app path has no parent: {}", app_path.display())
        })?;
        Ok(parent.join(name))
    }

    fn set_cursor_style(&self, style: crate::CursorStyle) {
        *self.active_cursor.lock() = style;
    }

    fn should_auto_hide_scrollbars(&self) -> bool {
        false
    }

    #[cfg(any(target_os = "linux", target_os = "freebsd"))]
    fn write_to_primary(&self, item: ClipboardItem) {
        *self.current_primary_item.lock() = Some(item);
    }

    fn write_to_clipboard(&self, item: ClipboardItem) {
        *self.current_clipboard_item.lock() = Some(item);
    }

    #[cfg(any(target_os = "linux", target_os = "freebsd"))]
    fn read_from_primary(&self) -> Option<ClipboardItem> {
        self.current_primary_item.lock().clone()
    }

    fn read_from_clipboard(&self) -> Option<ClipboardItem> {
        self.current_clipboard_item.lock().clone()
    }

    fn write_credentials(&self, url: &str, username: &str, password: &[u8]) -> Task<Result<()>> {
        self.credentials
            .borrow_mut()
            .insert(url.to_string(), (username.to_string(), password.to_vec()));
        Task::ready(Ok(()))
    }

    fn read_credentials(&self, url: &str) -> Task<Result<Option<(String, Vec<u8>)>>> {
        Task::ready(Ok(self.credentials.borrow().get(url).cloned()))
    }

    fn delete_credentials(&self, url: &str) -> Task<Result<()>> {
        self.credentials.borrow_mut().remove(url);
        Task::ready(Ok(()))
    }

    fn register_url_scheme(&self, scheme: &str) -> Task<anyhow::Result<()>> {
        self.registered_url_schemes
            .borrow_mut()
            .push(scheme.to_string());
        Task::ready(Ok(()))
    }

    fn open_with_system(&self, _path: &Path) {}
}

impl TestScreenCaptureSource {
    /// Create a fake screen capture source, for testing.
    pub fn new() -> Self {
        Self {}
    }
}

#[cfg(target_os = "windows")]
impl Drop for TestPlatform {
    fn drop(&mut self) {
        unsafe {
            std::mem::ManuallyDrop::drop(&mut self.bitmap_factory);
            windows::Win32::System::Ole::OleUninitialize();
        }
    }
}

struct TestKeyboardLayout;

impl PlatformKeyboardLayout for TestKeyboardLayout {
    fn id(&self) -> &str {
        "kael.keyboard.example"
    }

    fn name(&self) -> &str {
        "kael.keyboard.example"
    }
}
