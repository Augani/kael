pub(crate) mod accessibility;
mod auto_launch;
mod biometric;
#[cfg(any(feature = "wayland", feature = "x11"))]
mod character_palette;
#[cfg(any(feature = "wayland", feature = "x11"))]
mod context_menu;
pub(crate) mod dbus_util;
pub(crate) mod dialog;
mod dispatcher;
mod dock;
mod global_hotkey;
#[cfg(feature = "webview-wayland-gtk4")]
mod gtk4_backend;
#[cfg(feature = "webview-wayland-gtk4")]
mod gtk4_scene;
#[cfg(feature = "webview-wayland-gtk4")]
mod gtk4_webview;
mod headless;
mod ipc;
mod keyboard;
mod microphone;
mod network;
mod pipewire_capture;

pub(crate) use ipc::resolve_socket_path;
pub(crate) use microphone::*;
pub(crate) use pipewire_capture::*;
mod notifications;
mod os_info;
mod platform;
mod power;
#[cfg(feature = "linux-platform")]
pub(crate) mod print;
#[cfg(feature = "linux-platform")]
mod text_system;
mod tray;
#[cfg(all(feature = "wayland", not(feature = "webview-wayland-gtk4")))]
mod wayland;
#[cfg(all(any(), feature = "linux-platform"))]
mod webview;
#[cfg(all(feature = "x11", not(feature = "webview-wayland-gtk4")))]
mod x11;

#[cfg(feature = "linux-platform")]
mod xdg_desktop_portal;

#[allow(unused_imports)]
pub(crate) use accessibility::*;
pub(crate) use dispatcher::*;
#[cfg(feature = "webview-wayland-gtk4")]
pub(crate) use gtk4_backend::*;
#[cfg(feature = "webview-wayland-gtk4")]
pub(crate) use gtk4_scene::gtk4_wayland_scene_proof_paintable;
pub(crate) use headless::*;
pub(crate) use keyboard::*;
pub(crate) use platform::*;
#[cfg(feature = "linux-platform")]
pub(crate) use text_system::*;
#[cfg(all(feature = "wayland", not(feature = "webview-wayland-gtk4")))]
pub(crate) use wayland::*;
#[cfg(all(feature = "x11", not(feature = "webview-wayland-gtk4")))]
pub(crate) use x11::*;
#[cfg(feature = "linux-platform")]
pub(crate) use xdg_desktop_portal::XdgDesktopPortalCaptureBackend;

pub(crate) fn catch_platform_callback<T>(
    name: &'static str,
    fallback: T,
    callback: impl FnOnce() -> T,
) -> T {
    crate::platform::catch_platform_callback("Linux", name, fallback, callback)
}

/// XWayland translates core X11 cursor warps into compositor pointer-lock
/// operations. Some supported XWayland releases can terminate the X server
/// when a client warps immediately after releasing a grab, so keep cursor
/// position restoration as a native-Xorg enhancement rather than making it
/// part of pointer-lock cleanup in a Wayland session.
#[cfg(any(feature = "webview-wayland-gtk4", feature = "x11"))]
pub(crate) fn x11_pointer_position_restore_is_safe() -> bool {
    let has_wayland_display =
        std::env::var_os("WAYLAND_DISPLAY").is_some_and(|value| !value.is_empty());
    let is_wayland_session =
        std::env::var("XDG_SESSION_TYPE").is_ok_and(|value| value.eq_ignore_ascii_case("wayland"));
    !has_wayland_display && !is_wayland_session
}

#[cfg(all(feature = "screen-capture", feature = "linux-platform"))]
pub(crate) type PlatformScreenCaptureFrame = scap::frame::Frame;
#[cfg(not(all(feature = "screen-capture", feature = "linux-platform")))]
pub(crate) type PlatformScreenCaptureFrame = ();
