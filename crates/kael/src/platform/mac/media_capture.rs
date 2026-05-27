use crate::media_capture::{
    CaptureBackend, CaptureConfig, CaptureDeviceInfo, CaptureDeviceKind, CaptureSession,
    CaptureSessionState, DeviceEnumerator, FrameCallback, PixelFormat,
};
use anyhow::{Result, anyhow};
use cocoa::{
    base::{BOOL, NO, YES, id, nil},
    foundation::NSUInteger,
};
use core_foundation::{base::TCFType, string::CFStringRef};
use core_video::image_buffer::CVImageBufferRef;
use ctor::ctor;
use media::{
    core_media::{CMSampleBuffer, CMSampleBufferRef},
    core_video::kCVPixelFormatType_32BGRA,
};
use objc::{
    class,
    declare::ClassDecl,
    msg_send,
    runtime::{Class, Object, Sel},
    sel, sel_impl,
};
use std::{
    ffi::c_void,
    ptr,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Instant,
};

use super::{
    NSStringExt,
    dispatcher::dispatch_sys::{DISPATCH_QUEUE_PRIORITY_HIGH, dispatch_get_global_queue},
};

#[link(name = "AVFoundation", kind = "framework")]
unsafe extern "C" {
    static AVMediaTypeAudio: id;
    static AVMediaTypeVideo: id;
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

static mut VIDEO_OUTPUT_DELEGATE_CLASS: *const Class = ptr::null();
const VIDEO_OUTPUT_STATE_IVAR: &str = "video_output_state";

#[ctor(unsafe)]
unsafe fn build_classes() {
    if !unsafe { VIDEO_OUTPUT_DELEGATE_CLASS.is_null() } {
        return;
    }

    let mut decl = ClassDecl::new("GPUICameraVideoOutputDelegate", class!(NSObject)).unwrap();
    decl.add_ivar::<*mut c_void>(VIDEO_OUTPUT_STATE_IVAR);
    unsafe {
        decl.add_method(
            sel!(captureOutput:didOutputSampleBuffer:fromConnection:),
            capture_output_did_output_sample_buffer as extern "C" fn(&Object, Sel, id, id, id),
        );
        decl.add_method(
            sel!(captureOutput:didDropSampleBuffer:fromConnection:),
            capture_output_did_drop_sample_buffer as extern "C" fn(&Object, Sel, id, id, id),
        );
        VIDEO_OUTPUT_DELEGATE_CLASS = decl.register();
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
    capture_session: Option<id>,
    capture_input: Option<id>,
    video_output: Option<id>,
    video_output_delegate: Option<id>,
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

        let session: id = unsafe { msg_send![class!(AVCaptureSession), new] };
        let video_output: id = unsafe { msg_send![class!(AVCaptureVideoDataOutput), new] };
        let video_output_delegate: id = unsafe { msg_send![VIDEO_OUTPUT_DELEGATE_CLASS, new] };

        let delegate_state = Box::new(VideoOutputState {
            callback,
            dropped: Arc::clone(&self.dropped),
            latency_ms: Arc::clone(&self.latency_ms),
        });
        unsafe {
            (*video_output_delegate).set_ivar(
                VIDEO_OUTPUT_STATE_IVAR,
                Box::into_raw(delegate_state) as *mut c_void,
            );
        }

        let input = match unsafe { create_camera_input(&self.config.device_id) } {
            Ok(input) => input,
            Err(error) => {
                unsafe {
                    release_delegate_state(video_output_delegate);
                    release_obj(video_output_delegate);
                    release_obj(video_output);
                    release_obj(session);
                }
                self.state = CaptureSessionState::Error;
                return Err(error);
            }
        };

        if let Err(error) = unsafe { configure_video_output(video_output) } {
            unsafe {
                release_obj(input);
                release_delegate_state(video_output_delegate);
                release_obj(video_output_delegate);
                release_obj(video_output);
                release_obj(session);
            }
            self.state = CaptureSessionState::Error;
            return Err(error);
        }

        let can_add_input: BOOL = unsafe { msg_send![session, canAddInput: input] };
        let can_add_output: BOOL = unsafe { msg_send![session, canAddOutput: video_output] };
        if can_add_input != YES || can_add_output != YES {
            unsafe {
                release_obj(input);
                release_delegate_state(video_output_delegate);
                release_obj(video_output_delegate);
                release_obj(video_output);
                release_obj(session);
            }
            self.state = CaptureSessionState::Error;
            anyhow::bail!("AVFoundation camera session could not add the selected input/output");
        }

        unsafe {
            let _: () = msg_send![session, addInput: input];
            let _: () = msg_send![session, addOutput: video_output];
            let queue =
                dispatch_get_global_queue(DISPATCH_QUEUE_PRIORITY_HIGH.try_into().unwrap(), 0);
            let _: () = msg_send![video_output, setSampleBufferDelegate: video_output_delegate queue: queue];
            let _: () = msg_send![session, startRunning];
        }

        let is_running: BOOL = unsafe { msg_send![session, isRunning] };
        if is_running != YES {
            unsafe {
                let _: () = msg_send![video_output, setSampleBufferDelegate:nil queue:nil];
                let _: () = msg_send![session, stopRunning];
                release_obj(input);
                release_delegate_state(video_output_delegate);
                release_obj(video_output_delegate);
                release_obj(video_output);
                release_obj(session);
            }
            self.state = CaptureSessionState::Error;
            anyhow::bail!("AVFoundation camera session failed to start running");
        }

        self.capture_session = Some(session);
        self.capture_input = Some(input);
        self.video_output = Some(video_output);
        self.video_output_delegate = Some(video_output_delegate);
        self.state = CaptureSessionState::Running;
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
                let _: () = msg_send![video_output, setSampleBufferDelegate:nil queue:nil];
            }
        }
        if let Some(session) = self.capture_session {
            unsafe {
                let _: () = msg_send![session, stopRunning];
            }
        }
        if let Some(delegate) = self.video_output_delegate.take() {
            unsafe {
                release_delegate_state(delegate);
                release_obj(delegate);
            }
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

fn enumerate_devices(kind: CaptureDeviceKind, media_type: id) -> Result<Vec<CaptureDeviceInfo>> {
    unsafe {
        let devices: id = msg_send![class!(AVCaptureDevice), devicesWithMediaType: media_type];
        if devices == nil {
            return Ok(Vec::new());
        }

        let count: NSUInteger = msg_send![devices, count];
        let mut result = Vec::with_capacity(count as usize);
        for index in 0..count {
            let device: id = msg_send![devices, objectAtIndex: index];
            if device == nil {
                continue;
            }

            let unique_id: id = msg_send![device, uniqueID];
            let localized_name: id = msg_send![device, localizedName];
            let connected: BOOL = msg_send![device, isConnected];

            result.push(CaptureDeviceInfo {
                id: NSStringExt::to_str(&unique_id).to_string(),
                name: NSStringExt::to_str(&localized_name).to_string(),
                kind,
                is_available: connected != NO,
            });
        }

        Ok(result)
    }
}

unsafe fn create_camera_input(device_id: &str) -> Result<id> {
    let devices: id =
        unsafe { msg_send![class!(AVCaptureDevice), devicesWithMediaType: AVMediaTypeVideo] };
    if devices == nil {
        anyhow::bail!("AVFoundation did not return any video capture devices");
    }

    let count: NSUInteger = unsafe { msg_send![devices, count] };
    let mut selected_device = nil;
    for index in 0..count {
        let candidate: id = unsafe { msg_send![devices, objectAtIndex: index] };
        if candidate == nil {
            continue;
        }

        let unique_id: id = unsafe { msg_send![candidate, uniqueID] };
        if unsafe { NSStringExt::to_str(&unique_id) } == device_id {
            selected_device = candidate;
            break;
        }
    }

    if selected_device == nil {
        anyhow::bail!("camera device `{device_id}` is no longer available");
    }

    let mut error: id = nil;
    let input: id = unsafe {
        msg_send![class!(AVCaptureDeviceInput), deviceInputWithDevice:selected_device error:&mut error]
    };
    if input == nil {
        if error != nil {
            let description: id = unsafe { msg_send![error, localizedDescription] };
            anyhow::bail!("failed to open camera `{device_id}`: {}", unsafe {
                NSStringExt::to_str(&description)
            });
        }
        anyhow::bail!("failed to create an AVFoundation input for `{device_id}`");
    }

    Ok(input)
}

unsafe fn configure_video_output(video_output: id) -> Result<()> {
    let pixel_format: id =
        unsafe { msg_send![class!(NSNumber), numberWithUnsignedInt:kCVPixelFormatType_32BGRA] };
    let settings: id = unsafe {
        msg_send![
            class!(NSDictionary),
            dictionaryWithObject: pixel_format
            forKey: kCVPixelBufferPixelFormatTypeKey as id
        ]
    };
    let _: () = unsafe { msg_send![video_output, setVideoSettings: settings] };
    let _: () = unsafe { msg_send![video_output, setAlwaysDiscardsLateVideoFrames: YES] };
    Ok(())
}

unsafe fn release_delegate_state(delegate: id) {
    if delegate == nil {
        return;
    }

    let delegate = unsafe { delegate.as_ref() }.unwrap();
    let state_ptr = unsafe { *delegate.get_ivar::<*mut c_void>(VIDEO_OUTPUT_STATE_IVAR) };
    if !state_ptr.is_null() {
        drop(unsafe { Box::from_raw(state_ptr as *mut VideoOutputState) });
        unsafe {
            (*(delegate as *const Object as *mut Object))
                .set_ivar(VIDEO_OUTPUT_STATE_IVAR, ptr::null_mut::<c_void>());
        }
    }
}

unsafe fn release_obj(object: id) {
    if object != nil {
        let _: () = unsafe { msg_send![object, release] };
    }
}

extern "C" fn capture_output_did_output_sample_buffer(
    this: &Object,
    _: Sel,
    _output: id,
    sample_buffer: id,
    _connection: id,
) {
    let state_ptr = unsafe { *this.get_ivar::<*mut c_void>(VIDEO_OUTPUT_STATE_IVAR) };
    if state_ptr.is_null() || sample_buffer == nil {
        return;
    }

    let start = Instant::now();
    let state = unsafe { &*(state_ptr as *const VideoOutputState) };
    unsafe {
        let sample_buffer = CMSampleBuffer::wrap_under_get_rule(sample_buffer as CMSampleBufferRef);
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
            let target =
                &mut data[row * expected_bytes_per_row..(row + 1) * expected_bytes_per_row];
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

extern "C" fn capture_output_did_drop_sample_buffer(
    this: &Object,
    _: Sel,
    _output: id,
    _sample_buffer: id,
    _connection: id,
) {
    let state_ptr = unsafe { *this.get_ivar::<*mut c_void>(VIDEO_OUTPUT_STATE_IVAR) };
    if state_ptr.is_null() {
        return;
    }

    let state = unsafe { &*(state_ptr as *const VideoOutputState) };
    state.dropped.fetch_add(1, Ordering::Relaxed);
}

fn cmtime_to_millis(value: i64, timescale: i32) -> u64 {
    if timescale <= 0 || value <= 0 {
        0
    } else {
        ((value as i128 * 1_000) / timescale as i128) as u64
    }
}
