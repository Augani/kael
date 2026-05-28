use crate::{
    DevicePixels, ForegroundExecutor, SharedString, SourceMetadata,
    platform::{ScreenCaptureFrame, ScreenCaptureSource, ScreenCaptureStream},
    size,
};
use anyhow::{Result, anyhow};
use block2::RcBlock;
use collections::HashMap;
use core_foundation::base::TCFType;
use core_graphics::display::{
    CGDirectDisplayID, CGDisplayBounds, CGDisplayCopyDisplayMode, CGDisplayModeGetPixelHeight,
    CGDisplayModeGetPixelWidth, CGDisplayModeRelease, CGGetActiveDisplayList,
};
use futures::channel::oneshot;
use media::core_media::{CMSampleBuffer, CMSampleBufferRef};
use metal::NSInteger;
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject};
use objc2::{AnyThread, DefinedClass, define_class, msg_send};
use objc2_foundation::{NSObject, NSObjectProtocol, NSString};
use std::ffi::CStr;
use std::{cell::RefCell, ffi::c_void, ptr, rc::Rc};

#[derive(Clone)]
pub struct MacScreenCaptureSource {
    sc_display: *mut AnyObject,
    meta: Option<ScreenMeta>,
}

pub struct MacScreenCaptureStream {
    sc_stream: *mut AnyObject,
    sc_stream_output: *mut AnyObject,
    meta: SourceMetadata,
}

const FRAME_CALLBACK_IVAR: &str = "frame_callback";

#[allow(non_upper_case_globals)]
const SCStreamOutputTypeScreen: NSInteger = 0;

unsafe fn lookup_class(name: &CStr) -> &'static AnyClass {
    AnyClass::get(name).unwrap_or_else(|| panic!("missing class {name:?}"))
}

unsafe fn ns_string_to_str(s: *mut AnyObject) -> String {
    if s.is_null() {
        return String::new();
    }
    let ns: &NSString = unsafe { &*(s as *const NSString) };
    ns.to_string()
}

#[derive(Default)]
struct StreamDelegateIvars;

define_class!(
    #[unsafe(super = NSObject)]
    #[ivars = StreamDelegateIvars]
    #[name = "GPUIStreamDelegate"]
    struct GPUIStreamDelegate;

    unsafe impl NSObjectProtocol for GPUIStreamDelegate {}

    impl GPUIStreamDelegate {
        #[unsafe(method(outputVideoEffectDidStartForStream:))]
        fn output_video_effect_did_start_for_stream(&self, _stream: *mut AnyObject) {}

        #[unsafe(method(outputVideoEffectDidStopForStream:))]
        fn output_video_effect_did_stop_for_stream(&self, _stream: *mut AnyObject) {}

        #[unsafe(method(stream:didStopWithError:))]
        fn stream_did_stop_with_error(
            &self,
            _stream: *mut AnyObject,
            _error: *mut AnyObject,
        ) {
        }
    }
);

impl GPUIStreamDelegate {
    fn new() -> Retained<Self> {
        let this = Self::alloc().set_ivars(StreamDelegateIvars);
        unsafe { msg_send![super(this), init] }
    }
}

#[derive(Default)]
struct StreamOutputIvars {
    callback: std::cell::Cell<*mut c_void>,
}

define_class!(
    #[unsafe(super = NSObject)]
    #[ivars = StreamOutputIvars]
    #[name = "GPUIStreamOutput"]
    struct GPUIStreamOutput;

    unsafe impl NSObjectProtocol for GPUIStreamOutput {}

    impl GPUIStreamOutput {
        #[unsafe(method(stream:didOutputSampleBuffer:ofType:))]
        fn stream_did_output_sample_buffer_of_type(
            &self,
            _stream: *mut AnyObject,
            sample_buffer: *mut AnyObject,
            buffer_type: NSInteger,
        ) {
            if buffer_type != SCStreamOutputTypeScreen {
                return;
            }

            let cb_ptr = self.ivars().callback.get();
            if cb_ptr.is_null() {
                return;
            }

            unsafe {
                let sample_buffer = sample_buffer as CMSampleBufferRef;
                let sample_buffer = CMSampleBuffer::wrap_under_get_rule(sample_buffer);
                if let Some(buffer) = sample_buffer.image_buffer() {
                    let callback: Box<Box<dyn Fn(ScreenCaptureFrame)>> =
                        Box::from_raw(cb_ptr as *mut _);
                    callback(ScreenCaptureFrame(buffer));
                    std::mem::forget(callback);
                }
            }
        }
    }
);

impl GPUIStreamOutput {
    fn new(callback: *mut c_void) -> Retained<Self> {
        let this = Self::alloc().set_ivars(StreamOutputIvars {
            callback: std::cell::Cell::new(callback),
        });
        unsafe { msg_send![super(this), init] }
    }
}

impl ScreenCaptureSource for MacScreenCaptureSource {
    fn metadata(&self) -> Result<SourceMetadata> {
        let (display_id, size) = unsafe {
            let display_id: CGDirectDisplayID = msg_send![self.sc_display, displayID];
            let display_mode_ref = CGDisplayCopyDisplayMode(display_id);
            let width = CGDisplayModeGetPixelWidth(display_mode_ref);
            let height = CGDisplayModeGetPixelHeight(display_mode_ref);
            CGDisplayModeRelease(display_mode_ref);

            (
                display_id,
                size(DevicePixels(width as i32), DevicePixels(height as i32)),
            )
        };
        let (label, is_main) = self
            .meta
            .clone()
            .map(|meta| (meta.label, meta.is_main))
            .unzip();

        Ok(SourceMetadata {
            id: display_id as u64,
            label,
            is_main,
            resolution: size,
        })
    }

    fn stream(
        &self,
        _foreground_executor: &ForegroundExecutor,
        frame_callback: Box<dyn Fn(ScreenCaptureFrame) + Send>,
    ) -> oneshot::Receiver<Result<Box<dyn ScreenCaptureStream>>> {
        unsafe {
            let stream_alloc: *mut AnyObject = msg_send![lookup_class(c"SCStream"), alloc];
            let filter_alloc: *mut AnyObject = msg_send![lookup_class(c"SCContentFilter"), alloc];
            let configuration_alloc: *mut AnyObject =
                msg_send![lookup_class(c"SCStreamConfiguration"), alloc];

            let excluded_windows: *mut AnyObject = msg_send![lookup_class(c"NSArray"), array];
            let filter: *mut AnyObject = msg_send![
                filter_alloc,
                initWithDisplay: self.sc_display,
                excludingWindows: excluded_windows
            ];
            let configuration: *mut AnyObject = msg_send![configuration_alloc, init];
            let _: *mut AnyObject = msg_send![configuration, setScalesToFit: true];
            let _: *mut AnyObject = msg_send![configuration, setPixelFormat: 0x42475241_u32];

            let delegate = GPUIStreamDelegate::new();
            let callback_ptr = Box::into_raw(Box::new(frame_callback)) as *mut c_void;
            let output = GPUIStreamOutput::new(callback_ptr);

            let meta = self.metadata().unwrap();
            let _: *mut AnyObject =
                msg_send![configuration, setWidth: meta.resolution.width.0 as i64];
            let _: *mut AnyObject =
                msg_send![configuration, setHeight: meta.resolution.height.0 as i64];
            let stream: *mut AnyObject = msg_send![
                stream_alloc,
                initWithFilter: filter,
                configuration: configuration,
                delegate: &*delegate
            ];

            let (tx, rx) = oneshot::channel();

            let mut error: *mut AnyObject = ptr::null_mut();
            let null_q: *mut c_void = ptr::null_mut();
            let _: () = msg_send![
                stream,
                addStreamOutput: &*output,
                type: SCStreamOutputTypeScreen,
                sampleHandlerQueue: null_q,
                error: &mut error
            ];
            if !error.is_null() {
                let message: *mut AnyObject = msg_send![error, localizedDescription];
                let msg = ns_string_to_str(message);
                tx.send(Err(anyhow!("failed to add stream output {msg}")))
                    .ok();
                return rx;
            }

            // Retain stream/output so they outlive this scope; the returned
            // MacScreenCaptureStream owns them and releases on drop.
            let stream_owned: *mut AnyObject = msg_send![stream, retain];
            let output_owned: *mut AnyObject = msg_send![&*output, retain];
            // Keep the delegate alive on the heap so SCStream's weak reference stays valid.
            let _delegate_keep = Box::leak(Box::new(delegate));

            let tx = Rc::new(RefCell::new(Some(tx)));
            let handler = RcBlock::new({
                let tx = tx.clone();
                move |error: *mut AnyObject| {
                    let result = if error.is_null() {
                        let stream = MacScreenCaptureStream {
                            meta: meta.clone(),
                            sc_stream: stream_owned,
                            sc_stream_output: output_owned,
                        };
                        Ok(Box::new(stream) as Box<dyn ScreenCaptureStream>)
                    } else {
                        let message: *mut AnyObject =
                            unsafe { msg_send![error, localizedDescription] };
                        let msg = unsafe { ns_string_to_str(message) };
                        Err(anyhow!("failed to start screen capture stream {msg}"))
                    };
                    if let Some(tx) = tx.borrow_mut().take() {
                        tx.send(result).ok();
                    }
                }
            });
            let _: () = msg_send![stream, startCaptureWithCompletionHandler: &*handler];
            rx
        }
    }
}

impl Drop for MacScreenCaptureSource {
    fn drop(&mut self) {
        unsafe {
            let _: () = msg_send![self.sc_display, release];
        }
    }
}

impl ScreenCaptureStream for MacScreenCaptureStream {
    fn metadata(&self) -> Result<SourceMetadata> {
        Ok(self.meta.clone())
    }
}

impl Drop for MacScreenCaptureStream {
    fn drop(&mut self) {
        unsafe {
            let mut error: *mut AnyObject = ptr::null_mut();
            let _: () = msg_send![
                self.sc_stream,
                removeStreamOutput: self.sc_stream_output,
                type: SCStreamOutputTypeScreen,
                error: &mut error
            ];
            if !error.is_null() {
                let message: *mut AnyObject = msg_send![error, localizedDescription];
                log::error!(
                    "failed to remove stream output {}",
                    ns_string_to_str(message)
                );
            }

            let handler = RcBlock::new(move |error: *mut AnyObject| {
                if !error.is_null() {
                    let message: *mut AnyObject = unsafe { msg_send![error, localizedDescription] };
                    log::error!("failed to stop screen capture stream {}", unsafe {
                        ns_string_to_str(message)
                    });
                }
            });
            let _: () = msg_send![self.sc_stream, stopCaptureWithCompletionHandler: &*handler];
            let _: () = msg_send![self.sc_stream, release];
            let _: () = msg_send![self.sc_stream_output, release];
        }
    }
}

#[derive(Clone)]
struct ScreenMeta {
    label: SharedString,
    // Is this the screen with menu bar?
    is_main: bool,
}

unsafe fn screen_id_to_human_label() -> HashMap<CGDirectDisplayID, ScreenMeta> {
    unsafe {
        let screens: *mut AnyObject = msg_send![lookup_class(c"NSScreen"), screens];
        let count: usize = msg_send![screens, count];
        let mut map = HashMap::default();
        let screen_number_key = NSString::from_str("NSScreenNumber");
        for i in 0..count {
            let screen: *mut AnyObject = msg_send![screens, objectAtIndex: i];
            let device_desc: *mut AnyObject = msg_send![screen, deviceDescription];
            if device_desc.is_null() {
                continue;
            }

            let nsnumber: *mut AnyObject =
                msg_send![device_desc, objectForKey: &*screen_number_key];
            if nsnumber.is_null() {
                continue;
            }

            let screen_id: u32 = msg_send![nsnumber, unsignedIntValue];

            let name: *mut AnyObject = msg_send![screen, localizedName];
            if !name.is_null() {
                let label = ns_string_to_str(name);
                map.insert(
                    screen_id,
                    ScreenMeta {
                        label: label.into(),
                        is_main: i == 0,
                    },
                );
            }
        }
        map
    }
}

pub(crate) fn get_sources() -> oneshot::Receiver<Result<Vec<Rc<dyn ScreenCaptureSource>>>> {
    unsafe {
        let (tx, rx) = oneshot::channel();
        let tx = Rc::new(RefCell::new(Some(tx)));
        let screen_id_to_label = screen_id_to_human_label();
        let block = RcBlock::new(
            move |shareable_content: *mut AnyObject, error: *mut AnyObject| {
                let Some(tx) = tx.borrow_mut().take() else {
                    return;
                };

                let result = if error.is_null() {
                    let displays: *mut AnyObject =
                        unsafe { msg_send![shareable_content, displays] };
                    let count: usize = unsafe { msg_send![displays, count] };
                    let mut result = Vec::new();
                    for i in 0..count {
                        let display: *mut AnyObject =
                            unsafe { msg_send![displays, objectAtIndex: i] };
                        let id: CGDirectDisplayID = unsafe { msg_send![display, displayID] };
                        let meta = screen_id_to_label.get(&id).cloned();
                        let retained: *mut AnyObject = unsafe { msg_send![display, retain] };
                        let source = MacScreenCaptureSource {
                            sc_display: retained,
                            meta,
                        };
                        result.push(Rc::new(source) as Rc<dyn ScreenCaptureSource>);
                    }
                    Ok(result)
                } else {
                    let msg: *mut AnyObject = unsafe { msg_send![error, localizedDescription] };
                    Err(anyhow!("Screen share failed: {}", unsafe {
                        ns_string_to_str(msg)
                    }))
                };
                tx.send(result).ok();
            },
        );

        let _: () = msg_send![
            lookup_class(c"SCShareableContent"),
            getShareableContentExcludingDesktopWindows: true,
            onScreenWindowsOnly: true,
            completionHandler: &*block
        ];
        rx
    }
}

use crate::media_capture::{
    CaptureBackend, CaptureConfig, CaptureDeviceInfo, CaptureDeviceKind, CaptureSession,
    CaptureSessionState, DeviceEnumerator, FrameCallback,
};
use std::sync::atomic::{AtomicU64, Ordering};

pub struct MacScreenCaptureBackend;

impl MacScreenCaptureBackend {
    pub fn new() -> Self {
        Self
    }
}

impl DeviceEnumerator for MacScreenCaptureBackend {
    fn devices(&self, kind: CaptureDeviceKind) -> Result<Vec<CaptureDeviceInfo>> {
        match kind {
            CaptureDeviceKind::Screen => {
                let mut count: u32 = 0;
                let result = unsafe { CGGetActiveDisplayList(0, ptr::null_mut(), &mut count) };
                if result != 0 {
                    return Ok(Vec::new());
                }
                let mut ids = vec![0u32; count as usize];
                let result = unsafe { CGGetActiveDisplayList(count, ids.as_mut_ptr(), &mut count) };
                if result != 0 {
                    return Ok(Vec::new());
                }
                ids.truncate(count as usize);
                let labels = unsafe { screen_id_to_human_label() };
                let devices = ids
                    .into_iter()
                    .map(|id| {
                        let _bounds = unsafe { CGDisplayBounds(id) };
                        let name = labels
                            .get(&id)
                            .map(|m| m.label.to_string())
                            .unwrap_or_else(|| format!("Display {}", id));
                        CaptureDeviceInfo {
                            id: id.to_string(),
                            name,
                            kind: CaptureDeviceKind::Screen,
                            is_available: true,
                        }
                    })
                    .collect();
                Ok(devices)
            }
            CaptureDeviceKind::Window => Ok(Vec::new()),
            _ => Ok(Vec::new()),
        }
    }
}

impl CaptureBackend for MacScreenCaptureBackend {
    fn create_session(&self, config: &CaptureConfig) -> Result<Box<dyn CaptureSession>> {
        Ok(Box::new(MacScreenCaptureSession::new(config.clone())))
    }
}

struct MacScreenCaptureSession {
    config: CaptureConfig,
    state: CaptureSessionState,
    dropped: AtomicU64,
    latency_ms: AtomicU64,
    callback: Option<FrameCallback>,
}

impl MacScreenCaptureSession {
    fn new(config: CaptureConfig) -> Self {
        Self {
            config,
            state: CaptureSessionState::Idle,
            dropped: AtomicU64::new(0),
            latency_ms: AtomicU64::new(0),
            callback: None,
        }
    }
}

impl CaptureSession for MacScreenCaptureSession {
    fn start(&mut self, config: CaptureConfig, callback: FrameCallback) -> Result<()> {
        self.config = config;
        self.state = CaptureSessionState::Starting;
        self.callback = Some(callback);
        self.state = CaptureSessionState::Running;
        Ok(())
    }

    fn pause(&mut self) -> Result<()> {
        self.state = CaptureSessionState::Paused;
        Ok(())
    }

    fn resume(&mut self) -> Result<()> {
        self.state = CaptureSessionState::Running;
        Ok(())
    }

    fn stop(&mut self) -> Result<()> {
        self.state = CaptureSessionState::Stopped;
        self.callback = None;
        Ok(())
    }

    fn state(&self) -> CaptureSessionState {
        self.state
    }

    fn dropped_frame_count(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }

    fn latency_ms(&self) -> u64 {
        self.latency_ms.load(Ordering::Relaxed)
    }
}
