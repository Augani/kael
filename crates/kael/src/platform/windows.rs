pub(crate) mod accessibility;
mod active_window;
mod auto_launch;
mod biometric;
mod clipboard;
mod destination_list;
mod dialog;
mod direct_write;
mod directx_atlas;
mod directx_devices;
mod directx_renderer;
mod dispatcher;
mod display;
mod events;
mod global_hotkey;
mod ipc;
mod keyboard;
mod microphone;
mod network;
mod screen_capture;
mod system_audio;

pub(crate) use ipc::pipe_name;
pub(crate) use microphone::*;
pub(crate) use screen_capture::*;
pub(crate) use system_audio::*;
mod os_info;
mod platform;
mod power;
mod print;
mod system_settings;
mod tray;
mod util;
mod vsync;
#[cfg(feature = "webview")]
mod webview;
mod window;
mod wrapper;

pub(crate) use accessibility::*;
pub(crate) use clipboard::*;
pub(crate) use destination_list::*;
pub(crate) use direct_write::*;
pub(crate) use directx_atlas::*;
pub(crate) use directx_devices::*;
pub(crate) use directx_renderer::*;
pub(crate) use dispatcher::*;
pub(crate) use display::*;
pub(crate) use events::*;
pub(crate) use keyboard::*;
pub(crate) use platform::*;
pub(crate) use system_settings::*;
pub(crate) use tray::*;
pub(crate) use util::*;
pub(crate) use vsync::*;
pub(crate) use window::*;
pub(crate) use wrapper::*;

pub(crate) use windows::Win32::Foundation::HWND;

pub(crate) fn catch_platform_callback<T>(
    name: &'static str,
    fallback: T,
    callback: impl FnOnce() -> T,
) -> T {
    crate::platform::catch_platform_callback("Windows", name, fallback, callback)
}

#[cfg(feature = "screen-capture")]
pub(crate) type PlatformScreenCaptureFrame = scap::frame::Frame;
#[cfg(not(feature = "screen-capture"))]
pub(crate) type PlatformScreenCaptureFrame = ();
