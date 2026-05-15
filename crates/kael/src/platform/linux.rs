pub(crate) mod accessibility;
mod auto_launch;
mod biometric;
#[cfg(any(feature = "wayland", feature = "x11"))]
mod character_palette;
#[cfg(any(feature = "wayland", feature = "x11"))]
mod context_menu;
pub(crate) mod dialog;
mod dispatcher;
mod dock;
mod global_hotkey;
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
#[cfg(any(feature = "wayland", feature = "x11"))]
mod text_system;
mod tray;
#[cfg(feature = "wayland")]
mod wayland;
#[cfg(any(feature = "wayland", feature = "x11"))]
mod webview;
#[cfg(feature = "x11")]
mod x11;

#[cfg(any(feature = "wayland", feature = "x11"))]
mod xdg_desktop_portal;

#[allow(unused_imports)]
pub(crate) use accessibility::*;
pub(crate) use dispatcher::*;
pub(crate) use headless::*;
pub(crate) use keyboard::*;
pub(crate) use platform::*;
#[cfg(any(feature = "wayland", feature = "x11"))]
pub(crate) use text_system::*;
#[cfg(feature = "wayland")]
pub(crate) use wayland::*;
#[cfg(feature = "x11")]
pub(crate) use x11::*;
#[cfg(any(feature = "wayland", feature = "x11"))]
pub(crate) use xdg_desktop_portal::XdgDesktopPortalCaptureBackend;

#[cfg(all(feature = "screen-capture", any(feature = "wayland", feature = "x11")))]
pub(crate) type PlatformScreenCaptureFrame = scap::frame::Frame;
#[cfg(not(all(feature = "screen-capture", any(feature = "wayland", feature = "x11"))))]
pub(crate) type PlatformScreenCaptureFrame = ();
