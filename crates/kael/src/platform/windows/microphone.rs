use crate::PermissionStatus;
use crate::media_capture::{
    CaptureBackend, CaptureConfig, CaptureDeviceInfo, CaptureDeviceKind, CaptureSession,
    CaptureSessionState, DeviceEnumerator, FrameCallback,
};
use anyhow::{Result, anyhow};
use std::sync::atomic::{AtomicU64, Ordering};
use windows::Media::Capture::MediaCapture;

pub struct WindowsMicrophoneBackend;

impl WindowsMicrophoneBackend {
    pub fn new() -> Self {
        Self
    }
}

impl DeviceEnumerator for WindowsMicrophoneBackend {
    fn devices(&self, kind: CaptureDeviceKind) -> Result<Vec<CaptureDeviceInfo>> {
        match kind {
            CaptureDeviceKind::Microphone => Ok(vec![CaptureDeviceInfo {
                id: "mic-0".to_string(),
                name: "Default Microphone".to_string(),
                kind: CaptureDeviceKind::Microphone,
                is_available: true,
            }]),
            _ => Ok(vec![]),
        }
    }
}

impl CaptureBackend for WindowsMicrophoneBackend {
    fn create_session(&self, config: &CaptureConfig) -> Result<Box<dyn CaptureSession>> {
        match config.kind {
            CaptureDeviceKind::Microphone => {
                Ok(Box::new(WindowsMicrophoneSession::new(config.clone())))
            }
            _ => Err(anyhow!(
                "WindowsMicrophoneBackend does not support {:?}",
                config.kind
            )),
        }
    }
}

struct WindowsMicrophoneSession {
    config: CaptureConfig,
    state: CaptureSessionState,
    dropped: AtomicU64,
    latency_ms: AtomicU64,
    callback: Option<FrameCallback>,
}

impl WindowsMicrophoneSession {
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

impl CaptureSession for WindowsMicrophoneSession {
    fn start(&mut self, config: CaptureConfig, callback: FrameCallback) -> Result<()> {
        self.config = config;
        self.state = CaptureSessionState::Starting;
        self.callback = Some(callback);
        Err(anyhow!(
            "Windows microphone capture requires MediaCapture runtime initialization"
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

pub fn microphone_status() -> PermissionStatus {
    let capture = match MediaCapture::new() {
        Ok(capture) => capture,
        Err(_) => {
            return PermissionStatus::Granted;
        }
    };

    match capture
        .InitializeAsync()
        .and_then(super::util::block_on_action)
    {
        Ok(()) => PermissionStatus::Granted,
        Err(err) => {
            if err.code().0 as u32 == 0x80070005 {
                PermissionStatus::Denied
            } else {
                PermissionStatus::Granted
            }
        }
    }
}
