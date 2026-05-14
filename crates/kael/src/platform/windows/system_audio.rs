use crate::media_capture::{
    CaptureBackend, CaptureConfig, CaptureDeviceInfo, CaptureDeviceKind, CaptureSession,
    CaptureSessionState, DeviceEnumerator, FrameCallback,
};
use anyhow::{Result, anyhow};
use std::sync::atomic::{AtomicU64, Ordering};

pub struct WindowsSystemAudioBackend;

impl WindowsSystemAudioBackend {
    pub fn new() -> Self {
        Self
    }
}

impl DeviceEnumerator for WindowsSystemAudioBackend {
    fn devices(&self, kind: CaptureDeviceKind) -> Result<Vec<CaptureDeviceInfo>> {
        match kind {
            CaptureDeviceKind::SystemAudio => Ok(vec![CaptureDeviceInfo {
                id: "system-audio-0".to_string(),
                name: "System Audio Loopback".to_string(),
                kind: CaptureDeviceKind::SystemAudio,
                is_available: true,
            }]),
            _ => Ok(vec![]),
        }
    }
}

impl CaptureBackend for WindowsSystemAudioBackend {
    fn create_session(&self, config: &CaptureConfig) -> Result<Box<dyn CaptureSession>> {
        match config.kind {
            CaptureDeviceKind::SystemAudio => {
                Ok(Box::new(WindowsSystemAudioSession::new(config.clone())))
            }
            _ => Err(anyhow!(
                "WindowsSystemAudioBackend does not support {:?}",
                config.kind
            )),
        }
    }
}

struct WindowsSystemAudioSession {
    config: CaptureConfig,
    state: CaptureSessionState,
    dropped: AtomicU64,
    latency_ms: AtomicU64,
    callback: Option<FrameCallback>,
}

impl WindowsSystemAudioSession {
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

impl CaptureSession for WindowsSystemAudioSession {
    fn start(&mut self, config: CaptureConfig, callback: FrameCallback) -> Result<()> {
        self.config = config;
        self.state = CaptureSessionState::Starting;
        self.callback = Some(callback);
        Err(anyhow!(
            "WASAPI loopback capture requires runtime initialization"
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
