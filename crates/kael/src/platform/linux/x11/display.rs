use anyhow::Context as _;
use uuid::Uuid;
use x11rb::protocol::randr::ConnectionExt as _;
use x11rb::{connection::Connection as _, xcb_ffi::XCBConnection};

use crate::{Bounds, DisplayId, Pixels, PlatformDisplay, Size, px};

#[derive(Debug)]
pub(crate) struct X11Display {
    x_screen_index: usize,
    bounds: Bounds<Pixels>,
    uuid: Uuid,
    refresh_rate: Option<f32>,
}

impl X11Display {
    pub(crate) fn new(
        xcb: &XCBConnection,
        scale_factor: f32,
        x_screen_index: usize,
    ) -> anyhow::Result<Self> {
        let screen = xcb
            .setup()
            .roots
            .get(x_screen_index)
            .with_context(|| format!("No screen found with index {x_screen_index}"))?;
        let refresh_rate = query_refresh_rate(xcb, screen.root);
        Ok(Self {
            x_screen_index,
            bounds: Bounds {
                origin: Default::default(),
                size: Size {
                    width: px(screen.width_in_pixels as f32 / scale_factor),
                    height: px(screen.height_in_pixels as f32 / scale_factor),
                },
            },
            uuid: Uuid::from_bytes([0; 16]),
            refresh_rate,
        })
    }
}

/// Compute the active refresh rate for a screen from its first driven RandR CRTC.
/// Returns `None` when RandR is unavailable or no CRTC is active.
fn query_refresh_rate(xcb: &XCBConnection, root: x11rb::protocol::xproto::Window) -> Option<f32> {
    let screen_resources = xcb
        .randr_get_screen_resources_current(root)
        .ok()?
        .reply()
        .ok()?;

    let mode_info = screen_resources.crtcs.iter().find_map(|crtc| {
        let crtc_info = xcb
            .randr_get_crtc_info(*crtc, x11rb::CURRENT_TIME)
            .ok()?
            .reply()
            .ok()?;
        screen_resources
            .modes
            .iter()
            .find(|m| m.id == crtc_info.mode)
    })?;

    if mode_info.dot_clock == 0 || mode_info.htotal == 0 || mode_info.vtotal == 0 {
        None
    } else {
        let hertz =
            mode_info.dot_clock as f64 / (mode_info.htotal as f64 * mode_info.vtotal as f64);
        Some(hertz as f32)
    }
}

impl PlatformDisplay for X11Display {
    fn id(&self) -> DisplayId {
        DisplayId(self.x_screen_index as u32)
    }

    fn uuid(&self) -> anyhow::Result<Uuid> {
        Ok(self.uuid)
    }

    fn bounds(&self) -> Bounds<Pixels> {
        self.bounds
    }

    fn refresh_rate(&self) -> Option<f32> {
        self.refresh_rate
    }
}
