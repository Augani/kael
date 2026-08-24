use anyhow::Context as _;
use uuid::Uuid;
use x11rb::protocol::{randr, randr::ConnectionExt as _};
use x11rb::{connection::Connection as _, xcb_ffi::XCBConnection};

use crate::{Bounds, DisplayId, Pixels, PlatformDisplay, Point, Size, px};

#[derive(Debug)]
pub(crate) struct X11Display {
    display_id: DisplayId,
    bounds: Bounds<Pixels>,
    uuid: Uuid,
    refresh_rate: Option<f32>,
    scale_factor: f32,
}

impl X11Display {
    pub(crate) fn all(
        xcb: &XCBConnection,
        fallback_scale: f32,
        x_screen_index: usize,
    ) -> Vec<Self> {
        let Some(screen) = xcb.setup().roots.get(x_screen_index) else {
            return Vec::new();
        };
        let displays = monitor_displays(xcb, screen.root, fallback_scale)
            .unwrap_or_default()
            .into_iter()
            .map(|display| display.into_display(x_screen_index))
            .collect::<Vec<_>>();
        if displays.is_empty() {
            Self::new(xcb, fallback_scale, x_screen_index)
                .ok()
                .into_iter()
                .collect()
        } else {
            displays
        }
    }

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
        if let Some(display) = monitor_displays(xcb, screen.root, scale_factor)
            .ok()
            .and_then(|displays| {
                displays
                    .iter()
                    .find(|display| display.primary)
                    .or_else(|| displays.first())
                    .cloned()
            })
        {
            return Ok(display.into_display(x_screen_index));
        }
        let refresh_rate = query_refresh_rate(xcb, screen.root, &[]);
        Ok(Self {
            display_id: DisplayId(x_screen_index as u32),
            bounds: Bounds {
                origin: Default::default(),
                size: Size {
                    width: px(screen.width_in_pixels as f32 / scale_factor),
                    height: px(screen.height_in_pixels as f32 / scale_factor),
                },
            },
            uuid: Uuid::from_bytes([0; 16]),
            refresh_rate,
            scale_factor,
        })
    }

    pub(crate) fn for_window(
        xcb: &XCBConnection,
        root: x11rb::protocol::xproto::Window,
        x_screen_index: usize,
        physical_bounds: Bounds<i32>,
        fallback_scale: f32,
    ) -> anyhow::Result<Self> {
        let displays = monitor_displays(xcb, root, fallback_scale)?;
        let center = Point {
            x: physical_bounds
                .origin
                .x
                .saturating_add(physical_bounds.size.width / 2),
            y: physical_bounds
                .origin
                .y
                .saturating_add(physical_bounds.size.height / 2),
        };
        let selected = displays
            .iter()
            .find(|display| display.contains(center))
            .or_else(|| {
                displays
                    .iter()
                    .max_by_key(|display| display.overlap_area(physical_bounds))
            });
        Ok(selected
            .cloned()
            .map(|display| display.into_display(x_screen_index))
            .unwrap_or_else(|| Self {
                display_id: DisplayId(x_screen_index as u32),
                bounds: physical_bounds.map(|value| px(value as f32 / fallback_scale)),
                uuid: Uuid::from_bytes([0; 16]),
                refresh_rate: query_refresh_rate(xcb, root, &[]),
                scale_factor: fallback_scale,
            }))
    }
}

#[derive(Clone)]
struct MonitorDisplay {
    name: u32,
    primary: bool,
    physical_bounds: Bounds<i32>,
    refresh_rate: Option<f32>,
    scale_factor: f32,
}

impl MonitorDisplay {
    fn contains(&self, point: Point<i32>) -> bool {
        point.x >= self.physical_bounds.origin.x
            && point.y >= self.physical_bounds.origin.y
            && point.x
                < self
                    .physical_bounds
                    .origin
                    .x
                    .saturating_add(self.physical_bounds.size.width)
            && point.y
                < self
                    .physical_bounds
                    .origin
                    .y
                    .saturating_add(self.physical_bounds.size.height)
    }

    fn overlap_area(&self, bounds: Bounds<i32>) -> i64 {
        let left = self.physical_bounds.origin.x.max(bounds.origin.x);
        let top = self.physical_bounds.origin.y.max(bounds.origin.y);
        let right = self
            .physical_bounds
            .origin
            .x
            .saturating_add(self.physical_bounds.size.width)
            .min(bounds.origin.x.saturating_add(bounds.size.width));
        let bottom = self
            .physical_bounds
            .origin
            .y
            .saturating_add(self.physical_bounds.size.height)
            .min(bounds.origin.y.saturating_add(bounds.size.height));
        i64::from(right.saturating_sub(left).max(0)) * i64::from(bottom.saturating_sub(top).max(0))
    }

    fn into_display(self, x_screen_index: usize) -> X11Display {
        let mut identity = Vec::with_capacity(24);
        identity.extend_from_slice(&(x_screen_index as u64).to_ne_bytes());
        identity.extend_from_slice(&self.name.to_ne_bytes());
        identity.extend_from_slice(&self.physical_bounds.origin.x.to_ne_bytes());
        identity.extend_from_slice(&self.physical_bounds.origin.y.to_ne_bytes());
        X11Display {
            display_id: DisplayId(self.name),
            bounds: self
                .physical_bounds
                .map(|value| px(value as f32 / self.scale_factor)),
            uuid: Uuid::new_v5(&Uuid::NAMESPACE_OID, &identity),
            refresh_rate: self.refresh_rate,
            scale_factor: self.scale_factor,
        }
    }
}

fn monitor_displays(
    xcb: &XCBConnection,
    root: x11rb::protocol::xproto::Window,
    fallback_scale: f32,
) -> anyhow::Result<Vec<MonitorDisplay>> {
    let monitors = xcb
        .randr_get_monitors(root, true)?
        .reply()
        .context("failed to query active RandR monitors")?;
    Ok(monitors
        .monitors
        .into_iter()
        .filter(|monitor| monitor.width != 0 && monitor.height != 0)
        .map(|monitor| {
            let scale_factor = monitor_scale_factor(
                monitor.width,
                monitor.height,
                monitor.width_in_millimeters,
                monitor.height_in_millimeters,
            )
            .unwrap_or(fallback_scale);
            MonitorDisplay {
                name: monitor.name,
                primary: monitor.primary,
                physical_bounds: Bounds {
                    origin: Point {
                        x: i32::from(monitor.x),
                        y: i32::from(monitor.y),
                    },
                    size: Size {
                        width: i32::from(monitor.width),
                        height: i32::from(monitor.height),
                    },
                },
                refresh_rate: query_refresh_rate(xcb, root, &monitor.outputs),
                scale_factor,
            }
        })
        .collect())
}

/// Compute the active refresh rate for a monitor's driven RandR CRTC.
fn query_refresh_rate(
    xcb: &XCBConnection,
    root: x11rb::protocol::xproto::Window,
    outputs: &[randr::Output],
) -> Option<f32> {
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
        if !outputs.is_empty()
            && !crtc_info
                .outputs
                .iter()
                .any(|output| outputs.contains(output))
        {
            return None;
        }
        screen_resources
            .modes
            .iter()
            .find(|m| m.id == crtc_info.mode)
    })?;

    if mode_info.dot_clock == 0 || mode_info.htotal == 0 || mode_info.vtotal == 0 {
        None
    } else {
        let mut hertz =
            mode_info.dot_clock as f64 / (mode_info.htotal as f64 * mode_info.vtotal as f64);
        let flags = u32::from(mode_info.mode_flags);
        if flags & u32::from(randr::ModeFlag::INTERLACE) != 0 {
            hertz *= 2.0;
        }
        if flags & u32::from(randr::ModeFlag::DOUBLE_SCAN) != 0 {
            hertz /= 2.0;
        }
        Some(hertz as f32)
    }
}

fn monitor_scale_factor(
    width_px: u16,
    height_px: u16,
    width_mm: u32,
    height_mm: u32,
) -> Option<f32> {
    if width_px == 0 || height_px == 0 || width_mm == 0 || height_mm == 0 {
        return None;
    }
    let diagonal_ppmm = ((f64::from(width_px) * f64::from(height_px))
        / (f64::from(width_mm) * f64::from(height_mm)))
    .sqrt();
    let scale = (diagonal_ppmm * (12.0 * 25.4 / 96.0)).round() / 12.0;
    (scale.is_finite() && (1.0..=20.0).contains(&scale)).then_some(scale as f32)
}

impl PlatformDisplay for X11Display {
    fn id(&self) -> DisplayId {
        self.display_id
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

    fn scale_factor(&self) -> f32 {
        self.scale_factor
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn monitor_selection_prefers_containing_center_then_overlap() {
        let left = MonitorDisplay {
            name: 1,
            primary: true,
            physical_bounds: Bounds::new(
                Point { x: 0, y: 0 },
                Size {
                    width: 1920,
                    height: 1080,
                },
            ),
            refresh_rate: Some(60.0),
            scale_factor: 1.0,
        };
        assert!(left.contains(Point { x: 100, y: 100 }));
        assert!(!left.contains(Point { x: 2_000, y: 100 }));
        assert_eq!(
            left.overlap_area(Bounds::new(
                Point { x: 1_900, y: 0 },
                Size {
                    width: 100,
                    height: 100
                }
            )),
            2_000
        );
    }

    #[test]
    fn monitor_dpi_is_bounded_and_quantized() {
        assert_eq!(monitor_scale_factor(1920, 1080, 508, 285), Some(1.0));
        assert_eq!(monitor_scale_factor(0, 1080, 508, 285), None);
        assert_eq!(monitor_scale_factor(1920, 1080, 0, 285), None);
    }
}
