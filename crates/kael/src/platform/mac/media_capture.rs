use crate::media_capture::{
    CaptureBackend, CaptureConfig, CaptureDeviceInfo, CaptureDeviceKind, CaptureSession,
    CaptureSessionState, DeviceEnumerator, FrameCallback, PixelFormat,
};
use anyhow::{Result, anyhow};
use core_foundation::{base::TCFType, string::CFStringRef};
use core_video::image_buffer::CVImageBufferRef;
use media::{
    core_media::{CMSampleBuffer, CMSampleBufferRef},
    core_video::kCVPixelFormatType_32BGRA,
};
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject};
use objc2::{AnyThread, DefinedClass, define_class, msg_send};
use objc2_foundation::{NSObject, NSObjectProtocol, NSString};
use std::cell::Cell;
use std::{
    ffi::{CStr, c_void},
    ptr,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Instant,
};

use super::dispatcher::dispatch_sys::{DISPATCH_QUEUE_PRIORITY_HIGH, dispatch_get_global_queue};

#[link(name = "AVFoundation", kind = "framework")]
unsafe extern "C" {
    static AVMediaTypeAudio: *mut AnyObject;
    static AVMediaTypeVideo: *mut AnyObject;
}

#[link(name = "CoreVideo", kind = "framework")]
unsafe extern "C" {
    static kCVPixelBufferPixelFormatTypeKey: CFStringRef;

    fn CVPixelBufferLockBaseAddress(pixel_buffer: CVImageBufferRef, lock_flags: u64) -> i32;
    fn CVPixelBufferUnlockBaseAddress(pixel_buffer: CVImageBufferRef, lock_flags: u64) -> i32;
    fn CVPixelBufferGetWidth(pixel_buffer: CVImageBufferRef) -> usize;
    fn CVPixelBufferGetHeight(pixel_buffer: CVImageBufferRef) -> usize;
    fn CVPixelBufferGetBytesPerRow(pixel_buffer: CVImageBufferRef) -> usize;
    fn CVPixelBufferGetBaseAddress(pixel_buffer: CVImageBufferRef) -> *mut c_void;
    fn CVPixelBufferGetPixelFormatType(pixel_buffer: CVImageBufferRef) -> u32;
}

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

unsafe fn release_obj(object: *mut AnyObject) {
    if !object.is_null() {
        unsafe {
            let _: () = msg_send![object, release];
        }
    }
}

#[derive(Default)]
struct VideoOutputDelegateIvars {
    state: Cell<*mut c_void>,
}

define_class!(
    #[unsafe(super = NSObject)]
    #[ivars = VideoOutputDelegateIvars]
    #[name = "GPUICameraVideoOutputDelegate"]
    struct GPUICameraVideoOutputDelegate;

    unsafe impl NSObjectProtocol for GPUICameraVideoOutputDelegate {}

    impl GPUICameraVideoOutputDelegate {
        #[unsafe(method(captureOutput:didOutputSampleBuffer:fromConnection:))]
        fn did_output(
            &self,
            _output: *mut AnyObject,
            sample_buffer: *mut AnyObject,
            _connection: *mut AnyObject,
        ) {
            let state_ptr = self.ivars().state.get();
            if state_ptr.is_null() || sample_buffer.is_null() {
                return;
            }

            let start = Instant::now();
            let state = unsafe { &*(state_ptr as *const VideoOutputState) };
            unsafe {
                let sample_buffer =
                    CMSampleBuffer::wrap_under_get_rule(sample_buffer as CMSampleBufferRef);
                let Some(image_buffer) = sample_buffer.image_buffer() else {
                    state.dropped.fetch_add(1, Ordering::Relaxed);
                    return;
                };

                let pixel_buffer = image_buffer.as_concrete_TypeRef();
                if CVPixelBufferLockBaseAddress(pixel_buffer, 0) != 0 {
                    state.dropped.fetch_add(1, Ordering::Relaxed);
                    return;
                }

                let pixel_format = CVPixelBufferGetPixelFormatType(pixel_buffer);
                if pixel_format != kCVPixelFormatType_32BGRA {
                    let _ = CVPixelBufferUnlockBaseAddress(pixel_buffer, 0);
                    state.dropped.fetch_add(1, Ordering::Relaxed);
                    return;
                }

                let width = CVPixelBufferGetWidth(pixel_buffer) as u32;
                let height = CVPixelBufferGetHeight(pixel_buffer) as u32;
                let bytes_per_row = CVPixelBufferGetBytesPerRow(pixel_buffer);
                let base_address = CVPixelBufferGetBaseAddress(pixel_buffer) as *const u8;
                if base_address.is_null() {
                    let _ = CVPixelBufferUnlockBaseAddress(pixel_buffer, 0);
                    state.dropped.fetch_add(1, Ordering::Relaxed);
                    return;
                }

                let expected_bytes_per_row = width as usize * 4;
                let mut data = vec![0u8; expected_bytes_per_row * height as usize];
                for row in 0..height as usize {
                    let source = base_address.add(row * bytes_per_row);
                    let target = &mut data
                        [row * expected_bytes_per_row..(row + 1) * expected_bytes_per_row];
                    ptr::copy_nonoverlapping(source, target.as_mut_ptr(), expected_bytes_per_row);
                }

                let _ = CVPixelBufferUnlockBaseAddress(pixel_buffer, 0);

                let timestamp_ms = sample_buffer
                    .sample_timing_info(0)
                    .map(|timing| {
                        cmtime_to_millis(
                            timing.presentationTimeStamp.value,
                            timing.presentationTimeStamp.timescale,
                        )
                    })
                    .unwrap_or_default();

                (state.callback)(crate::media_capture::CaptureFrame::Video {
                    width,
                    height,
                    format: PixelFormat::Bgra32,
                    data: Arc::new(data),
                    timestamp_ms,
                });
                state
                    .latency_ms
                    .store(start.elapsed().as_millis() as u64, Ordering::Relaxed);
            }
        }

        #[unsafe(method(captureOutput:didDropSampleBuffer:fromConnection:))]
        fn did_drop(
            &self,
            _output: *mut AnyObject,
            _sample_buffer: *mut AnyObject,
            _connection: *mut AnyObject,
        ) {
            let state_ptr = self.ivars().state.get();
            if state_ptr.is_null() {
                return;
            }
            let state = unsafe { &*(state_ptr as *const VideoOutputState) };
            state.dropped.fetch_add(1, Ordering::Relaxed);
        }
    }
);

impl GPUICameraVideoOutputDelegate {
    fn new(state_ptr: *mut c_void) -> Retained<Self> {
        let this = Self::alloc().set_ivars(VideoOutputDelegateIvars {
            state: Cell::new(state_ptr),
        });
        unsafe { msg_send![super(this), init] }
    }

    fn take_state(&self) -> *mut c_void {
        let ptr = self.ivars().state.replace(ptr::null_mut());
        ptr
    }
}

pub struct MacMediaCaptureBackend;

impl MacMediaCaptureBackend {
    pub fn new() -> Self {
        Self
    }
}

impl DeviceEnumerator for MacMediaCaptureBackend {
    fn devices(&self, kind: CaptureDeviceKind) -> Result<Vec<CaptureDeviceInfo>> {
        match kind {
            CaptureDeviceKind::Camera => enumerate_devices(kind, unsafe { AVMediaTypeVideo }),
            CaptureDeviceKind::Microphone => enumerate_devices(kind, unsafe { AVMediaTypeAudio }),
            _ => Ok(Vec::new()),
        }
    }
}

impl CaptureBackend for MacMediaCaptureBackend {
    fn create_session(&self, config: &CaptureConfig) -> Result<Box<dyn CaptureSession>> {
        match config.kind {
            CaptureDeviceKind::Camera => Ok(Box::new(MacCameraCaptureSession::new(config.clone()))),
            CaptureDeviceKind::Microphone => {
                Ok(Box::new(MacMicrophoneCaptureSession::new(config.clone())))
            }
            _ => Err(anyhow!(
                "MacMediaCaptureBackend does not support {:?}",
                config.kind
            )),
        }
    }
}

struct MacCameraCaptureSession {
    config: CaptureConfig,
    state: CaptureSessionState,
    dropped: Arc<AtomicU64>,
    latency_ms: Arc<AtomicU64>,
    capture_session: Option<*mut AnyObject>,
    capture_input: Option<*mut AnyObject>,
    video_output: Option<*mut AnyObject>,
    video_output_delegate: Option<Retained<GPUICameraVideoOutputDelegate>>,
}

impl MacCameraCaptureSession {
    fn new(config: CaptureConfig) -> Self {
        Self {
            config,
            state: CaptureSessionState::Idle,
            dropped: Arc::new(AtomicU64::new(0)),
            latency_ms: Arc::new(AtomicU64::new(0)),
            capture_session: None,
            capture_input: None,
            video_output: None,
            video_output_delegate: None,
        }
    }
}

// AVFoundation values here are only moved between Rust owners; actual capture
// callbacks stay on the dispatch queue configured in `start`.
unsafe impl Send for MacCameraCaptureSession {}

impl CaptureSession for MacCameraCaptureSession {
    fn start(&mut self, config: CaptureConfig, callback: FrameCallback) -> Result<()> {
        let _ = self.stop();
        self.config = config;
        self.state = CaptureSessionState::Starting;

        self.dropped.store(0, Ordering::Relaxed);
        self.latency_ms.store(0, Ordering::Relaxed);

        unsafe {
            let session: *mut AnyObject = msg_send![lookup_class(c"AVCaptureSession"), new];
            let video_output: *mut AnyObject =
                msg_send![lookup_class(c"AVCaptureVideoDataOutput"), new];

            let delegate_state = Box::new(VideoOutputState {
                callback,
                dropped: Arc::clone(&self.dropped),
                latency_ms: Arc::clone(&self.latency_ms),
            });
            let state_ptr = Box::into_raw(delegate_state) as *mut c_void;
            let video_output_delegate = GPUICameraVideoOutputDelegate::new(state_ptr);

            let input = match create_camera_input(&self.config.device_id) {
                Ok(input) => input,
                Err(error) => {
                    drop(Box::from_raw(state_ptr as *mut VideoOutputState));
                    drop(video_output_delegate);
                    release_obj(video_output);
                    release_obj(session);
                    self.state = CaptureSessionState::Error;
                    return Err(error);
                }
            };

            if let Err(error) = configure_video_output(video_output) {
                release_obj(input);
                drop(Box::from_raw(state_ptr as *mut VideoOutputState));
                drop(video_output_delegate);
                release_obj(video_output);
                release_obj(session);
                self.state = CaptureSessionState::Error;
                return Err(error);
            }

            let can_add_input: bool = msg_send![session, canAddInput: input];
            let can_add_output: bool = msg_send![session, canAddOutput: video_output];
            if !can_add_input || !can_add_output {
                release_obj(input);
                drop(Box::from_raw(state_ptr as *mut VideoOutputState));
                drop(video_output_delegate);
                release_obj(video_output);
                release_obj(session);
                self.state = CaptureSessionState::Error;
                anyhow::bail!(
                    "AVFoundation camera session could not add the selected input/output"
                );
            }

            let _: () = msg_send![session, addInput: input];
            let _: () = msg_send![session, addOutput: video_output];
            let queue =
                dispatch_get_global_queue(DISPATCH_QUEUE_PRIORITY_HIGH.try_into().unwrap(), 0);
            let queue_obj = queue as *mut c_void;
            let _: () = msg_send![video_output, setSampleBufferDelegate: &*video_output_delegate, queue: queue_obj];
            let _: () = msg_send![session, startRunning];

            let is_running: bool = msg_send![session, isRunning];
            if !is_running {
                let null_obj: *mut AnyObject = ptr::null_mut();
                let null_q: *mut c_void = ptr::null_mut();
                let _: () =
                    msg_send![video_output, setSampleBufferDelegate: null_obj, queue: null_q];
                let _: () = msg_send![session, stopRunning];
                release_obj(input);
                drop(Box::from_raw(state_ptr as *mut VideoOutputState));
                drop(video_output_delegate);
                release_obj(video_output);
                release_obj(session);
                self.state = CaptureSessionState::Error;
                anyhow::bail!("AVFoundation camera session failed to start running");
            }

            self.capture_session = Some(session);
            self.capture_input = Some(input);
            self.video_output = Some(video_output);
            self.video_output_delegate = Some(video_output_delegate);
            self.state = CaptureSessionState::Running;
        }
        Ok(())
    }

    fn pause(&mut self) -> Result<()> {
        if let Some(session) = self.capture_session {
            unsafe {
                let _: () = msg_send![session, stopRunning];
            }
        }
        self.state = CaptureSessionState::Paused;
        Ok(())
    }

    fn resume(&mut self) -> Result<()> {
        if let Some(session) = self.capture_session {
            unsafe {
                let _: () = msg_send![session, startRunning];
            }
            self.state = CaptureSessionState::Running;
            Ok(())
        } else {
            self.state = CaptureSessionState::Error;
            Err(anyhow!("camera session has not been initialized"))
        }
    }

    fn stop(&mut self) -> Result<()> {
        if let Some(video_output) = self.video_output {
            unsafe {
                let null_obj: *mut AnyObject = ptr::null_mut();
                let null_q: *mut c_void = ptr::null_mut();
                let _: () =
                    msg_send![video_output, setSampleBufferDelegate: null_obj, queue: null_q];
            }
        }
        if let Some(session) = self.capture_session {
            unsafe {
                let _: () = msg_send![session, stopRunning];
            }
        }
        if let Some(delegate) = self.video_output_delegate.take() {
            let state_ptr = delegate.take_state();
            if !state_ptr.is_null() {
                drop(unsafe { Box::from_raw(state_ptr as *mut VideoOutputState) });
            }
            drop(delegate);
        }
        if let Some(video_output) = self.video_output.take() {
            unsafe {
                release_obj(video_output);
            }
        }
        if let Some(input) = self.capture_input.take() {
            unsafe {
                release_obj(input);
            }
        }
        if let Some(session) = self.capture_session.take() {
            unsafe {
                release_obj(session);
            }
        }

        self.state = CaptureSessionState::Stopped;
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

impl Drop for MacCameraCaptureSession {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

struct MacMicrophoneCaptureSession {
    config: CaptureConfig,
    state: CaptureSessionState,
    dropped: AtomicU64,
    latency_ms: AtomicU64,
    callback: Option<FrameCallback>,
}

impl MacMicrophoneCaptureSession {
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

impl CaptureSession for MacMicrophoneCaptureSession {
    fn start(&mut self, config: CaptureConfig, callback: FrameCallback) -> Result<()> {
        self.config = config;
        self.state = CaptureSessionState::Starting;
        self.callback = Some(callback);
        Err(anyhow!(
            "CoreAudio microphone capture requires runtime initialization"
        ))
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

struct VideoOutputState {
    callback: FrameCallback,
    dropped: Arc<AtomicU64>,
    latency_ms: Arc<AtomicU64>,
}

fn enumerate_devices(
    kind: CaptureDeviceKind,
    media_type: *mut AnyObject,
) -> Result<Vec<CaptureDeviceInfo>> {
    unsafe {
        let devices: *mut AnyObject =
            msg_send![lookup_class(c"AVCaptureDevice"), devicesWithMediaType: media_type];
        if devices.is_null() {
            return Ok(Vec::new());
        }

        let count: usize = msg_send![devices, count];
        let mut result = Vec::with_capacity(count);
        for index in 0..count {
            let device: *mut AnyObject = msg_send![devices, objectAtIndex: index];
            if device.is_null() {
                continue;
            }

            let unique_id: *mut AnyObject = msg_send![device, uniqueID];
            let localized_name: *mut AnyObject = msg_send![device, localizedName];
            let connected: bool = msg_send![device, isConnected];

            result.push(CaptureDeviceInfo {
                id: ns_string_to_str(unique_id),
                name: ns_string_to_str(localized_name),
                kind,
                is_available: connected,
            });
        }

        Ok(result)
    }
}

unsafe fn create_camera_input(device_id: &str) -> Result<*mut AnyObject> {
    unsafe {
        let devices: *mut AnyObject =
            msg_send![lookup_class(c"AVCaptureDevice"), devicesWithMediaType: AVMediaTypeVideo];
        if devices.is_null() {
            anyhow::bail!("AVFoundation did not return any video capture devices");
        }

        let count: usize = msg_send![devices, count];
        let mut selected_device: *mut AnyObject = ptr::null_mut();
        for index in 0..count {
            let candidate: *mut AnyObject = msg_send![devices, objectAtIndex: index];
            if candidate.is_null() {
                continue;
            }

            let unique_id: *mut AnyObject = msg_send![candidate, uniqueID];
            if ns_string_to_str(unique_id) == device_id {
                selected_device = candidate;
                break;
            }
        }

        if selected_device.is_null() {
            anyhow::bail!("camera device `{device_id}` is no longer available");
        }

        let mut error: *mut AnyObject = ptr::null_mut();
        let input: *mut AnyObject = msg_send![
            lookup_class(c"AVCaptureDeviceInput"),
            deviceInputWithDevice: selected_device,
            error: &mut error
        ];
        if input.is_null() {
            if !error.is_null() {
                let description: *mut AnyObject = msg_send![error, localizedDescription];
                anyhow::bail!(
                    "failed to open camera `{device_id}`: {}",
                    ns_string_to_str(description)
                );
            }
            anyhow::bail!("failed to create an AVFoundation input for `{device_id}`");
        }

        Ok(input)
    }
}

unsafe fn configure_video_output(video_output: *mut AnyObject) -> Result<()> {
    unsafe {
        let pixel_format: *mut AnyObject = msg_send![
            lookup_class(c"NSNumber"),
            numberWithUnsignedInt: kCVPixelFormatType_32BGRA
        ];
        let key_ptr = kCVPixelBufferPixelFormatTypeKey as *mut AnyObject;
        let settings: *mut AnyObject = msg_send![
            lookup_class(c"NSDictionary"),
            dictionaryWithObject: pixel_format,
            forKey: key_ptr
        ];
        let _: () = msg_send![video_output, setVideoSettings: settings];
        let _: () = msg_send![video_output, setAlwaysDiscardsLateVideoFrames: true];
    }
    Ok(())
}

fn cmtime_to_millis(value: i64, timescale: i32) -> u64 {
    if timescale <= 0 || value <= 0 {
        0
    } else {
        ((value as i128 * 1_000) / timescale as i128) as u64
    }
}
