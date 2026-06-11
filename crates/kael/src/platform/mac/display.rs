use crate::{Bounds, DisplayId, Pixels, PlatformDisplay, px, size};
use anyhow::Result;
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

            if result == 0 {
                displays.set_len(display_count as usize);
                displays.into_iter().map(MacDisplay)
            } else {
                panic!("Failed to get active display list. Result: {result}");
            }
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
        Ok(Uuid::from_bytes([
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
        ]))
    }

    fn bounds(&self) -> Bounds<Pixels> {
        unsafe {
            let cg = CGDisplayBounds(self.0);

            Bounds {
                origin: crate::point(px(cg.origin.x as f32), px(cg.origin.y as f32)),
                size: size(px(cg.size.width as f32), px(cg.size.height as f32)),
            }
        }
    }

    fn refresh_rate(&self) -> Option<f32> {
        // `CGDisplayModeGetRefreshRate` reports 0.0 for displays that do not advertise a
        // fixed rate (notably some built-in panels), so a zero is treated as "unknown".
        let rate = CGDisplay::new(self.0).display_mode()?.refresh_rate();
        if rate > 0.0 { Some(rate as f32) } else { None }
    }
}
