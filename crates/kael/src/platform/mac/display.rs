use super::{finite_native_coordinate, finite_native_dimension};
use crate::{Bounds, DisplayId, Pixels, PlatformDisplay, px, size};
use anyhow::Result;
use core_foundation::base::{CFRelease, CFTypeRef};
use core_foundation::uuid::{CFUUIDGetUUIDBytes, CFUUIDRef};
use core_graphics::display::{
    CGDirectDisplayID, CGDisplay, CGDisplayBounds, CGGetActiveDisplayList, CGMainDisplayID,
};
use uuid::Uuid;

#[derive(Debug)]
pub(crate) struct MacDisplay(pub(crate) CGDirectDisplayID);

unsafe impl Send for MacDisplay {}

impl MacDisplay {
    /// Get the screen with the given [`DisplayId`].
    pub fn find_by_id(id: DisplayId) -> Option<Self> {
        Self::all().find(|screen| screen.id() == id)
    }

    /// Get the primary screen - the one with the menu bar, and whose bottom left
    /// corner is at the origin of the AppKit coordinate system.
    pub fn primary() -> Self {
        // `CGMainDisplayID` returns the display with the menu bar, whose bottom-left
        // corner is at the origin of the AppKit coordinate system. Unlike `NSScreen`,
        // it is safe to call off the main thread (headless/test contexts) and does not
        // depend on `CGGetActiveDisplayList`, which can be empty while the machine sleeps.
        Self(unsafe { CGMainDisplayID() })
    }

    /// Obtains an iterator over all currently active system displays.
    pub fn all() -> impl Iterator<Item = Self> {
        unsafe {
            // We're assuming there aren't more than 32 displays connected to the system.
            let mut displays = Vec::with_capacity(32);
            let mut display_count = 0;
            let result = CGGetActiveDisplayList(
                displays.capacity() as u32,
                displays.as_mut_ptr(),
                &mut display_count,
            );

            if result == 0 && display_count as usize <= displays.capacity() {
                displays.set_len(display_count as usize);
            } else {
                log::error!("failed to get a bounded active display list");
            }
            displays.into_iter().map(MacDisplay)
        }
    }
}

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn CGDisplayCreateUUIDFromDisplayID(display: CGDirectDisplayID) -> CFUUIDRef;
}

impl PlatformDisplay for MacDisplay {
    fn id(&self) -> DisplayId {
        DisplayId(self.0)
    }

    fn uuid(&self) -> Result<Uuid> {
        let cfuuid = unsafe { CGDisplayCreateUUIDFromDisplayID(self.0 as CGDirectDisplayID) };
        anyhow::ensure!(
            !cfuuid.is_null(),
            "AppKit returned a null from CGDisplayCreateUUIDFromDisplayID"
        );

        let bytes = unsafe { CFUUIDGetUUIDBytes(cfuuid) };
        let uuid = Uuid::from_bytes([
            bytes.byte0,
            bytes.byte1,
            bytes.byte2,
            bytes.byte3,
            bytes.byte4,
            bytes.byte5,
            bytes.byte6,
            bytes.byte7,
            bytes.byte8,
            bytes.byte9,
            bytes.byte10,
            bytes.byte11,
            bytes.byte12,
            bytes.byte13,
            bytes.byte14,
            bytes.byte15,
        ]);
        unsafe { CFRelease(cfuuid as CFTypeRef) };
        Ok(uuid)
    }

    fn bounds(&self) -> Bounds<Pixels> {
        unsafe {
            let cg = CGDisplayBounds(self.0);

            Bounds {
                origin: crate::point(
                    px(finite_native_coordinate(cg.origin.x)),
                    px(finite_native_coordinate(cg.origin.y)),
                ),
                size: size(
                    px(finite_native_dimension(cg.size.width)),
                    px(finite_native_dimension(cg.size.height)),
                ),
            }
        }
    }

    fn refresh_rate(&self) -> Option<f32> {
        // `CGDisplayModeGetRefreshRate` reports 0.0 for displays that do not advertise a
        // fixed rate (notably some built-in panels), so a zero is treated as "unknown".
        let rate = CGDisplay::new(self.0).display_mode()?.refresh_rate();
        if rate.is_finite() && rate > 0.0 && rate <= f32::MAX as f64 {
            Some(rate as f32)
        } else {
            None
        }
    }
}
