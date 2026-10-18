use crate::{
    dispatch_get_main_queue,
    dispatch_sys::{
        _dispatch_source_type_data_add, dispatch_resume, dispatch_set_context,
        dispatch_source_cancel, dispatch_source_create, dispatch_source_merge_data,
        dispatch_source_set_event_handler_f, dispatch_source_t, dispatch_suspend,
    },
};
use anyhow::Result;
use core_graphics::display::CGDirectDisplayID;
use std::ffi::c_void;

pub struct DisplayLink {
    display_link: Option<sys::DisplayLink>,
    display_id: CGDirectDisplayID,
    frame_requests: dispatch_source_t,
    running: bool,
}

fn resume_retained_slot<T>(
    retained: &mut Option<T>,
    create: impl FnOnce() -> Result<T>,
    resume: impl FnOnce(&mut T) -> Result<()>,
) -> Result<()> {
    if retained.is_none() {
        *retained = Some(create()?);
    }
    // Keep a successfully constructed link even if starting/retargeting fails.
    // Subsequent refreshes retry it without constructing another native object.
    resume(retained.as_mut().expect("retained display link exists"))
}

impl DisplayLink {
    pub fn new(
        display_id: CGDirectDisplayID,
        data: *mut c_void,
        callback: unsafe extern "C" fn(*mut c_void),
    ) -> Result<DisplayLink> {
        unsafe extern "C" fn display_link_callback(
            _display_link_out: *mut sys::CVDisplayLink,
            _current_time: *const sys::CVTimeStamp,
            _output_time: *const sys::CVTimeStamp,
            _flags_in: i64,
            _flags_out: *mut i64,
            frame_requests: *mut c_void,
        ) -> i32 {
            unsafe {
                let frame_requests = frame_requests as dispatch_source_t;
                dispatch_source_merge_data(frame_requests, 1);
                0
            }
        }

        unsafe {
            let frame_requests = dispatch_source_create(
                &_dispatch_source_type_data_add,
                0,
                0,
                dispatch_get_main_queue(),
            );
            anyhow::ensure!(
                !frame_requests.is_null(),
                "unable to create frame dispatch source"
            );
            dispatch_set_context(
                crate::dispatch_sys::dispatch_object_t {
                    _ds: frame_requests,
                },
                data,
            );
            dispatch_source_set_event_handler_f(frame_requests, Some(callback));

            let display_link = match sys::DisplayLink::new(
                display_id,
                display_link_callback,
                frame_requests as *mut c_void,
            ) {
                Ok(display_link) => display_link,
                Err(error) => {
                    dispatch_source_cancel(frame_requests);
                    dispatch_resume(crate::dispatch_sys::dispatch_object_t {
                        _ds: frame_requests,
                    });
                    return Err(error);
                }
            };

            Ok(Self {
                display_link: Some(display_link),
                display_id,
                frame_requests,
                running: false,
            })
        }
    }

    /// Keep one native link for this window, including while it is idle. The
    /// callback context is borrowed; neither this slot nor CoreVideo owns the
    /// window. Only final window teardown runs the native-release workaround.
    pub(crate) fn resume_retained(
        retained: &mut Option<Self>,
        display_id: CGDirectDisplayID,
        data: *mut c_void,
        callback: unsafe extern "C" fn(*mut c_void),
    ) -> Result<()> {
        resume_retained_slot(
            retained,
            || Self::new(display_id, data, callback),
            |link| {
                if link.display_id != display_id {
                    // Pause before changing the native display. Failure retains
                    // the same object for a later refresh to retry.
                    link.stop()?;
                    let native = link
                        .display_link
                        .as_mut()
                        .ok_or_else(|| anyhow::anyhow!("display link is unavailable"))?;
                    unsafe { native.set_display(display_id)? };
                    link.display_id = display_id;
                }
                link.start()
            },
        )
    }

    pub(crate) fn is_running(&self) -> bool {
        self.running
    }

    pub fn start(&mut self) -> Result<()> {
        if self.running {
            return Ok(());
        }
        let display_link = self
            .display_link
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("display link is unavailable"))?;
        unsafe { display_link.start()? };
        unsafe {
            dispatch_resume(crate::dispatch_sys::dispatch_object_t {
                _ds: self.frame_requests,
            });
        }
        self.running = true;
        Ok(())
    }

    pub fn stop(&mut self) -> Result<()> {
        if !self.running {
            return Ok(());
        }
        let display_link = self
            .display_link
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("display link is unavailable"))?;
        unsafe { display_link.stop()? };
        unsafe {
            dispatch_suspend(crate::dispatch_sys::dispatch_object_t {
                _ds: self.frame_requests,
            });
        }
        self.running = false;
        Ok(())
    }
}

impl Drop for DisplayLink {
    fn drop(&mut self) {
        let source_is_suspended = if self.running {
            match self.stop() {
                Ok(()) => true,
                Err(error) => {
                    log::error!("failed to stop display link during teardown: {error:#}");
                    false
                }
            }
        } else {
            true
        };
        // We see occasional segfaults on the CVDisplayLink thread.
        //
        // It seems possible that this happens because CVDisplayLinkRelease releases the CVDisplayLink
        // on the main thread immediately, but the background thread that CVDisplayLink uses for timers
        // is still accessing it.
        //
        // We might also want to upgrade to CADisplayLink, but that requires dropping old macOS support.
        std::mem::forget(self.display_link.take());
        unsafe {
            dispatch_source_cancel(self.frame_requests);
            if source_is_suspended {
                dispatch_resume(crate::dispatch_sys::dispatch_object_t {
                    _ds: self.frame_requests,
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::Cell, sync::Arc};

    #[derive(Default)]
    struct FakeLink {
        starts: usize,
        running: bool,
    }

    #[test]
    fn retained_display_link_idle_wakes_reuse_one_construction_and_release_factory_captures() {
        let constructions = Cell::new(0);
        let owner = Arc::new(());
        let weak_owner = Arc::downgrade(&owner);
        let mut retained = None;
        for _ in 0..1_024 {
            // A factory may capture its caller. The reuse helper must drop even
            // an unused factory immediately rather than retaining the window.
            let factory_owner = owner.clone();
            resume_retained_slot(
                &mut retained,
                || {
                    let _owner = factory_owner;
                    constructions.set(constructions.get() + 1);
                    Ok(FakeLink::default())
                },
                |link| {
                    link.starts += 1;
                    link.running = true;
                    Ok(())
                },
            )
            .unwrap();
            assert_eq!(Arc::strong_count(&owner), 1);
            let link = retained.as_mut().unwrap();
            assert!(link.running);
            link.running = false;
        }
        assert_eq!(constructions.get(), 1);
        assert_eq!(retained.unwrap().starts, 1_024);
        drop(owner);
        assert!(weak_owner.upgrade().is_none());
    }

    #[test]
    fn retained_display_link_start_failure_retries_the_existing_object() {
        let mut retained = None;
        assert!(
            resume_retained_slot(
                &mut retained,
                || Ok(FakeLink::default()),
                |link| {
                    link.starts += 1;
                    anyhow::bail!("transient native start failure")
                },
            )
            .is_err()
        );
        assert!(!retained.as_ref().unwrap().running);
        resume_retained_slot(
            &mut retained,
            || panic!("a start failure must not recreate the link"),
            |link| {
                link.starts += 1;
                link.running = true;
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(retained.unwrap().starts, 2);
    }

    #[test]
    fn retained_display_link_construction_failure_leaves_a_retryable_empty_slot() {
        let mut retained: Option<FakeLink> = None;
        assert!(
            resume_retained_slot(
                &mut retained,
                || anyhow::bail!("display unavailable"),
                |_| panic!("cannot start a failed construction"),
            )
            .is_err()
        );
        assert!(retained.is_none());
        resume_retained_slot(&mut retained, || Ok(FakeLink::default()), |_| Ok(())).unwrap();
        assert!(retained.is_some());
    }

    #[test]
    #[ignore = "explicit runtime probe: requires an awake native macOS display"]
    fn native_retained_display_link_probe() {
        use core_graphics::display::{CGGetActiveDisplayList, CGMainDisplayID};
        use foreign_types::ForeignType;

        unsafe extern "C" fn noop(_: *mut c_void) {}

        let display_id = unsafe { CGMainDisplayID() };
        assert_ne!(display_id, 0, "runtime probe requires a real main display");
        let mut retained = None;
        DisplayLink::resume_retained(&mut retained, display_id, std::ptr::null_mut(), noop)
            .expect("runtime probe must construct and start a native display link");
        let identity = retained
            .as_ref()
            .unwrap()
            .display_link
            .as_ref()
            .unwrap()
            .as_ptr();
        eprintln!("NATIVE_DISPLAY_LINK_PROBE_BEGIN: display={display_id} native={identity:p}");
        const CYCLES: usize = 128;
        for _ in 0..CYCLES {
            let link = retained.as_mut().unwrap();
            assert!(link.is_running());
            link.stop().expect("native display link must stop");
            assert!(!link.is_running());
            link.stop().expect("repeated pause must be idempotent");
            DisplayLink::resume_retained(&mut retained, display_id, std::ptr::null_mut(), noop)
                .expect("native display link must resume");
            assert!(retained.as_ref().unwrap().is_running());
            assert_eq!(
                retained
                    .as_ref()
                    .unwrap()
                    .display_link
                    .as_ref()
                    .unwrap()
                    .as_ptr(),
                identity,
                "an idle wake constructed a different native object"
            );
        }

        let mut displays = [0; 32];
        let mut count = 0;
        assert_eq!(
            unsafe {
                CGGetActiveDisplayList(displays.len() as u32, displays.as_mut_ptr(), &mut count)
            },
            0,
            "runtime probe must enumerate active displays"
        );
        assert!(count > 0 && count as usize <= displays.len());
        let mut retargets = 0;
        for other in displays[..count as usize]
            .iter()
            .copied()
            .filter(|other| *other != display_id)
            .chain(std::iter::once(display_id))
        {
            DisplayLink::resume_retained(&mut retained, other, std::ptr::null_mut(), noop)
                .expect("native display retarget must succeed");
            assert!(retained.as_ref().unwrap().is_running());
            assert_eq!(
                retained
                    .as_ref()
                    .unwrap()
                    .display_link
                    .as_ref()
                    .unwrap()
                    .as_ptr(),
                identity
            );
            retargets += usize::from(other != display_id);
        }
        retained.as_mut().unwrap().stop().unwrap();
        assert!(!retained.as_ref().unwrap().is_running());
        eprintln!(
            "NATIVE_DISPLAY_LINK_PROBE_OK: cycles={CYCLES} native={identity:p} constructions=1 stopped={CYCLES} resumed={CYCLES} other_display_retargets={retargets} final_stopped=true"
        );
    }
}

mod sys {
    //! Derived from display-link crate under the following license:
    //! <https://github.com/BrainiumLLC/display-link/blob/master/LICENSE-MIT>
    //! Apple docs: [CVDisplayLink](https://developer.apple.com/documentation/corevideo/cvdisplaylinkoutputcallback?language=objc)
    #![allow(dead_code, non_upper_case_globals)]

    use anyhow::Result;
    use core_graphics::display::CGDirectDisplayID;
    use foreign_types::{ForeignType, foreign_type};
    use std::{
        ffi::c_void,
        fmt::{self, Debug, Formatter},
    };

    #[derive(Debug)]
    pub enum CVDisplayLink {}

    foreign_type! {
        pub unsafe type DisplayLink {
            type CType = CVDisplayLink;
            fn drop = CVDisplayLinkRelease;
            fn clone = CVDisplayLinkRetain;
        }
    }

    impl Debug for DisplayLink {
        fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
            formatter
                .debug_tuple("DisplayLink")
                .field(&self.as_ptr())
                .finish()
        }
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    pub(crate) struct CVTimeStamp {
        pub version: u32,
        pub video_time_scale: i32,
        pub video_time: i64,
        pub host_time: u64,
        pub rate_scalar: f64,
        pub video_refresh_period: i64,
        pub smpte_time: CVSMPTETime,
        pub flags: u64,
        pub reserved: u64,
    }

    pub type CVTimeStampFlags = u64;

    pub const kCVTimeStampVideoTimeValid: CVTimeStampFlags = 1 << 0;
    pub const kCVTimeStampHostTimeValid: CVTimeStampFlags = 1 << 1;
    pub const kCVTimeStampSMPTETimeValid: CVTimeStampFlags = 1 << 2;
    pub const kCVTimeStampVideoRefreshPeriodValid: CVTimeStampFlags = 1 << 3;
    pub const kCVTimeStampRateScalarValid: CVTimeStampFlags = 1 << 4;
    pub const kCVTimeStampTopField: CVTimeStampFlags = 1 << 16;
    pub const kCVTimeStampBottomField: CVTimeStampFlags = 1 << 17;
    pub const kCVTimeStampVideoHostTimeValid: CVTimeStampFlags =
        kCVTimeStampVideoTimeValid | kCVTimeStampHostTimeValid;
    pub const kCVTimeStampIsInterlaced: CVTimeStampFlags =
        kCVTimeStampTopField | kCVTimeStampBottomField;

    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    pub(crate) struct CVSMPTETime {
        pub subframes: i16,
        pub subframe_divisor: i16,
        pub counter: u32,
        pub time_type: u32,
        pub flags: u32,
        pub hours: i16,
        pub minutes: i16,
        pub seconds: i16,
        pub frames: i16,
    }

    pub type CVSMPTETimeType = u32;

    pub const kCVSMPTETimeType24: CVSMPTETimeType = 0;
    pub const kCVSMPTETimeType25: CVSMPTETimeType = 1;
    pub const kCVSMPTETimeType30Drop: CVSMPTETimeType = 2;
    pub const kCVSMPTETimeType30: CVSMPTETimeType = 3;
    pub const kCVSMPTETimeType2997: CVSMPTETimeType = 4;
    pub const kCVSMPTETimeType2997Drop: CVSMPTETimeType = 5;
    pub const kCVSMPTETimeType60: CVSMPTETimeType = 6;
    pub const kCVSMPTETimeType5994: CVSMPTETimeType = 7;

    pub type CVSMPTETimeFlags = u32;

    pub const kCVSMPTETimeValid: CVSMPTETimeFlags = 1 << 0;
    pub const kCVSMPTETimeRunning: CVSMPTETimeFlags = 1 << 1;

    pub type CVDisplayLinkOutputCallback = unsafe extern "C" fn(
        display_link_out: *mut CVDisplayLink,
        // A pointer to the current timestamp. This represents the timestamp when the callback is called.
        current_time: *const CVTimeStamp,
        // A pointer to the output timestamp. This represents the timestamp for when the frame will be displayed.
        output_time: *const CVTimeStamp,
        // Unused
        flags_in: i64,
        // Unused
        flags_out: *mut i64,
        // A pointer to app-defined data.
        display_link_context: *mut c_void,
    ) -> i32;

    #[link(name = "CoreFoundation", kind = "framework")]
    #[link(name = "CoreVideo", kind = "framework")]
    #[allow(improper_ctypes, unknown_lints, clippy::duplicated_attributes)]
    unsafe extern "C" {
        pub fn CVDisplayLinkCreateWithActiveCGDisplays(
            display_link_out: *mut *mut CVDisplayLink,
        ) -> i32;
        pub fn CVDisplayLinkSetCurrentCGDisplay(
            display_link: &mut DisplayLinkRef,
            display_id: u32,
        ) -> i32;
        pub fn CVDisplayLinkSetOutputCallback(
            display_link: &mut DisplayLinkRef,
            callback: CVDisplayLinkOutputCallback,
            user_info: *mut c_void,
        ) -> i32;
        pub fn CVDisplayLinkStart(display_link: &mut DisplayLinkRef) -> i32;
        pub fn CVDisplayLinkStop(display_link: &mut DisplayLinkRef) -> i32;
        pub fn CVDisplayLinkRelease(display_link: *mut CVDisplayLink);
        pub fn CVDisplayLinkRetain(display_link: *mut CVDisplayLink) -> *mut CVDisplayLink;
    }

    impl DisplayLink {
        /// Apple docs: [CVDisplayLinkCreateWithCGDisplay](https://developer.apple.com/documentation/corevideo/1456981-cvdisplaylinkcreatewithcgdisplay?language=objc)
        pub unsafe fn new(
            display_id: CGDirectDisplayID,
            callback: CVDisplayLinkOutputCallback,
            user_info: *mut c_void,
        ) -> Result<Self> {
            unsafe {
                let mut display_link: *mut CVDisplayLink = 0 as _;

                let code = CVDisplayLinkCreateWithActiveCGDisplays(&mut display_link);
                anyhow::ensure!(code == 0, "could not create display link, code: {}", code);

                let mut display_link = DisplayLink::from_ptr(display_link);

                let code = CVDisplayLinkSetOutputCallback(&mut display_link, callback, user_info);
                anyhow::ensure!(code == 0, "could not set output callback, code: {}", code);

                let code = CVDisplayLinkSetCurrentCGDisplay(&mut display_link, display_id);
                anyhow::ensure!(
                    code == 0,
                    "could not assign display to display link, code: {}",
                    code
                );

                Ok(display_link)
            }
        }
    }

    impl DisplayLinkRef {
        pub unsafe fn set_display(&mut self, display_id: CGDirectDisplayID) -> Result<()> {
            let code = unsafe { CVDisplayLinkSetCurrentCGDisplay(self, display_id) };
            anyhow::ensure!(
                code == 0,
                "could not assign display to display link, code: {}",
                code
            );
            Ok(())
        }

        /// Apple docs: [CVDisplayLinkStart](https://developer.apple.com/documentation/corevideo/1457193-cvdisplaylinkstart?language=objc)
        pub unsafe fn start(&mut self) -> Result<()> {
            unsafe {
                let code = CVDisplayLinkStart(self);
                anyhow::ensure!(code == 0, "could not start display link, code: {}", code);
                Ok(())
            }
        }

        /// Apple docs: [CVDisplayLinkStop](https://developer.apple.com/documentation/corevideo/1457281-cvdisplaylinkstop?language=objc)
        pub unsafe fn stop(&mut self) -> Result<()> {
            unsafe {
                let code = CVDisplayLinkStop(self);
                anyhow::ensure!(code == 0, "could not stop display link, code: {}", code);
                Ok(())
            }
        }
    }
}
