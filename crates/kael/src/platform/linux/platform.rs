use std::{
    collections::HashMap,
    env,
    path::{Path, PathBuf},
    process::Command,
    rc::Rc,
    sync::{Arc, Mutex},
};
#[cfg(any(feature = "wayland", feature = "x11"))]
use std::{
    ffi::OsString,
    fs::File,
    io::Read as _,
    os::fd::{AsFd, AsRawFd, FromRawFd},
    time::Duration,
};

use anyhow::{Context as _, anyhow};
use async_task::Runnable;
use calloop::{LoopSignal, channel::Channel};
use futures::channel::oneshot;
use util::ResultExt as _;
#[cfg(any(feature = "wayland", feature = "x11"))]
use xkbcommon::xkb::{self, Keycode, Keysym, State};

use crate::platform::tab_manager::{TabManagerState, WindowTabManager};
use crate::{
    Action, AnyWindowHandle, AttentionType, BackgroundExecutor, BiometricStatus, Bounds,
    ClipboardItem, CursorStyle, DialogOptions, DisplayId, FocusedWindowInfo, ForegroundExecutor,
    Keymap, Keystroke, LinuxDispatcher, MediaKeyEvent, Menu, MenuItem, NetworkStatus, OsInfo,
    OwnedMenu, PathPromptOptions, PermissionStatus, Pixels, Platform, PlatformDisplay,
    PlatformKeyboardLayout, PlatformKeyboardMapper, PlatformTextSystem, PlatformWindow, Point,
    PowerMode, PowerSaveBlockerKind, Result, SharedString, SystemPowerEvent, Task, TrayIconEvent,
    TrayMenuItem, WindowAppearance, WindowParams, px,
};

#[cfg(any(feature = "wayland", feature = "x11"))]
pub(crate) const SCROLL_LINES: f32 = 3.0;

// Values match the defaults on GTK.
// Taken from https://github.com/GNOME/gtk/blob/main/gtk/gtksettings.c#L320
#[cfg(any(feature = "wayland", feature = "x11"))]
pub(crate) const DOUBLE_CLICK_INTERVAL: Duration = Duration::from_millis(400);
pub(crate) const DOUBLE_CLICK_DISTANCE: Pixels = px(5.0);
pub(crate) const KEYRING_LABEL: &str = "zed-github-account";

pub trait LinuxClient {
    fn compositor_name(&self) -> &'static str;
    fn with_common<R>(&self, f: impl FnOnce(&mut LinuxCommon) -> R) -> R;
    fn keyboard_layout(&self) -> Box<dyn PlatformKeyboardLayout>;
    fn displays(&self) -> Vec<Rc<dyn PlatformDisplay>>;
    #[allow(unused)]
    fn display(&self, id: DisplayId) -> Option<Rc<dyn PlatformDisplay>>;
    fn primary_display(&self) -> Option<Rc<dyn PlatformDisplay>>;
    #[cfg(feature = "screen-capture")]
    fn is_screen_capture_supported(&self) -> bool;
    #[cfg(feature = "screen-capture")]
    fn screen_capture_sources(
        &self,
    ) -> oneshot::Receiver<Result<Vec<Rc<dyn crate::ScreenCaptureSource>>>>;

    fn open_window(
        &self,
        handle: AnyWindowHandle,
        options: WindowParams,
    ) -> anyhow::Result<Box<dyn PlatformWindow>>;
    fn set_cursor_style(&self, style: CursorStyle);
    fn open_uri(&self, uri: &str);
    fn reveal_path(&self, path: PathBuf);
    fn write_to_primary(&self, item: ClipboardItem);
    fn write_to_clipboard(&self, item: ClipboardItem);
    fn read_from_primary(&self) -> Option<ClipboardItem>;
    fn read_from_clipboard(&self) -> Option<ClipboardItem>;
    fn active_window(&self) -> Option<AnyWindowHandle>;
    fn window_stack(&self) -> Option<Vec<AnyWindowHandle>>;
    fn run(&self);

    fn focused_window_info(&self) -> Option<FocusedWindowInfo> {
        None
    }

    fn set_tray_icon(&self, _icon: Option<&[u8]>) {}
    fn set_tray_menu(&self, _menu: Vec<TrayMenuItem>) {}
    fn set_tray_tooltip(&self, _tooltip: &str) {}
    fn set_tray_panel_mode(&self, _enabled: bool) {}
    fn get_tray_icon_bounds(&self) -> Option<Bounds<Pixels>> {
        None
    }
    fn register_global_hotkey(&self, _id: u32, _keystroke: &Keystroke) -> Result<()> {
        Err(anyhow::anyhow!(
            "Global hotkeys not supported on this platform"
        ))
    }
    fn unregister_global_hotkey(&self, _id: u32) {}

    fn system_idle_time(&self) -> Option<Duration> {
        None
    }

    fn request_user_attention(&self, _level: AttentionType, _handle: Option<AnyWindowHandle>) {}

    fn cancel_user_attention(&self, _handle: Option<AnyWindowHandle>) {}

    #[cfg(any(feature = "wayland", feature = "x11"))]
    fn window_identifier(
        &self,
    ) -> impl Future<Output = Option<ashpd::WindowIdentifier>> + Send + 'static {
        std::future::ready::<Option<ashpd::WindowIdentifier>>(None)
    }
}

#[derive(Default)]
pub(crate) struct PlatformHandlers {
    pub(crate) open_urls: Option<Box<dyn FnMut(Vec<String>)>>,
    pub(crate) quit: Option<Box<dyn FnMut()>>,
    pub(crate) reopen: Option<Box<dyn FnMut()>>,
    pub(crate) app_menu_action: Option<Box<dyn FnMut(&dyn Action)>>,
    pub(crate) will_open_app_menu: Option<Box<dyn FnMut()>>,
    pub(crate) validate_app_menu_command: Option<Box<dyn FnMut(&dyn Action) -> bool>>,
    pub(crate) keyboard_layout_change: Option<Box<dyn FnMut()>>,
    pub(crate) tray_icon_event: Option<Box<dyn FnMut(TrayIconEvent)>>,
    pub(crate) tray_menu_action: Option<Box<dyn FnMut(SharedString)>>,
    pub(crate) global_hotkey: Option<Box<dyn FnMut(u32)>>,
    pub(crate) global_hotkey_up: Option<Box<dyn FnMut(u32)>>,
    pub(crate) system_power: Option<Box<dyn FnMut(SystemPowerEvent)>>,
    pub(crate) network_status_change: Option<Box<dyn FnMut(NetworkStatus)>>,
    pub(crate) media_key: Option<Box<dyn FnMut(MediaKeyEvent)>>,
    #[allow(dead_code)]
    pub(crate) context_menu: Option<Box<dyn FnMut(SharedString)>>,
}

pub(crate) enum PowerSaveHandle {
    /// Display sleep inhibition: screensaver D-Bus cookie + logind idle inhibit child.
    DisplaySleep {
        screensaver_cookie: Option<u32>,
        logind_child: Option<std::process::Child>,
    },
    /// Suspend inhibition via logind sleep inhibit child process.
    SuspendChild(std::process::Child),
}

pub(crate) struct LinuxCommon {
    pub(crate) background_executor: BackgroundExecutor,
    pub(crate) foreground_executor: ForegroundExecutor,
    pub(crate) text_system: Arc<dyn PlatformTextSystem>,
    pub(crate) appearance: WindowAppearance,
    pub(crate) auto_hide_scrollbars: bool,
    pub(crate) callbacks: PlatformHandlers,
    pub(crate) signal: LoopSignal,
    pub(crate) menus: Vec<OwnedMenu>,
    pub(crate) keep_alive_without_windows: bool,
    pub(crate) power_save_blockers: HashMap<u32, PowerSaveHandle>,
    pub(crate) next_blocker_id: u32,
    pub(crate) last_network_status: NetworkStatus,
    pub(crate) attention_window: Option<AnyWindowHandle>,
    pub(crate) network_monitor: Option<super::network::NetworkMonitorHandle>,
    pub(crate) network_tx: calloop::channel::Sender<NetworkStatus>,
    pub(crate) system_power_monitor: Option<super::power::SystemPowerMonitorHandle>,
    pub(crate) system_power_tx: calloop::channel::Sender<SystemPowerEvent>,
    pub(crate) tab_manager_state: Arc<Mutex<TabManagerState>>,
}

impl LinuxCommon {
    pub fn new(
        signal: LoopSignal,
    ) -> (
        Self,
        Channel<Runnable>,
        calloop::channel::Channel<NetworkStatus>,
        calloop::channel::Channel<SystemPowerEvent>,
    ) {
        let (main_sender, main_receiver) = calloop::channel::channel::<Runnable>();
        let (network_tx, network_rx) = calloop::channel::channel::<NetworkStatus>();
        let (system_power_tx, system_power_rx) = calloop::channel::channel::<SystemPowerEvent>();

        #[cfg(any(feature = "wayland", feature = "x11"))]
        let text_system = Arc::new(crate::CosmicTextSystem::new());
        #[cfg(not(any(feature = "wayland", feature = "x11")))]
        let text_system = Arc::new(crate::NoopTextSystem::new());

        let callbacks = PlatformHandlers::default();

        let dispatcher = Arc::new(LinuxDispatcher::new(main_sender));

        let background_executor = BackgroundExecutor::new(dispatcher.clone());

        let common = LinuxCommon {
            background_executor,
            foreground_executor: ForegroundExecutor::new(dispatcher),
            text_system,
            appearance: WindowAppearance::Light,
            auto_hide_scrollbars: false,
            callbacks,
            signal,
            menus: Vec::new(),
            keep_alive_without_windows: false,
            power_save_blockers: HashMap::new(),
            next_blocker_id: 0,
            last_network_status: NetworkStatus::Online,
            attention_window: None,
            network_monitor: None,
            network_tx,
            system_power_monitor: None,
            system_power_tx,
            tab_manager_state: WindowTabManager::shared_state(),
        };

        (common, main_receiver, network_rx, system_power_rx)
    }
}

impl Drop for LinuxCommon {
    fn drop(&mut self) {
        for (_, handle) in self.power_save_blockers.drain() {
            crate::platform::linux::power::release_blocker(handle);
        }
    }
}

impl<P: LinuxClient + 'static> Platform for P {
    fn background_executor(&self) -> BackgroundExecutor {
        self.with_common(|common| common.background_executor.clone())
    }

    fn foreground_executor(&self) -> ForegroundExecutor {
        self.with_common(|common| common.foreground_executor.clone())
    }

    fn text_system(&self) -> Arc<dyn PlatformTextSystem> {
        self.with_common(|common| common.text_system.clone())
    }

    fn keyboard_layout(&self) -> Box<dyn PlatformKeyboardLayout> {
        self.keyboard_layout()
    }

    fn keyboard_mapper(&self) -> Rc<dyn PlatformKeyboardMapper> {
        Rc::new(crate::DummyKeyboardMapper)
    }

    fn on_keyboard_layout_change(&self, callback: Box<dyn FnMut()>) {
        self.with_common(|common| common.callbacks.keyboard_layout_change = Some(callback));
    }

    fn run(&self, on_finish_launching: Box<dyn FnOnce()>) {
        on_finish_launching();

        // On Linux, URL scheme handlers are invoked by launching the app with the URL
        // as a command-line argument. Check args for URLs and dispatch to on_open_urls.
        let url_args: Vec<String> = env::args()
            .skip(1)
            .filter(|arg| arg.contains("://"))
            .collect();
        if !url_args.is_empty() {
            self.with_common(|common| {
                if let Some(callback) = common.callbacks.open_urls.as_mut() {
                    callback(url_args);
                }
            });
        }

        LinuxClient::run(self);

        let quit = self.with_common(|common| common.callbacks.quit.take());
        if let Some(mut fun) = quit {
            fun();
        }
    }

    fn quit(&self) {
        self.with_common(|common| common.signal.stop());
    }

    fn compositor_name(&self) -> &'static str {
        self.compositor_name()
    }

    fn restart(&self, binary_path: Option<PathBuf>) {
        use std::os::unix::process::CommandExt as _;

        // get the process id of the current process
        let app_pid = std::process::id().to_string();
        // get the path to the executable
        let app_path = if let Some(path) = binary_path {
            path
        } else {
            match self.app_path() {
                Ok(path) => path,
                Err(err) => {
                    log::error!("Failed to get app path: {:?}", err);
                    return;
                }
            }
        };

        log::info!("Restarting process, using app path: {:?}", app_path);

        // Script to wait for the current process to exit and then restart the app.
        let script = format!(
            r#"
            while kill -0 {pid} 2>/dev/null; do
                sleep 0.1
            done

            {app_path}
            "#,
            pid = app_pid,
            app_path = app_path.display()
        );

        #[allow(
            clippy::disallowed_methods,
            reason = "We are restarting ourselves, using std command thus is fine"
        )]
        let restart_process = Command::new("/usr/bin/env")
            .arg("bash")
            .arg("-c")
            .arg(script)
            .process_group(0)
            .spawn();

        match restart_process {
            Ok(_) => self.quit(),
            Err(e) => log::error!("failed to spawn restart script: {:?}", e),
        }
    }

    fn activate(&self, _ignoring_other_apps: bool) {
        log::info!("activate is not implemented on Linux, ignoring the call")
    }

    fn hide(&self) {
        log::info!("hide is not implemented on Linux, ignoring the call")
    }

    fn hide_other_apps(&self) {
        log::info!("hide_other_apps is not implemented on Linux, ignoring the call")
    }

    fn unhide_other_apps(&self) {
        log::info!("unhide_other_apps is not implemented on Linux, ignoring the call")
    }

    fn primary_display(&self) -> Option<Rc<dyn PlatformDisplay>> {
        self.primary_display()
    }

    fn displays(&self) -> Vec<Rc<dyn PlatformDisplay>> {
        self.displays()
    }

    #[cfg(feature = "screen-capture")]
    fn is_screen_capture_supported(&self) -> bool {
        self.is_screen_capture_supported()
    }

    #[cfg(feature = "screen-capture")]
    fn screen_capture_sources(
        &self,
    ) -> oneshot::Receiver<Result<Vec<Rc<dyn crate::ScreenCaptureSource>>>> {
        self.screen_capture_sources()
    }

    fn active_window(&self) -> Option<AnyWindowHandle> {
        self.active_window()
    }

    fn window_stack(&self) -> Option<Vec<AnyWindowHandle>> {
        self.window_stack()
    }

    fn open_window(
        &self,
        handle: AnyWindowHandle,
        options: WindowParams,
    ) -> anyhow::Result<Box<dyn PlatformWindow>> {
        self.open_window(handle, options)
    }

    fn open_url(&self, url: &str) {
        self.open_uri(url);
    }

    fn on_open_urls(&self, callback: Box<dyn FnMut(Vec<String>)>) {
        self.with_common(|common| common.callbacks.open_urls = Some(callback));
    }

    fn prompt_for_paths(
        &self,
        options: PathPromptOptions,
    ) -> oneshot::Receiver<Result<Option<Vec<PathBuf>>>> {
        let (done_tx, done_rx) = oneshot::channel();

        #[cfg(not(any(feature = "wayland", feature = "x11")))]
        let _ = (done_tx.send(Ok(None)), options);

        #[cfg(any(feature = "wayland", feature = "x11"))]
        let identifier = self.window_identifier();

        #[cfg(any(feature = "wayland", feature = "x11"))]
        self.foreground_executor()
            .spawn(async move {
                let title = if options.directories {
                    "Open Folder"
                } else {
                    "Open File"
                };

                let request = match ashpd::desktop::file_chooser::OpenFileRequest::default()
                    .identifier(identifier.await)
                    .modal(true)
                    .title(title)
                    .accept_label(options.prompt.as_ref().map(crate::SharedString::as_str))
                    .multiple(options.multiple)
                    .directory(options.directories)
                    .send()
                    .await
                {
                    Ok(request) => request,
                    Err(ashpd::Error::PortalNotFound(_)) => {
                        // XDG Desktop Portal unavailable — fall back to zenity/kdialog
                        log::info!(
                            "XDG Desktop Portal not available for file chooser, \
                             falling back to built-in dialog"
                        );
                        let result = Ok(crate::platform::linux::dialog::prompt_for_paths_fallback(
                            &options,
                        ));
                        let _ = done_tx.send(result);
                        return;
                    }
                    Err(err) => {
                        let _ = done_tx.send(Err(err.into()));
                        return;
                    }
                };

                let result = match request.response() {
                    Ok(response) => Ok(Some(
                        response
                            .uris()
                            .iter()
                            .filter_map(|uri| uri.to_file_path().ok())
                            .collect::<Vec<_>>(),
                    )),
                    Err(ashpd::Error::Response(_)) => Ok(None),
                    Err(e) => Err(e.into()),
                };
                let _ = done_tx.send(result);
            })
            .detach();
        done_rx
    }

    fn prompt_for_new_path(
        &self,
        directory: &Path,
        suggested_name: Option<&str>,
    ) -> oneshot::Receiver<Result<Option<PathBuf>>> {
        let (done_tx, done_rx) = oneshot::channel();

        #[cfg(not(any(feature = "wayland", feature = "x11")))]
        let _ = (done_tx.send(Ok(None)), directory, suggested_name);

        #[cfg(any(feature = "wayland", feature = "x11"))]
        let identifier = self.window_identifier();

        #[cfg(any(feature = "wayland", feature = "x11"))]
        self.foreground_executor()
            .spawn({
                let directory = directory.to_owned();
                let suggested_name = suggested_name.map(|s| s.to_owned());

                async move {
                    let mut request_builder =
                        ashpd::desktop::file_chooser::SaveFileRequest::default()
                            .identifier(identifier.await)
                            .modal(true)
                            .title("Save File")
                            .current_folder(directory.clone())
                            .expect("pathbuf should not be nul terminated");

                    if let Some(ref suggested_name) = suggested_name {
                        request_builder = request_builder.current_name(suggested_name.as_str());
                    }

                    let request = match request_builder.send().await {
                        Ok(request) => request,
                        Err(ashpd::Error::PortalNotFound(_)) => {
                            // XDG Desktop Portal unavailable — fall back to zenity/kdialog
                            log::info!(
                                "XDG Desktop Portal not available for save dialog, \
                                 falling back to built-in dialog"
                            );
                            let result = Ok(
                                crate::platform::linux::dialog::prompt_for_new_path_fallback(
                                    &directory,
                                    suggested_name.as_deref(),
                                ),
                            );
                            let _ = done_tx.send(result);
                            return;
                        }
                        Err(err) => {
                            let _ = done_tx.send(Err(err.into()));
                            return;
                        }
                    };

                    let result = match request.response() {
                        Ok(response) => Ok(response
                            .uris()
                            .first()
                            .and_then(|uri| uri.to_file_path().ok())),
                        Err(ashpd::Error::Response(_)) => Ok(None),
                        Err(e) => Err(e.into()),
                    };
                    let _ = done_tx.send(result);
                }
            })
            .detach();

        done_rx
    }

    fn can_select_mixed_files_and_dirs(&self) -> bool {
        // The XDG Desktop Portal FileChooser specification only supports selecting
        // either files or directories via the `directory` boolean option — there is
        // no way to allow simultaneous selection of both. The zenity/kdialog
        // fallback dialogs have the same limitation.
        false
    }

    fn reveal_path(&self, path: &Path) {
        self.reveal_path(path.to_owned());
    }

    fn open_with_system(&self, path: &Path) {
        let path = path.to_owned();
        self.background_executor()
            .spawn(async move {
                let _ = smol::process::Command::new("xdg-open")
                    .arg(path)
                    .spawn()
                    .context("invoking xdg-open")
                    .log_err()?
                    .status()
                    .await
                    .log_err()?;
                Some(())
            })
            .detach();
    }

    fn on_quit(&self, callback: Box<dyn FnMut()>) {
        self.with_common(|common| {
            common.callbacks.quit = Some(callback);
        });
    }

    fn on_reopen(&self, callback: Box<dyn FnMut()>) {
        self.with_common(|common| {
            common.callbacks.reopen = Some(callback);
        });
    }

    fn on_app_menu_action(&self, callback: Box<dyn FnMut(&dyn Action)>) {
        self.with_common(|common| {
            common.callbacks.app_menu_action = Some(callback);
        });
    }

    fn on_will_open_app_menu(&self, callback: Box<dyn FnMut()>) {
        self.with_common(|common| {
            common.callbacks.will_open_app_menu = Some(callback);
        });
    }

    fn on_validate_app_menu_command(&self, callback: Box<dyn FnMut(&dyn Action) -> bool>) {
        self.with_common(|common| {
            common.callbacks.validate_app_menu_command = Some(callback);
        });
    }

    fn app_path(&self) -> Result<PathBuf> {
        // get the path of the executable of the current process
        let app_path = env::current_exe()?;
        Ok(app_path)
    }

    fn set_menus(&self, menus: Vec<Menu>, _keymap: &Keymap) {
        self.with_common(|common| {
            common.menus = menus.into_iter().map(|menu| menu.owned()).collect();
        })
    }

    fn get_menus(&self) -> Option<Vec<OwnedMenu>> {
        self.with_common(|common| Some(common.menus.clone()))
    }

    fn set_dock_menu(&self, menu: Vec<MenuItem>, keymap: &Keymap) {
        super::dock::set_dock_menu(&menu, keymap);
    }

    fn path_for_auxiliary_executable(&self, _name: &str) -> Result<PathBuf> {
        Err(anyhow::Error::msg(
            "Platform<LinuxPlatform>::path_for_auxiliary_executable is not implemented yet",
        ))
    }

    fn set_cursor_style(&self, style: CursorStyle) {
        self.set_cursor_style(style)
    }

    fn should_auto_hide_scrollbars(&self) -> bool {
        self.with_common(|common| common.auto_hide_scrollbars)
    }

    fn write_credentials(&self, url: &str, username: &str, password: &[u8]) -> Task<Result<()>> {
        let url = url.to_string();
        let username = username.to_string();
        let password = password.to_vec();
        self.background_executor().spawn(async move {
            let keyring = oo7::Keyring::new().await?;
            keyring.unlock().await?;
            keyring
                .create_item(
                    KEYRING_LABEL,
                    &vec![("url", &url), ("username", &username)],
                    password,
                    true,
                )
                .await?;
            Ok(())
        })
    }

    fn read_credentials(&self, url: &str) -> Task<Result<Option<(String, Vec<u8>)>>> {
        let url = url.to_string();
        self.background_executor().spawn(async move {
            let keyring = oo7::Keyring::new().await?;
            keyring.unlock().await?;

            let items = keyring.search_items(&vec![("url", &url)]).await?;

            for item in items.into_iter() {
                if item.label().await.is_ok_and(|label| label == KEYRING_LABEL) {
                    let attributes = item.attributes().await?;
                    let username = attributes
                        .get("username")
                        .context("Cannot find username in stored credentials")?;
                    item.unlock().await?;
                    let secret = item.secret().await?;

                    // we lose the zeroizing capabilities at this boundary,
                    // a current limitation GPUI's credentials api
                    return Ok(Some((username.to_string(), secret.to_vec())));
                } else {
                    continue;
                }
            }
            Ok(None)
        })
    }

    fn delete_credentials(&self, url: &str) -> Task<Result<()>> {
        let url = url.to_string();
        self.background_executor().spawn(async move {
            let keyring = oo7::Keyring::new().await?;
            keyring.unlock().await?;

            let items = keyring.search_items(&vec![("url", &url)]).await?;

            for item in items.into_iter() {
                if item.label().await.is_ok_and(|label| label == KEYRING_LABEL) {
                    item.delete().await?;
                    return Ok(());
                }
            }

            Ok(())
        })
    }

    fn window_appearance(&self) -> WindowAppearance {
        self.with_common(|common| common.appearance)
    }

    fn register_url_scheme(&self, scheme: &str) -> Task<anyhow::Result<()>> {
        let scheme = scheme.to_string();
        Task::ready((|| {
            let exe_path =
                env::current_exe().context("Failed to determine application executable path")?;
            let exe_str = exe_path.to_string_lossy();

            // Derive an app name from the executable filename for the .desktop file
            let app_name = exe_path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("gpui-app");
            let desktop_filename = format!("{}-url-handler.desktop", app_name);

            // Determine the XDG data directory for .desktop files
            let data_dir = env::var_os("XDG_DATA_HOME")
                .map(PathBuf::from)
                .or_else(|| {
                    env::var_os("HOME").map(|home| PathBuf::from(home).join(".local").join("share"))
                })
                .ok_or_else(|| anyhow!("Could not determine XDG data directory"))?;

            let applications_dir = data_dir.join("applications");
            std::fs::create_dir_all(&applications_dir).with_context(|| {
                format!(
                    "Failed to create applications directory: {}",
                    applications_dir.display()
                )
            })?;

            let desktop_file_path = applications_dir.join(&desktop_filename);

            // Read existing .desktop file to preserve/merge MimeType entries
            let existing_mime_types = if desktop_file_path.exists() {
                std::fs::read_to_string(&desktop_file_path)
                    .ok()
                    .and_then(|content| {
                        content
                            .lines()
                            .find_map(|line| line.strip_prefix("MimeType=").map(|v| v.to_string()))
                    })
            } else {
                None
            };

            // Build the MimeType value, merging with any existing entries
            let new_mime = format!("x-scheme-handler/{}", scheme);
            let mime_type_value = if let Some(existing) = existing_mime_types {
                let existing_types: Vec<&str> =
                    existing.split(';').filter(|s| !s.is_empty()).collect();
                if existing_types.iter().any(|t| *t == new_mime) {
                    // Already registered
                    existing_types.join(";") + ";"
                } else {
                    let mut types = existing_types;
                    types.push(&new_mime);
                    types.join(";") + ";"
                }
            } else {
                format!("{};", new_mime)
            };

            // Write the .desktop file
            let desktop_content = format!(
                "[Desktop Entry]\n\
                 Type=Application\n\
                 Name={name}\n\
                 Exec=\"{exe}\" %u\n\
                 MimeType={mime}\n\
                 NoDisplay=true\n",
                name = app_name,
                exe = exe_str,
                mime = mime_type_value,
            );

            std::fs::write(&desktop_file_path, &desktop_content).with_context(|| {
                format!(
                    "Failed to write .desktop file: {}",
                    desktop_file_path.display()
                )
            })?;

            // Register the scheme with xdg-mime
            let status = Command::new("xdg-mime")
                .arg("default")
                .arg(&desktop_filename)
                .arg(format!("x-scheme-handler/{}", scheme))
                .status()
                .context("Failed to execute xdg-mime command")?;

            if !status.success() {
                return Err(anyhow!(
                    "xdg-mime command failed with exit code: {:?}",
                    status.code()
                ));
            }

            Ok(())
        })())
    }

    fn write_to_primary(&self, item: ClipboardItem) {
        self.write_to_primary(item)
    }

    fn write_to_clipboard(&self, item: ClipboardItem) {
        self.write_to_clipboard(item)
    }

    fn read_from_primary(&self) -> Option<ClipboardItem> {
        self.read_from_primary()
    }

    fn read_from_clipboard(&self) -> Option<ClipboardItem> {
        self.read_from_clipboard()
    }

    fn add_recent_document(&self, path: &Path) {
        super::dock::add_recent_document(path);
    }

    fn set_keep_alive_without_windows(&self, keep_alive: bool) {
        self.with_common(|common| common.keep_alive_without_windows = keep_alive);
    }

    fn set_tray_icon(&self, icon: Option<&[u8]>) {
        LinuxClient::set_tray_icon(self, icon);
    }

    fn set_tray_menu(&self, menu: Vec<TrayMenuItem>) {
        LinuxClient::set_tray_menu(self, menu);
    }

    fn set_tray_tooltip(&self, tooltip: &str) {
        LinuxClient::set_tray_tooltip(self, tooltip);
    }

    fn on_tray_icon_event(&self, callback: Box<dyn FnMut(TrayIconEvent)>) {
        self.with_common(|common| common.callbacks.tray_icon_event = Some(callback));
    }

    fn on_tray_menu_action(&self, callback: Box<dyn FnMut(SharedString)>) {
        self.with_common(|common| common.callbacks.tray_menu_action = Some(callback));
    }

    fn set_tray_panel_mode(&self, enabled: bool) {
        LinuxClient::set_tray_panel_mode(self, enabled);
    }

    fn get_tray_icon_bounds(&self) -> Option<Bounds<Pixels>> {
        LinuxClient::get_tray_icon_bounds(self)
    }

    fn register_global_hotkey(&self, id: u32, keystroke: &Keystroke) -> Result<()> {
        LinuxClient::register_global_hotkey(self, id, keystroke)
    }

    fn unregister_global_hotkey(&self, id: u32) {
        LinuxClient::unregister_global_hotkey(self, id);
    }

    fn on_global_hotkey(&self, callback: Box<dyn FnMut(u32)>) {
        self.with_common(|common| common.callbacks.global_hotkey = Some(callback));
    }

    fn on_global_hotkey_up(&self, callback: Box<dyn FnMut(u32)>) {
        self.with_common(|common| common.callbacks.global_hotkey_up = Some(callback));
    }

    fn focused_window_info(&self) -> Option<FocusedWindowInfo> {
        LinuxClient::focused_window_info(self)
    }

    fn set_auto_launch(&self, app_id: &str, enabled: bool) -> Result<()> {
        crate::platform::linux::auto_launch::set_auto_launch(app_id, enabled)
    }

    fn is_auto_launch_enabled(&self, app_id: &str) -> bool {
        crate::platform::linux::auto_launch::is_auto_launch_enabled(app_id)
    }

    fn show_notification(&self, title: &str, body: &str) -> Result<()> {
        crate::platform::linux::notifications::show_notification(title, body)
    }

    fn show_notification_with_actions(
        &self,
        title: &str,
        body: &str,
        actions: &[crate::NotificationAction],
        callback: Box<dyn FnMut(String)>,
    ) -> Result<()> {
        let foreground = self.foreground_executor();
        crate::platform::linux::notifications::show_notification_with_actions(
            title, body, actions, callback, foreground,
        )
    }

    fn os_info(&self) -> OsInfo {
        crate::platform::linux::os_info::get_os_info()
    }

    fn network_status(&self) -> NetworkStatus {
        super::network::query_network_status()
    }

    fn on_network_status_change(&self, callback: Box<dyn FnMut(NetworkStatus)>) {
        self.with_common(|common| {
            common.callbacks.network_status_change = Some(callback);

            // Start the NetworkManager D-Bus monitor if not already running.
            if common.network_monitor.is_none() {
                let tx = common.network_tx.clone();
                common.network_monitor = super::network::start_network_monitor(tx);
                if common.network_monitor.is_none() {
                    log::warn!(
                        "NetworkManager D-Bus monitor unavailable; \
                         network change events will not be delivered"
                    );
                }
            }
        });
    }

    fn start_power_save_blocker(&self, kind: PowerSaveBlockerKind) -> Option<u32> {
        self.with_common(|common| {
            let handle = match kind {
                PowerSaveBlockerKind::PreventDisplaySleep => {
                    crate::platform::linux::power::inhibit_screensaver(
                        "gpui",
                        "Power save blocker",
                    )?
                }
                PowerSaveBlockerKind::PreventAppSuspension => {
                    crate::platform::linux::power::inhibit_suspend("gpui", "Power save blocker")?
                }
            };
            let id = common.next_blocker_id;
            common.next_blocker_id += 1;
            common.power_save_blockers.insert(id, handle);
            Some(id)
        })
    }

    fn stop_power_save_blocker(&self, id: u32) {
        self.with_common(|common| {
            if let Some(handle) = common.power_save_blockers.remove(&id) {
                crate::platform::linux::power::release_blocker(handle);
            }
        });
    }

    fn power_mode(&self) -> PowerMode {
        super::power::power_mode()
    }

    fn system_idle_time(&self) -> Option<Duration> {
        LinuxClient::system_idle_time(self)
    }

    fn on_system_power_event(&self, callback: Box<dyn FnMut(SystemPowerEvent)>) {
        self.with_common(|common| {
            common.callbacks.system_power = Some(callback);

            if common.system_power_monitor.is_none() {
                let tx = common.system_power_tx.clone();
                common.system_power_monitor = super::power::start_system_power_monitor(tx);
                if common.system_power_monitor.is_none() {
                    log::warn!(
                        "system power event monitoring is unavailable; \
                         Linux power-change callbacks will not be delivered"
                    );
                }
            }
        });
    }

    fn on_media_key_event(&self, callback: Box<dyn FnMut(MediaKeyEvent)>) {
        self.with_common(|common| common.callbacks.media_key = Some(callback));
    }

    fn request_user_attention(&self, level: AttentionType) {
        let handle = self.active_window();
        self.with_common(|common| common.attention_window = handle);
        LinuxClient::request_user_attention(self, level, handle);
    }

    fn cancel_user_attention(&self) {
        let handle = self.with_common(|common| common.attention_window.take());
        LinuxClient::cancel_user_attention(self, handle);
    }

    fn set_dock_badge(&self, label: Option<&str>) {
        super::dock::set_dock_badge(label);
    }

    fn show_context_menu(
        &self,
        position: Point<Pixels>,
        items: Vec<TrayMenuItem>,
        callback: Box<dyn FnMut(SharedString)>,
    ) {
        if items.is_empty() {
            return;
        }

        let compositor = self.compositor_name();
        match compositor {
            #[cfg(feature = "x11")]
            "X11" => {
                super::context_menu::show_x11_context_menu(position, items, callback);
            }
            _ => {
                #[cfg(feature = "wayland")]
                super::context_menu::show_wayland_context_menu(position, items, callback);
                #[cfg(not(feature = "wayland"))]
                log::warn!("Context menus require X11 or Wayland feature to be enabled");
            }
        }
    }

    fn show_dialog(&self, options: DialogOptions) -> oneshot::Receiver<usize> {
        let (tx, rx) = oneshot::channel();
        self.background_executor()
            .spawn(async move {
                let result = crate::platform::linux::dialog::show_dialog(&options);
                let _ = tx.send(result);
            })
            .detach();
        rx
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
        // On Linux, microphone permission is either managed by PipeWire's portal
        // (which prompts on first device access) or not enforced at all (PulseAudio).
        // We check the current status synchronously, matching the Windows pattern.
        let status = super::microphone::microphone_status();
        callback(status != PermissionStatus::Denied);
    }

    fn accessibility_status(&self) -> PermissionStatus {
        super::accessibility::accessibility_status()
    }
}

#[cfg(any(feature = "wayland", feature = "x11"))]
pub(crate) fn keysym_to_media_key(keysym: xkbcommon::xkb::Keysym) -> Option<MediaKeyEvent> {
    use xkbcommon::xkb::Keysym;
    match keysym {
        Keysym::XF86_AudioPlay => Some(MediaKeyEvent::PlayPause),
        Keysym::XF86_AudioPause => Some(MediaKeyEvent::Pause),
        Keysym::XF86_AudioStop => Some(MediaKeyEvent::Stop),
        Keysym::XF86_AudioNext => Some(MediaKeyEvent::NextTrack),
        Keysym::XF86_AudioPrev => Some(MediaKeyEvent::PreviousTrack),
        _ => None,
    }
}

#[cfg(any(feature = "wayland", feature = "x11"))]
pub(super) fn open_uri_internal(
    executor: BackgroundExecutor,
    uri: &str,
    activation_token: Option<String>,
) {
    if let Some(uri) = ashpd::url::Url::parse(uri).log_err() {
        executor
            .spawn(async move {
                match ashpd::desktop::open_uri::OpenFileRequest::default()
                    .activation_token(activation_token.clone().map(ashpd::ActivationToken::from))
                    .send_uri(&uri)
                    .await
                {
                    Ok(_) => return,
                    Err(e) => log::error!("Failed to open with dbus: {}", e),
                }

                for mut command in open::commands(uri.to_string()) {
                    if let Some(token) = activation_token.as_ref() {
                        command.env("XDG_ACTIVATION_TOKEN", token);
                    }
                    let program = format!("{:?}", command.get_program());
                    match smol::process::Command::from(command).spawn() {
                        Ok(mut cmd) => {
                            cmd.status().await.log_err();
                            return;
                        }
                        Err(e) => {
                            log::error!("Failed to open with {}: {}", program, e)
                        }
                    }
                }
            })
            .detach();
    }
}

#[cfg(any(feature = "x11", feature = "wayland"))]
pub(super) fn reveal_path_internal(
    executor: BackgroundExecutor,
    path: PathBuf,
    activation_token: Option<String>,
) {
    executor
        .spawn(async move {
            if let Some(dir) = File::open(path.clone()).log_err() {
                match ashpd::desktop::open_uri::OpenDirectoryRequest::default()
                    .activation_token(activation_token.map(ashpd::ActivationToken::from))
                    .send(&dir.as_fd())
                    .await
                {
                    Ok(_) => return,
                    Err(e) => log::error!("Failed to open with dbus: {}", e),
                }
                if path.is_dir() {
                    open::that_detached(path).log_err();
                } else {
                    open::that_detached(path.parent().unwrap_or(Path::new(""))).log_err();
                }
            }
        })
        .detach();
}

#[allow(unused)]
pub(super) fn is_within_click_distance(a: Point<Pixels>, b: Point<Pixels>) -> bool {
    let diff = a - b;
    diff.x.abs() <= DOUBLE_CLICK_DISTANCE && diff.y.abs() <= DOUBLE_CLICK_DISTANCE
}

#[cfg(any(feature = "wayland", feature = "x11"))]
pub(super) fn get_xkb_compose_state(cx: &xkb::Context) -> Option<xkb::compose::State> {
    let mut locales = Vec::default();
    if let Some(locale) = env::var_os("LC_CTYPE") {
        locales.push(locale);
    }
    locales.push(OsString::from("C"));
    let mut state: Option<xkb::compose::State> = None;
    for locale in locales {
        if let Ok(table) =
            xkb::compose::Table::new_from_locale(cx, &locale, xkb::compose::COMPILE_NO_FLAGS)
        {
            state = Some(xkb::compose::State::new(
                &table,
                xkb::compose::STATE_NO_FLAGS,
            ));
            break;
        }
    }
    state
}

#[cfg(any(feature = "wayland", feature = "x11"))]
pub(super) unsafe fn read_fd(mut fd: filedescriptor::FileDescriptor) -> Result<Vec<u8>> {
    let mut file = unsafe { File::from_raw_fd(fd.as_raw_fd()) };
    let mut buffer = Vec::new();
    file.read_to_end(&mut buffer)?;
    Ok(buffer)
}

#[cfg(any(feature = "wayland", feature = "x11"))]
pub(super) const DEFAULT_CURSOR_ICON_NAME: &str = "left_ptr";

impl CursorStyle {
    #[cfg(any(feature = "wayland", feature = "x11"))]
    pub(super) fn to_icon_names(self) -> &'static [&'static str] {
        // Based on cursor names from chromium:
        // https://github.com/chromium/chromium/blob/d3069cf9c973dc3627fa75f64085c6a86c8f41bf/ui/base/cursor/cursor_factory.cc#L113
        match self {
            CursorStyle::Arrow => &[DEFAULT_CURSOR_ICON_NAME],
            CursorStyle::IBeam => &["text", "xterm"],
            CursorStyle::Crosshair => &["crosshair", "cross"],
            CursorStyle::ClosedHand => &["closedhand", "grabbing", "hand2"],
            CursorStyle::OpenHand => &["openhand", "grab", "hand1"],
            CursorStyle::PointingHand => &["pointer", "hand", "hand2"],
            CursorStyle::ResizeLeft => &["w-resize", "left_side"],
            CursorStyle::ResizeRight => &["e-resize", "right_side"],
            CursorStyle::ResizeLeftRight => &["ew-resize", "sb_h_double_arrow"],
            CursorStyle::ResizeUp => &["n-resize", "top_side"],
            CursorStyle::ResizeDown => &["s-resize", "bottom_side"],
            CursorStyle::ResizeUpDown => &["sb_v_double_arrow", "ns-resize"],
            CursorStyle::ResizeUpLeftDownRight => &["size_fdiag", "bd_double_arrow", "nwse-resize"],
            CursorStyle::ResizeUpRightDownLeft => &["size_bdiag", "nesw-resize", "fd_double_arrow"],
            CursorStyle::ResizeColumn => &["col-resize", "sb_h_double_arrow"],
            CursorStyle::ResizeRow => &["row-resize", "sb_v_double_arrow"],
            CursorStyle::IBeamCursorForVerticalLayout => &["vertical-text"],
            CursorStyle::OperationNotAllowed => &["not-allowed", "crossed_circle"],
            CursorStyle::DragLink => &["alias"],
            CursorStyle::DragCopy => &["copy"],
            CursorStyle::ContextualMenu => &["context-menu"],
            CursorStyle::None => {
                #[cfg(debug_assertions)]
                panic!("CursorStyle::None should be handled separately in the client");
                #[cfg(not(debug_assertions))]
                &[DEFAULT_CURSOR_ICON_NAME]
            }
        }
    }
}

#[cfg(any(feature = "wayland", feature = "x11"))]
pub(super) fn log_cursor_icon_warning(message: impl std::fmt::Display) {
    if let Ok(xcursor_path) = env::var("XCURSOR_PATH") {
        log::warn!(
            "{:#}\ncursor icon loading may be failing if XCURSOR_PATH environment variable is invalid. \
                    XCURSOR_PATH overrides the default icon search. Its current value is '{}'",
            message,
            xcursor_path
        );
    } else {
        log::warn!("{:#}", message);
    }
}

#[cfg(any(feature = "wayland", feature = "x11"))]
fn guess_ascii(keycode: Keycode, shift: bool) -> Option<char> {
    let c = match (keycode.raw(), shift) {
        (24, _) => 'q',
        (25, _) => 'w',
        (26, _) => 'e',
        (27, _) => 'r',
        (28, _) => 't',
        (29, _) => 'y',
        (30, _) => 'u',
        (31, _) => 'i',
        (32, _) => 'o',
        (33, _) => 'p',
        (34, false) => '[',
        (34, true) => '{',
        (35, false) => ']',
        (35, true) => '}',
        (38, _) => 'a',
        (39, _) => 's',
        (40, _) => 'd',
        (41, _) => 'f',
        (42, _) => 'g',
        (43, _) => 'h',
        (44, _) => 'j',
        (45, _) => 'k',
        (46, _) => 'l',
        (47, false) => ';',
        (47, true) => ':',
        (48, false) => '\'',
        (48, true) => '"',
        (49, false) => '`',
        (49, true) => '~',
        (51, false) => '\\',
        (51, true) => '|',
        (52, _) => 'z',
        (53, _) => 'x',
        (54, _) => 'c',
        (55, _) => 'v',
        (56, _) => 'b',
        (57, _) => 'n',
        (58, _) => 'm',
        (59, false) => ',',
        (59, true) => '>',
        (60, false) => '.',
        (60, true) => '<',
        (61, false) => '/',
        (61, true) => '?',

        _ => return None,
    };

    Some(c)
}

#[cfg(any(feature = "wayland", feature = "x11"))]
impl crate::Keystroke {
    pub(super) fn from_xkb(
        state: &State,
        mut modifiers: crate::Modifiers,
        keycode: Keycode,
    ) -> Self {
        let key_utf32 = state.key_get_utf32(keycode);
        let key_utf8 = state.key_get_utf8(keycode);
        let key_sym = state.key_get_one_sym(keycode);

        let key = match key_sym {
            Keysym::Return => "enter".to_owned(),
            Keysym::Prior => "pageup".to_owned(),
            Keysym::Next => "pagedown".to_owned(),
            Keysym::ISO_Left_Tab => "tab".to_owned(),
            Keysym::KP_Prior => "pageup".to_owned(),
            Keysym::KP_Next => "pagedown".to_owned(),
            Keysym::XF86_Back => "back".to_owned(),
            Keysym::XF86_Forward => "forward".to_owned(),
            Keysym::XF86_Cut => "cut".to_owned(),
            Keysym::XF86_Copy => "copy".to_owned(),
            Keysym::XF86_Paste => "paste".to_owned(),
            Keysym::XF86_New => "new".to_owned(),
            Keysym::XF86_Open => "open".to_owned(),
            Keysym::XF86_Save => "save".to_owned(),

            Keysym::comma => ",".to_owned(),
            Keysym::period => ".".to_owned(),
            Keysym::less => "<".to_owned(),
            Keysym::greater => ">".to_owned(),
            Keysym::slash => "/".to_owned(),
            Keysym::question => "?".to_owned(),

            Keysym::semicolon => ";".to_owned(),
            Keysym::colon => ":".to_owned(),
            Keysym::apostrophe => "'".to_owned(),
            Keysym::quotedbl => "\"".to_owned(),

            Keysym::bracketleft => "[".to_owned(),
            Keysym::braceleft => "{".to_owned(),
            Keysym::bracketright => "]".to_owned(),
            Keysym::braceright => "}".to_owned(),
            Keysym::backslash => "\\".to_owned(),
            Keysym::bar => "|".to_owned(),

            Keysym::grave => "`".to_owned(),
            Keysym::asciitilde => "~".to_owned(),
            Keysym::exclam => "!".to_owned(),
            Keysym::at => "@".to_owned(),
            Keysym::numbersign => "#".to_owned(),
            Keysym::dollar => "$".to_owned(),
            Keysym::percent => "%".to_owned(),
            Keysym::asciicircum => "^".to_owned(),
            Keysym::ampersand => "&".to_owned(),
            Keysym::asterisk => "*".to_owned(),
            Keysym::parenleft => "(".to_owned(),
            Keysym::parenright => ")".to_owned(),
            Keysym::minus => "-".to_owned(),
            Keysym::underscore => "_".to_owned(),
            Keysym::equal => "=".to_owned(),
            Keysym::plus => "+".to_owned(),
            Keysym::space => "space".to_owned(),
            Keysym::BackSpace => "backspace".to_owned(),
            Keysym::Tab => "tab".to_owned(),
            Keysym::Delete => "delete".to_owned(),
            Keysym::Escape => "escape".to_owned(),

            Keysym::Left => "left".to_owned(),
            Keysym::Right => "right".to_owned(),
            Keysym::Up => "up".to_owned(),
            Keysym::Down => "down".to_owned(),
            Keysym::Home => "home".to_owned(),
            Keysym::End => "end".to_owned(),
            Keysym::Insert => "insert".to_owned(),

            _ => {
                let name = xkb::keysym_get_name(key_sym).to_lowercase();
                if key_sym.is_keypad_key() {
                    name.replace("kp_", "")
                } else if let Some(key) = key_utf8.chars().next()
                    && key_utf8.len() == 1
                    && key.is_ascii()
                {
                    if key.is_ascii_graphic() {
                        key_utf8.to_lowercase()
                    // map ctrl-a to `a`
                    // ctrl-0..9 may emit control codes like ctrl-[, but
                    // we don't want to map them to `[`
                    } else if key_utf32 <= 0x1f
                        && !name.chars().next().is_some_and(|c| c.is_ascii_digit())
                    {
                        ((key_utf32 as u8 + 0x40) as char)
                            .to_ascii_lowercase()
                            .to_string()
                    } else {
                        name
                    }
                } else if let Some(key_en) = guess_ascii(keycode, modifiers.shift) {
                    String::from(key_en)
                } else {
                    name
                }
            }
        };

        if modifiers.shift {
            // we only include the shift for upper-case letters by convention,
            // so don't include for numbers and symbols, but do include for
            // tab/enter, etc.
            if key.chars().count() == 1 && key.to_lowercase() == key.to_uppercase() {
                modifiers.shift = false;
            }
        }

        // Ignore control characters (and DEL) for the purposes of key_char
        let key_char =
            (key_utf32 >= 32 && key_utf32 != 127 && !key_utf8.is_empty()).then_some(key_utf8);

        Self {
            modifiers,
            key,
            key_char,
        }
    }

    /**
     * Returns which symbol the dead key represents
     * <https://developer.mozilla.org/en-US/docs/Web/API/UI_Events/Keyboard_event_key_values#dead_keycodes_for_linux>
     */
    pub fn underlying_dead_key(keysym: Keysym) -> Option<String> {
        match keysym {
            Keysym::dead_grave => Some("`".to_owned()),
            Keysym::dead_acute => Some("´".to_owned()),
            Keysym::dead_circumflex => Some("^".to_owned()),
            Keysym::dead_tilde => Some("~".to_owned()),
            Keysym::dead_macron => Some("¯".to_owned()),
            Keysym::dead_breve => Some("˘".to_owned()),
            Keysym::dead_abovedot => Some("˙".to_owned()),
            Keysym::dead_diaeresis => Some("¨".to_owned()),
            Keysym::dead_abovering => Some("˚".to_owned()),
            Keysym::dead_doubleacute => Some("˝".to_owned()),
            Keysym::dead_caron => Some("ˇ".to_owned()),
            Keysym::dead_cedilla => Some("¸".to_owned()),
            Keysym::dead_ogonek => Some("˛".to_owned()),
            Keysym::dead_iota => Some("ͅ".to_owned()),
            Keysym::dead_voiced_sound => Some("゙".to_owned()),
            Keysym::dead_semivoiced_sound => Some("゚".to_owned()),
            Keysym::dead_belowdot => Some("̣̣".to_owned()),
            Keysym::dead_hook => Some("̡".to_owned()),
            Keysym::dead_horn => Some("̛".to_owned()),
            Keysym::dead_stroke => Some("̶̶".to_owned()),
            Keysym::dead_abovecomma => Some("̓̓".to_owned()),
            Keysym::dead_abovereversedcomma => Some("ʽ".to_owned()),
            Keysym::dead_doublegrave => Some("̏".to_owned()),
            Keysym::dead_belowring => Some("˳".to_owned()),
            Keysym::dead_belowmacron => Some("̱".to_owned()),
            Keysym::dead_belowcircumflex => Some("ꞈ".to_owned()),
            Keysym::dead_belowtilde => Some("̰".to_owned()),
            Keysym::dead_belowbreve => Some("̮".to_owned()),
            Keysym::dead_belowdiaeresis => Some("̤".to_owned()),
            Keysym::dead_invertedbreve => Some("̯".to_owned()),
            Keysym::dead_belowcomma => Some("̦".to_owned()),
            Keysym::dead_currency => None,
            Keysym::dead_lowline => None,
            Keysym::dead_aboveverticalline => None,
            Keysym::dead_belowverticalline => None,
            Keysym::dead_longsolidusoverlay => None,
            Keysym::dead_a => None,
            Keysym::dead_A => None,
            Keysym::dead_e => None,
            Keysym::dead_E => None,
            Keysym::dead_i => None,
            Keysym::dead_I => None,
            Keysym::dead_o => None,
            Keysym::dead_O => None,
            Keysym::dead_u => None,
            Keysym::dead_U => None,
            Keysym::dead_small_schwa => Some("ə".to_owned()),
            Keysym::dead_capital_schwa => Some("Ə".to_owned()),
            Keysym::dead_greek => None,
            _ => None,
        }
    }
}

#[cfg(any(feature = "wayland", feature = "x11"))]
impl crate::Modifiers {
    pub(super) fn from_xkb(keymap_state: &State) -> Self {
        let shift = keymap_state.mod_name_is_active(xkb::MOD_NAME_SHIFT, xkb::STATE_MODS_EFFECTIVE);
        let alt = keymap_state.mod_name_is_active(xkb::MOD_NAME_ALT, xkb::STATE_MODS_EFFECTIVE);
        let control =
            keymap_state.mod_name_is_active(xkb::MOD_NAME_CTRL, xkb::STATE_MODS_EFFECTIVE);
        let platform =
            keymap_state.mod_name_is_active(xkb::MOD_NAME_LOGO, xkb::STATE_MODS_EFFECTIVE);
        Self {
            shift,
            alt,
            control,
            platform,
            function: false,
        }
    }
}

#[cfg(any(feature = "wayland", feature = "x11"))]
impl crate::Capslock {
    pub(super) fn from_xkb(keymap_state: &State) -> Self {
        let on = keymap_state.mod_name_is_active(xkb::MOD_NAME_CAPS, xkb::STATE_MODS_EFFECTIVE);
        Self { on }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Point, px};

    #[test]
    fn test_is_within_click_distance() {
        let zero = Point::new(px(0.0), px(0.0));
        assert!(is_within_click_distance(zero, Point::new(px(5.0), px(5.0))));
        assert!(is_within_click_distance(
            zero,
            Point::new(px(-4.9), px(5.0))
        ));
        assert!(is_within_click_distance(
            Point::new(px(3.0), px(2.0)),
            Point::new(px(-2.0), px(-2.0))
        ));
        assert!(!is_within_click_distance(
            zero,
            Point::new(px(5.0), px(5.1))
        ),);
    }
}
