use crate::PermissionStatus;
use crate::media_capture::{
    CaptureBackend, CaptureConfig, CaptureDeviceInfo, CaptureDeviceKind, CaptureSession,
    CaptureSessionState, DeviceEnumerator, FrameCallback,
};
use anyhow::{Result, anyhow};
use std::sync::atomic::{AtomicU64, Ordering};

pub struct LinuxMicrophoneBackend;

impl LinuxMicrophoneBackend {
    pub fn new() -> Self {
        Self
    }
}

impl DeviceEnumerator for LinuxMicrophoneBackend {
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

impl CaptureBackend for LinuxMicrophoneBackend {
    fn create_session(&self, config: &CaptureConfig) -> Result<Box<dyn CaptureSession>> {
        match config.kind {
            CaptureDeviceKind::Microphone => {
                Ok(Box::new(LinuxMicrophoneSession::new(config.clone())))
            }
            _ => Err(anyhow!(
                "LinuxMicrophoneBackend does not support {:?}",
                config.kind
            )),
        }
    }
}

struct LinuxMicrophoneSession {
    config: CaptureConfig,
    state: CaptureSessionState,
    dropped: AtomicU64,
    latency_ms: AtomicU64,
    callback: Option<FrameCallback>,
}

impl LinuxMicrophoneSession {
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

impl CaptureSession for LinuxMicrophoneSession {
    fn start(&mut self, config: CaptureConfig, callback: FrameCallback) -> Result<()> {
        self.config = config;
        self.state = CaptureSessionState::Starting;
        self.callback = Some(callback);
        Err(anyhow!(
            "PulseAudio/PipeWire microphone capture requires runtime initialization"
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

fn pipewire_permission_status() -> Option<PermissionStatus> {
    let output = std::process::Command::new("dbus-send")
        .args([
            "--session",
            "--dest=org.freedesktop.impl.portal.PermissionStore",
            "--type=method_call",
            "--print-reply",
            "/org/freedesktop/impl/portal/PermissionStore",
            "org.freedesktop.impl.portal.PermissionStore.Lookup",
            "string:devices",
            "string:microphone",
        ])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let stdout = std::str::from_utf8(&output.stdout).ok()?;

    if stdout.contains("\"no\"") {
        Some(PermissionStatus::Denied)
    } else if stdout.contains("\"yes\"") {
        Some(PermissionStatus::Granted)
    } else {
        Some(PermissionStatus::NotDetermined)
    }
}

pub fn microphone_status() -> PermissionStatus {
    if let Some(status) = pipewire_permission_status() {
        return status;
    }

    PermissionStatus::Granted
}

#[allow(dead_code)]
pub fn request_microphone_permission(callback: Box<dyn FnOnce(bool) + Send>) {
    let status = microphone_status();
    match status {
        PermissionStatus::Granted => callback(true),
        PermissionStatus::Denied | PermissionStatus::Restricted => callback(false),
        PermissionStatus::NotDetermined => callback(true),
    }
}
