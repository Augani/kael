use std::cell::RefCell;
use std::rc::Rc;

use anyhow::Context as _;
use calloop::{EventLoop, LoopHandle};
use util::ResultExt;

use crate::platform::linux::LinuxClient;
use crate::platform::{LinuxCommon, PlatformWindow};
use crate::{
    AnyWindowHandle, CursorStyle, DisplayId, LinuxKeyboardLayout, PlatformDisplay,
    PlatformKeyboardLayout, WindowParams,
};

pub struct HeadlessClientState {
    pub(crate) _loop_handle: LoopHandle<'static, HeadlessClient>,
    pub(crate) event_loop: Option<calloop::EventLoop<'static, HeadlessClient>>,
    pub(crate) common: LinuxCommon,
}

#[derive(Clone)]
pub(crate) struct HeadlessClient(Rc<RefCell<HeadlessClientState>>);

impl HeadlessClient {
    pub(crate) fn new() -> anyhow::Result<Self> {
        let event_loop =
            EventLoop::try_new().context("failed to create the headless event loop")?;

        let (common, main_receiver, network_rx, system_power_rx) =
            LinuxCommon::new(event_loop.get_signal());

        let handle = event_loop.handle();

        handle
            .insert_source(main_receiver, |event, _, _: &mut HeadlessClient| {
                if let calloop::channel::Event::Msg(runnable) = event {
                    crate::platform::catch_platform_callback(
                        "Linux",
                        "foreground task",
                        (),
                        || {
                            runnable.run();
                        },
                    );
                }
            })
            .map_err(|error| {
                anyhow::anyhow!("failed to register the headless task source: {error:?}")
            })?;

        handle
            .insert_source(network_rx, |event, _, client: &mut HeadlessClient| {
                if let calloop::channel::Event::Msg(status) = event {
                    let mut state = client.0.borrow_mut();
                    let prev = state.common.last_network_status;
                    state.common.last_network_status = status;
                    if status != prev {
                        let mut callback = state.common.callbacks.network_status_change.take();
                        drop(state);
                        if let Some(ref mut cb) = callback {
                            super::super::catch_platform_callback("network change", (), || {
                                cb(status)
                            });
                        }
                        client.0.borrow_mut().common.callbacks.network_status_change = callback;
                    }
                }
            })
            .map_err(|error| {
                anyhow::anyhow!("failed to register the headless network source: {error:?}")
            })?;

        handle
            .insert_source(system_power_rx, |event, _, client: &mut HeadlessClient| {
                if let calloop::channel::Event::Msg(power_event) = event {
                    let mut state = client.0.borrow_mut();
                    let mut callback = state.common.callbacks.system_power.take();
                    drop(state);
                    if let Some(ref mut cb) = callback {
                        super::super::catch_platform_callback("system power", (), || {
                            cb(power_event)
                        });
                    }
                    client.0.borrow_mut().common.callbacks.system_power = callback;
                }
            })
            .map_err(|error| {
                anyhow::anyhow!("failed to register the headless power source: {error:?}")
            })?;

        Ok(HeadlessClient(Rc::new(RefCell::new(HeadlessClientState {
            event_loop: Some(event_loop),
            _loop_handle: handle,
            common,
        }))))
    }
}

impl LinuxClient for HeadlessClient {
    fn with_common<R>(&self, f: impl FnOnce(&mut LinuxCommon) -> R) -> R {
        f(&mut self.0.borrow_mut().common)
    }

    fn keyboard_layout(&self) -> Box<dyn PlatformKeyboardLayout> {
        Box::new(LinuxKeyboardLayout::new("unknown".into()))
    }

    fn displays(&self) -> Vec<Rc<dyn PlatformDisplay>> {
        vec![]
    }

    fn primary_display(&self) -> Option<Rc<dyn PlatformDisplay>> {
        None
    }

    fn display(&self, _id: DisplayId) -> Option<Rc<dyn PlatformDisplay>> {
        None
    }

    #[cfg(feature = "screen-capture")]
    fn is_screen_capture_supported(&self) -> bool {
        false
    }

    #[cfg(feature = "screen-capture")]
    fn screen_capture_sources(
        &self,
    ) -> futures::channel::oneshot::Receiver<anyhow::Result<Vec<Rc<dyn crate::ScreenCaptureSource>>>>
    {
        let (mut tx, rx) = futures::channel::oneshot::channel();
        tx.send(Err(anyhow::anyhow!(
            "Headless mode does not support screen capture."
        )))
        .ok();
        rx
    }

    fn active_window(&self) -> Option<AnyWindowHandle> {
        None
    }

    fn window_stack(&self) -> Option<Vec<AnyWindowHandle>> {
        None
    }

    fn open_window(
        &self,
        _handle: AnyWindowHandle,
        _params: WindowParams,
    ) -> anyhow::Result<Box<dyn PlatformWindow>> {
        anyhow::bail!("neither DISPLAY nor WAYLAND_DISPLAY is set. You can run in headless mode");
    }

    fn compositor_name(&self) -> &'static str {
        "headless"
    }

    fn set_cursor_style(&self, _style: CursorStyle) {}

    fn open_uri(&self, _uri: &str) {}

    fn reveal_path(&self, _path: std::path::PathBuf) {}

    fn write_to_primary(&self, _item: crate::ClipboardItem) {}

    fn write_to_clipboard(&self, _item: crate::ClipboardItem) {}

    fn clear_clipboard(&self) {}

    fn read_from_primary(&self) -> Option<crate::ClipboardItem> {
        None
    }

    fn read_from_clipboard(&self) -> Option<crate::ClipboardItem> {
        None
    }

    fn run(&self) {
        let Some(mut event_loop) = self.0.borrow_mut().event_loop.take() else {
            log::warn!("ignoring a second attempt to run the headless event loop");
            return;
        };

        event_loop.run(None, &mut self.clone(), |_| {}).log_err();
    }
}
