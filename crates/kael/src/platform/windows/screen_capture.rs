use crate::media_capture::{
    CaptureBackend, CaptureConfig, CaptureDeviceInfo, CaptureDeviceKind, CaptureSession,
    CaptureSessionState, DeviceEnumerator, FrameCallback,
};
use anyhow::{Result, anyhow};
use std::sync::atomic::{AtomicU64, Ordering};

pub struct WindowsScreenCaptureBackend;

impl WindowsScreenCaptureBackend {
    pub fn new() -> Self {
        Self
    }
}

impl DeviceEnumerator for WindowsScreenCaptureBackend {
    fn devices(&self, kind: CaptureDeviceKind) -> Result<Vec<CaptureDeviceInfo>> {
        match kind {
            CaptureDeviceKind::Screen => Ok(vec![CaptureDeviceInfo {
                id: "display-0".to_string(),
                name: "Primary Display".to_string(),
                kind: CaptureDeviceKind::Screen,
                is_available: true,
            }]),
            CaptureDeviceKind::Window => Ok(vec![]),
            _ => Ok(vec![]),
        }
    }
}

impl CaptureBackend for WindowsScreenCaptureBackend {
    fn create_session(&self, config: &CaptureConfig) -> Result<Box<dyn CaptureSession>> {
        match config.kind {
            CaptureDeviceKind::Screen | CaptureDeviceKind::Window => {
                Ok(Box::new(WindowsScreenCaptureSession::new(config.clone())))
            }
            _ => Err(anyhow!(
                "WindowsScreenCaptureBackend does not support {:?}",
                config.kind
            )),
        }
    }
}

struct WindowsScreenCaptureSession {
    config: CaptureConfig,
    state: CaptureSessionState,
    dropped: AtomicU64,
    latency_ms: AtomicU64,
    callback: Option<FrameCallback>,
}

impl WindowsScreenCaptureSession {
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

impl CaptureSession for WindowsScreenCaptureSession {
    fn start(&mut self, config: CaptureConfig, callback: FrameCallback) -> Result<()> {
        self.config = config;
        self.state = CaptureSessionState::Starting;
        self.callback = Some(callback);
        Err(anyhow!(
            "Windows Graphics Capture API requires runtime initialization"
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
