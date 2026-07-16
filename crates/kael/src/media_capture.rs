//! Cross-platform media and capture infrastructure for GPUI.
//!
//! This module defines capture abstractions for screen, window, microphone,
//! camera, and system audio. Platform backends implement the actual capture
//! using native APIs (ScreenCaptureKit, Windows Graphics Capture, PipeWire,
//! etc.).

use std::{
    collections::HashMap,
    fmt::Debug,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Instant,
};

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::process_model::ProcessId;
use crate::security::{Capability, PermissionBroker, PermissionResult};

use crate::tracer::{TracePhase, Tracer};

// ---------------------------------------------------------------------------
// Device Enumeration
// ---------------------------------------------------------------------------

/// The kind of capture device.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CaptureDeviceKind {
    /// A physical or virtual display.
    Screen,
    /// A specific application window.
    Window,
    /// A microphone input.
    Microphone,
    /// A camera / video input.
    Camera,
    /// System audio output (loopback).
    SystemAudio,
}

impl CaptureDeviceKind {
    /// Stable lowercase key for generated logs and policies.
    pub fn key(&self) -> &'static str {
        match self {
            Self::Screen => "screen",
            Self::Window => "window",
            Self::Microphone => "microphone",
            Self::Camera => "camera",
            Self::SystemAudio => "system-audio",
        }
    }
}

/// Information about an available capture device.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CaptureDeviceInfo {
    /// A stable opaque identifier for this device.
    pub id: String,
    /// Human-readable display name.
    pub name: String,
    /// The kind of device.
    pub kind: CaptureDeviceKind,
    /// Whether the device is currently available.
    pub is_available: bool,
}

impl CaptureDeviceInfo {
    /// Content-safe summary that avoids logging device ids or display names.
    pub fn to_text(&self) -> String {
        format!(
            "capture device {}: available {}",
            self.kind.key(),
            self.is_available
        )
    }
}

/// Checked catalog of capture sources for picker UI and generated agents.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CaptureSourceCatalog {
    devices: Vec<CaptureDeviceInfo>,
}

impl CaptureSourceCatalog {
    /// Matching devices in stable query order.
    pub fn devices(&self) -> &[CaptureDeviceInfo] {
        &self.devices
    }

    /// First matching device, if any.
    pub fn first(&self) -> Option<&CaptureDeviceInfo> {
        self.devices.first()
    }

    /// Whether no devices matched the query.
    pub fn is_empty(&self) -> bool {
        self.devices.is_empty()
    }

    /// Number of devices that matched the query.
    pub fn len(&self) -> usize {
        self.devices.len()
    }

    /// Number of catalog devices with a particular kind.
    pub fn kind_count(&self, kind: CaptureDeviceKind) -> usize {
        self.devices
            .iter()
            .filter(|device| device.kind == kind)
            .count()
    }

    /// Number of currently available devices.
    pub fn available_count(&self) -> usize {
        self.devices
            .iter()
            .filter(|device| device.is_available)
            .count()
    }

    /// Number of currently unavailable devices.
    pub fn unavailable_count(&self) -> usize {
        self.devices
            .iter()
            .filter(|device| !device.is_available)
            .count()
    }

    /// Content-safe summary for source-picker previews and agent logs.
    pub fn to_text(&self) -> String {
        format!(
            "capture source catalog: {} devices, {} available, {} unavailable",
            self.len(),
            self.available_count(),
            self.unavailable_count()
        )
    }

    /// Build a capture config for the first matching device.
    pub fn first_config(&self, kind: CaptureDeviceKind) -> Result<CaptureConfig> {
        let Some(device) = self.first() else {
            anyhow::bail!("capture source catalog is empty");
        };
        anyhow::ensure!(
            device.kind == kind,
            "capture source kind mismatch: catalog device is {:?}, requested {:?}",
            device.kind,
            kind
        );
        Ok(CaptureConfig::new(device.id.clone(), kind))
    }
}

/// Builder for native media capture source catalog queries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptureSourceQueryBuilder {
    kinds: Vec<CaptureDeviceKind>,
    name_contains: Option<String>,
    require_available: bool,
    include_unavailable: bool,
    limit: Option<usize>,
}

impl CaptureSourceQueryBuilder {
    /// Create a query for one capture source kind.
    pub fn new(kind: CaptureDeviceKind) -> Self {
        Self {
            kinds: vec![kind],
            name_contains: None,
            require_available: true,
            include_unavailable: false,
            limit: None,
        }
    }

    /// Query capturable screens/displays.
    pub fn screens() -> Self {
        Self::new(CaptureDeviceKind::Screen)
    }

    /// Query capturable application windows.
    pub fn windows() -> Self {
        Self::new(CaptureDeviceKind::Window)
    }

    /// Query screens and windows for source picker UI.
    pub fn screens_and_windows() -> Self {
        Self {
            kinds: vec![CaptureDeviceKind::Screen, CaptureDeviceKind::Window],
            name_contains: None,
            require_available: true,
            include_unavailable: false,
            limit: None,
        }
    }

    /// Query cameras.
    pub fn cameras() -> Self {
        Self::new(CaptureDeviceKind::Camera)
    }

    /// Query microphones.
    pub fn microphones() -> Self {
        Self::new(CaptureDeviceKind::Microphone)
    }

    /// Query system audio loopback sources.
    pub fn system_audio() -> Self {
        Self::new(CaptureDeviceKind::SystemAudio)
    }

    /// Add another kind to this query.
    pub fn kind(mut self, kind: CaptureDeviceKind) -> Self {
        if !self.kinds.contains(&kind) {
            self.kinds.push(kind);
        }
        self
    }

    /// Filter devices whose display names contain text, case-insensitively.
    pub fn name_contains(mut self, name: impl Into<String>) -> Self {
        self.name_contains = Some(name.into());
        self
    }

    /// Include unavailable devices in the returned catalog.
    pub fn include_unavailable(mut self) -> Self {
        self.require_available = false;
        self.include_unavailable = true;
        self
    }

    /// Limit the number of returned devices.
    pub fn limit(mut self, limit: usize) -> Self {
        self.limit = Some(limit);
        self
    }

    /// Return requested kinds.
    pub fn kinds(&self) -> &[CaptureDeviceKind] {
        &self.kinds
    }

    /// Whether this query filters device display names.
    pub fn has_name_filter(&self) -> bool {
        self.name_contains.is_some()
    }

    /// Whether this query limits the number of returned devices.
    pub fn has_limit(&self) -> bool {
        self.limit.is_some()
    }

    /// Content-safe summary that avoids logging source names or filters.
    pub fn to_text(&self) -> String {
        format!(
            "capture source query: {} kinds, name-filter {}, require-available {}, include-unavailable {}, limit {}",
            self.kinds.len(),
            self.has_name_filter(),
            self.require_available,
            self.include_unavailable,
            self.has_limit()
        )
    }

    /// Validate this query without enumerating devices.
    pub fn validate(&self) -> Result<()> {
        anyhow::ensure!(
            !self.kinds.is_empty(),
            "capture source query must include at least one kind"
        );
        for (index, kind) in self.kinds.iter().enumerate() {
            anyhow::ensure!(
                !self.kinds[..index].contains(kind),
                "capture source kind declared more than once: {:?}",
                kind
            );
        }
        if let Some(name) = &self.name_contains {
            anyhow::ensure!(
                !name.trim().is_empty(),
                "capture source name filter cannot be empty"
            );
            anyhow::ensure!(
                name.trim() == name,
                "capture source name filter cannot have leading or trailing whitespace"
            );
        }
        if let Some(limit) = self.limit {
            anyhow::ensure!(
                limit > 0,
                "capture source query limit must be greater than zero"
            );
        }
        Ok(())
    }

    /// Resolve this query through a capture manager.
    pub fn resolve(self, manager: &CaptureManager) -> Result<CaptureSourceCatalog> {
        self.validate()?;
        let name_filter = self.name_contains.as_ref().map(|name| name.to_lowercase());
        let mut devices = Vec::new();
        for kind in &self.kinds {
            for device in manager.devices(*kind)? {
                if self.require_available && !device.is_available {
                    continue;
                }
                if !self.include_unavailable && !device.is_available {
                    continue;
                }
                if name_filter
                    .as_ref()
                    .is_some_and(|name| !device.name.to_lowercase().contains(name))
                {
                    continue;
                }
                devices.push(device);
                if self.limit.is_some_and(|limit| devices.len() >= limit) {
                    return Ok(CaptureSourceCatalog { devices });
                }
            }
        }
        Ok(CaptureSourceCatalog { devices })
    }
}

/// Enumerate available capture devices of the given kind.
///
/// Platform backends provide the actual implementation.
pub trait DeviceEnumerator: Send + Sync {
    /// List devices of the requested kind.
    fn devices(&self, kind: CaptureDeviceKind) -> Result<Vec<CaptureDeviceInfo>>;
}

/// A backend that can enumerate devices and create capture sessions.
pub trait CaptureBackend: DeviceEnumerator + Send + Sync {
    /// Create a capture session for the given configuration.
    fn create_session(&self, config: &CaptureConfig) -> Result<Box<dyn CaptureSession>>;
}

/// Optional capture-management layer for applications that want a default
/// registry-based way to wire platform backends together.
///
/// Applications can skip this type entirely and instantiate their own
/// [`CaptureSession`] implementations directly.
#[derive(Default)]
pub struct CaptureManager {
    backends: HashMap<CaptureDeviceKind, Arc<dyn CaptureBackend>>,
    permission_broker: Option<PermissionBroker>,
    process_id: Option<ProcessId>,
}

impl CaptureManager {
    /// Create an empty capture manager.
    pub fn new() -> Self {
        Self {
            backends: HashMap::new(),
            permission_broker: None,
            process_id: None,
        }
    }

    /// Create a capture manager with platform-default backends registered.
    pub fn with_default_backends() -> Self {
        default_capture_manager()
    }

    /// Set the permission broker for capability checks.
    pub fn set_permission_broker(&mut self, broker: PermissionBroker) {
        self.permission_broker = Some(broker);
    }

    /// Get a reference to the permission broker, if set.
    pub fn permission_broker(&self) -> Option<&PermissionBroker> {
        self.permission_broker.as_ref()
    }

    /// Set the process identifier used for capability checks.
    pub fn set_process_id(&mut self, process_id: ProcessId) {
        self.process_id = Some(process_id);
    }

    /// Get the process identifier used for capability checks.
    pub fn process_id(&self) -> Option<ProcessId> {
        self.process_id
    }

    /// Register a backend for a capture kind.
    pub fn register_backend(
        &mut self,
        kind: CaptureDeviceKind,
        backend: Arc<dyn CaptureBackend>,
    ) -> Option<Arc<dyn CaptureBackend>> {
        self.backends.insert(kind, backend)
    }

    /// Return the backend registered for a capture kind, if any.
    pub fn backend(&self, kind: CaptureDeviceKind) -> Option<Arc<dyn CaptureBackend>> {
        self.backends.get(&kind).cloned()
    }

    /// Enumerate devices for a capture kind through the registered backend.
    pub fn devices(&self, kind: CaptureDeviceKind) -> Result<Vec<CaptureDeviceInfo>> {
        let backend = self
            .backend(kind)
            .ok_or_else(|| anyhow::anyhow!("no capture backend registered for {:?}", kind))?;
        backend.devices(kind)
    }

    /// Enumerate a checked source catalog for picker UI or generated agents.
    pub fn sources(&self, query: CaptureSourceQueryBuilder) -> Result<CaptureSourceCatalog> {
        query.resolve(self)
    }

    /// Create a capture session from a registered backend.
    pub fn create_session(&self, config: &CaptureConfig) -> Result<Box<dyn CaptureSession>> {
        let capability = match config.kind {
            CaptureDeviceKind::Microphone => Capability::Microphone,
            CaptureDeviceKind::Camera => Capability::Camera,
            CaptureDeviceKind::Screen
            | CaptureDeviceKind::Window
            | CaptureDeviceKind::SystemAudio => Capability::ScreenCapture,
        };

        if let Some(broker) = &self.permission_broker {
            let process = self.process_id.unwrap_or(ProcessId(0));
            match broker.check(process, &capability) {
                PermissionResult::Granted => {}
                PermissionResult::Denied => {
                    anyhow::bail!("capability denied: {:?}", capability);
                }
                PermissionResult::Prompt => {
                    anyhow::bail!("capability prompt required: {:?}", capability);
                }
            }
        }

        let backend = self.backend(config.kind).ok_or_else(|| {
            anyhow::anyhow!("no capture backend registered for {:?}", config.kind)
        })?;
        backend.create_session(config)
    }

    /// Resolve a builder-shaped capture request into a concrete configuration.
    ///
    /// This is the ergonomic path for native desktop "capture the first
    /// available screen/camera/microphone" flows where app code should not need
    /// to enumerate devices and copy IDs by hand.
    pub fn config(&self, builder: impl Into<CaptureConfigBuilder>) -> Result<CaptureConfig> {
        builder.into().resolve(self)
    }

    /// Resolve a builder-shaped request and create its session.
    ///
    /// The returned [`CaptureConfig`] is the exact configuration that should be
    /// passed to [`CaptureSession::start`].
    pub fn create_session_with(
        &self,
        builder: impl Into<CaptureConfigBuilder>,
    ) -> Result<(CaptureConfig, Box<dyn CaptureSession>)> {
        let config = self.config(builder)?;
        let session = self.create_session(&config)?;
        Ok((config, session))
    }

    /// Resolve a grouped capture request into concrete configurations.
    ///
    /// Use this for common app flows such as screen + microphone, camera +
    /// microphone, or screen + system audio before constructing a
    /// [`CapturePipeline`].
    pub fn configs(
        &self,
        builder: impl Into<CaptureConfigSetBuilder>,
    ) -> Result<Vec<CaptureConfig>> {
        builder.into().resolve(self)
    }

    /// Resolve a grouped capture request into a managed pipeline.
    ///
    /// This is the checked path for native media capture flows that
    /// need screen + microphone, camera + microphone, or screen + system-audio
    /// sessions without hand-writing the session creation loop.
    pub fn pipeline_checked(
        &self,
        builder: impl Into<CaptureConfigSetBuilder>,
        callback: FrameCallback,
    ) -> Result<CapturePipeline> {
        let configs = self.configs(builder)?;
        let mut pipeline = CapturePipeline::new();
        for config in configs {
            let session = self.create_session(&config)?;
            pipeline.add_session(session, config, Arc::clone(&callback));
        }
        Ok(pipeline)
    }

    /// Resolve, create, and start a grouped capture pipeline.
    ///
    /// The returned pipeline owns the started sessions and should be kept alive
    /// until capture is stopped.
    pub fn start_pipeline_checked(
        &self,
        builder: impl Into<CaptureConfigSetBuilder>,
        callback: FrameCallback,
    ) -> Result<CapturePipeline> {
        let mut pipeline = self.pipeline_checked(builder, callback)?;
        pipeline.start_all()?;
        Ok(pipeline)
    }
}

// ---------------------------------------------------------------------------
// Capture Session
// ---------------------------------------------------------------------------

/// The state of a capture session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CaptureSessionState {
    /// The session is idle and not capturing.
    Idle,
    /// The session is starting.
    Starting,
    /// The session is actively capturing.
    Running,
    /// The session is paused.
    Paused,
    /// The session has stopped.
    Stopped,
    /// The session encountered an error.
    Error,
}

/// Configuration for a capture session.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CaptureConfig {
    /// The target device identifier.
    pub device_id: String,
    /// The kind of capture.
    pub kind: CaptureDeviceKind,
    /// Requested frame rate (frames per second), if applicable.
    pub frame_rate: Option<f64>,
    /// Requested resolution, if applicable.
    pub resolution: Option<(u32, u32)>,
    /// Whether to capture audio alongside video.
    pub include_audio: bool,
}

impl CaptureConfig {
    /// Create a capture configuration for a device and capture kind.
    pub fn new(device_id: impl Into<String>, kind: CaptureDeviceKind) -> Self {
        Self {
            device_id: device_id.into(),
            kind,
            frame_rate: None,
            resolution: None,
            include_audio: false,
        }
    }

    /// Set the preferred frame rate.
    pub fn frame_rate(mut self, frame_rate: f64) -> Self {
        self.frame_rate = Some(frame_rate);
        self
    }

    /// Set the preferred resolution.
    pub fn resolution(mut self, width: u32, height: u32) -> Self {
        self.resolution = Some((width, height));
        self
    }

    /// Enable or disable audio capture.
    pub fn include_audio(mut self, include_audio: bool) -> Self {
        self.include_audio = include_audio;
        self
    }

    /// Whether this config captures a video-like source.
    pub fn is_video_source(&self) -> bool {
        matches!(
            self.kind,
            CaptureDeviceKind::Screen | CaptureDeviceKind::Window | CaptureDeviceKind::Camera
        )
    }

    /// Whether this config captures an audio-like source.
    pub fn is_audio_source(&self) -> bool {
        matches!(
            self.kind,
            CaptureDeviceKind::Microphone | CaptureDeviceKind::SystemAudio
        ) || self.include_audio
    }

    /// Content-safe summary that avoids logging platform device ids.
    pub fn to_text(&self) -> String {
        format!(
            "capture config {}: frame-rate {}, resolution {}, audio {}",
            self.kind.key(),
            self.frame_rate.is_some(),
            self.resolution.is_some(),
            self.include_audio
        )
    }
}

/// Builder for capture configurations that can resolve devices through a manager.
///
/// Use this when an app wants the common native desktop capture flow: pick a
/// screen, window, camera, microphone, or system-audio source by intent, then
/// let [`CaptureManager`] resolve the concrete device ID and enforce platform
/// permissions.
#[derive(Debug, Clone, PartialEq)]
pub struct CaptureConfigBuilder {
    kind: CaptureDeviceKind,
    device_id: Option<String>,
    device_name_contains: Option<String>,
    require_available: bool,
    frame_rate: Option<f64>,
    resolution: Option<(u32, u32)>,
    include_audio: bool,
}

impl CaptureConfigBuilder {
    /// Create a builder for the requested capture kind.
    pub fn new(kind: CaptureDeviceKind) -> Self {
        Self {
            kind,
            device_id: None,
            device_name_contains: None,
            require_available: true,
            frame_rate: None,
            resolution: None,
            include_audio: false,
        }
    }

    /// Capture a physical or virtual display.
    pub fn screen() -> Self {
        Self::new(CaptureDeviceKind::Screen)
    }

    /// Capture a specific application window.
    pub fn window() -> Self {
        Self::new(CaptureDeviceKind::Window)
    }

    /// Capture a microphone input.
    pub fn microphone() -> Self {
        Self::new(CaptureDeviceKind::Microphone)
    }

    /// Capture a camera input.
    pub fn camera() -> Self {
        Self::new(CaptureDeviceKind::Camera)
    }

    /// Capture system audio output when the platform supports it.
    pub fn system_audio() -> Self {
        Self::new(CaptureDeviceKind::SystemAudio)
    }

    /// Use an explicit platform device identifier.
    pub fn device_id(mut self, device_id: impl Into<String>) -> Self {
        self.device_id = Some(device_id.into());
        self
    }

    /// Prefer a device whose display name contains the given text.
    pub fn device_name_contains(mut self, name: impl Into<String>) -> Self {
        self.device_name_contains = Some(name.into());
        self
    }

    /// Allow unavailable devices to be selected.
    ///
    /// This is useful for diagnostics or platforms that expose stable device
    /// IDs before the source is active. The default is to require availability.
    pub fn allow_unavailable(mut self) -> Self {
        self.require_available = false;
        self
    }

    /// Require devices to report as available.
    pub fn require_available(mut self, require_available: bool) -> Self {
        self.require_available = require_available;
        self
    }

    /// Set the preferred frame rate.
    pub fn frame_rate(mut self, frame_rate: f64) -> Self {
        self.frame_rate = Some(frame_rate);
        self
    }

    /// Set the preferred resolution.
    pub fn resolution(mut self, width: u32, height: u32) -> Self {
        self.resolution = Some((width, height));
        self
    }

    /// Enable audio capture alongside the selected source.
    pub fn include_audio(mut self, include_audio: bool) -> Self {
        self.include_audio = include_audio;
        self
    }

    /// Convenience alias for enabling audio capture.
    pub fn with_audio(self) -> Self {
        self.include_audio(true)
    }

    /// Requested capture kind.
    pub fn kind(&self) -> CaptureDeviceKind {
        self.kind
    }

    /// Whether an explicit platform device id was provided.
    pub fn has_device_id(&self) -> bool {
        self.device_id.is_some()
    }

    /// Whether a device-name filter was provided.
    pub fn has_device_name_filter(&self) -> bool {
        self.device_name_contains.is_some()
    }

    /// Whether a frame-rate preference was provided.
    pub fn has_frame_rate(&self) -> bool {
        self.frame_rate.is_some()
    }

    /// Whether a resolution preference was provided.
    pub fn has_resolution(&self) -> bool {
        self.resolution.is_some()
    }

    /// Whether this builder requests a video-like source.
    pub fn is_video_source(&self) -> bool {
        matches!(
            self.kind,
            CaptureDeviceKind::Screen | CaptureDeviceKind::Window | CaptureDeviceKind::Camera
        )
    }

    /// Whether this builder requests or includes audio.
    pub fn is_audio_source(&self) -> bool {
        matches!(
            self.kind,
            CaptureDeviceKind::Microphone | CaptureDeviceKind::SystemAudio
        ) || self.include_audio
    }

    /// Content-safe summary for generated capture plans.
    pub fn to_text(&self) -> String {
        format!(
            "capture config builder {}: device-id {}, name-filter {}, require-available {}, frame-rate {}, resolution {}, audio {}",
            self.kind.key(),
            self.has_device_id(),
            self.has_device_name_filter(),
            self.require_available,
            self.has_frame_rate(),
            self.has_resolution(),
            self.include_audio
        )
    }

    /// Validate this builder without resolving a device.
    pub fn validate(&self) -> Result<()> {
        if let Some(device_id) = &self.device_id
            && device_id.trim().is_empty()
        {
            anyhow::bail!("capture device id cannot be empty");
        }

        if let Some(name) = &self.device_name_contains
            && name.trim().is_empty()
        {
            anyhow::bail!("capture device name filter cannot be empty");
        }

        if let Some(frame_rate) = self.frame_rate
            && (!frame_rate.is_finite() || frame_rate <= 0.0)
        {
            anyhow::bail!("capture frame rate must be a positive finite number");
        }

        if let Some((width, height)) = self.resolution
            && (width == 0 || height == 0)
        {
            anyhow::bail!("capture resolution dimensions must be greater than zero");
        }

        Ok(())
    }

    /// Build a config when an explicit device id has been provided.
    pub fn build(self) -> Result<CaptureConfig> {
        self.validate()?;
        let Some(device_id) = self.device_id.clone() else {
            anyhow::bail!("capture device id is required; use resolve(manager) to select a device");
        };
        Ok(self.config_for_device(device_id))
    }

    /// Resolve this builder against the manager's registered devices.
    pub fn resolve(self, manager: &CaptureManager) -> Result<CaptureConfig> {
        self.validate()?;

        if let Some(device_id) = &self.device_id {
            let devices = manager.devices(self.kind)?;
            let Some(device) = devices.iter().find(|device| device.id == *device_id) else {
                anyhow::bail!(
                    "capture device {:?} with id {:?} was not found",
                    self.kind,
                    device_id
                );
            };
            if self.require_available && !device.is_available {
                anyhow::bail!(
                    "capture device {:?} with id {:?} is not available",
                    self.kind,
                    device_id
                );
            }
            return Ok(self.config_for_device(device.id.clone()));
        }

        let devices = manager.devices(self.kind)?;
        let name_filter = self
            .device_name_contains
            .as_ref()
            .map(|name| name.to_lowercase());
        let selected = devices
            .iter()
            .find(|device| {
                (!self.require_available || device.is_available)
                    && name_filter
                        .as_ref()
                        .is_none_or(|name| device.name.to_lowercase().contains(name))
            })
            .or_else(|| {
                if self.require_available {
                    None
                } else {
                    devices.iter().find(|device| {
                        name_filter
                            .as_ref()
                            .is_none_or(|name| device.name.to_lowercase().contains(name))
                    })
                }
            })
            .ok_or_else(|| {
                let availability = if self.require_available {
                    "available "
                } else {
                    ""
                };
                let name = self
                    .device_name_contains
                    .as_ref()
                    .map(|name| format!(" matching {name:?}"))
                    .unwrap_or_default();
                anyhow::anyhow!("no {availability}{:?} capture device{name}", self.kind)
            })?;

        Ok(self.config_for_device(selected.id.clone()))
    }

    fn config_for_device(self, device_id: String) -> CaptureConfig {
        CaptureConfig {
            device_id,
            kind: self.kind,
            frame_rate: self.frame_rate,
            resolution: self.resolution,
            include_audio: self.include_audio,
        }
    }
}

impl From<CaptureConfig> for CaptureConfigBuilder {
    fn from(config: CaptureConfig) -> Self {
        Self::new(config.kind)
            .device_id(config.device_id)
            .require_available(false)
            .include_audio(config.include_audio)
            .apply_optional_frame_rate(config.frame_rate)
            .apply_optional_resolution(config.resolution)
    }
}

impl CaptureConfigBuilder {
    fn apply_optional_frame_rate(mut self, frame_rate: Option<f64>) -> Self {
        self.frame_rate = frame_rate;
        self
    }

    fn apply_optional_resolution(mut self, resolution: Option<(u32, u32)>) -> Self {
        self.resolution = resolution;
        self
    }
}

/// Builder for resolving multiple capture sources together.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CaptureConfigSetBuilder {
    sources: Vec<CaptureConfigBuilder>,
}

impl CaptureConfigSetBuilder {
    /// Create an empty capture config set.
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a screen + microphone capture set.
    pub fn screen_with_microphone() -> Self {
        Self::new().screen().microphone()
    }

    /// Create a camera + microphone capture set.
    pub fn camera_with_microphone() -> Self {
        Self::new().camera().microphone()
    }

    /// Create a screen + system-audio capture set.
    pub fn screen_with_system_audio() -> Self {
        Self::new().screen().system_audio()
    }

    /// Add a capture source builder.
    pub fn source(mut self, source: impl Into<CaptureConfigBuilder>) -> Self {
        self.sources.push(source.into());
        self
    }

    /// Add a screen source.
    pub fn screen(self) -> Self {
        self.source(CaptureConfigBuilder::screen())
    }

    /// Add a window source.
    pub fn window(self) -> Self {
        self.source(CaptureConfigBuilder::window())
    }

    /// Add a camera source.
    pub fn camera(self) -> Self {
        self.source(CaptureConfigBuilder::camera())
    }

    /// Add a microphone source.
    pub fn microphone(self) -> Self {
        self.source(CaptureConfigBuilder::microphone())
    }

    /// Add a system-audio source.
    pub fn system_audio(self) -> Self {
        self.source(CaptureConfigBuilder::system_audio())
    }

    /// Apply a frame-rate preference to video sources in the set.
    pub fn video_frame_rate(mut self, frame_rate: f64) -> Self {
        for source in &mut self.sources {
            if matches!(
                source.kind,
                CaptureDeviceKind::Screen | CaptureDeviceKind::Window | CaptureDeviceKind::Camera
            ) {
                source.frame_rate = Some(frame_rate);
            }
        }
        self
    }

    /// Apply a resolution preference to video sources in the set.
    pub fn video_resolution(mut self, width: u32, height: u32) -> Self {
        for source in &mut self.sources {
            if matches!(
                source.kind,
                CaptureDeviceKind::Screen | CaptureDeviceKind::Window | CaptureDeviceKind::Camera
            ) {
                source.resolution = Some((width, height));
            }
        }
        self
    }

    /// Return the configured source builders.
    pub fn sources(&self) -> &[CaptureConfigBuilder] {
        &self.sources
    }

    /// Number of configured source builders.
    pub fn source_count(&self) -> usize {
        self.sources.len()
    }

    /// Number of configured sources with a particular kind.
    pub fn kind_count(&self, kind: CaptureDeviceKind) -> usize {
        self.sources
            .iter()
            .filter(|source| source.kind == kind)
            .count()
    }

    /// Whether any source captures video.
    pub fn has_video(&self) -> bool {
        self.sources
            .iter()
            .any(CaptureConfigBuilder::is_video_source)
    }

    /// Whether any source captures audio.
    pub fn has_audio(&self) -> bool {
        self.sources
            .iter()
            .any(CaptureConfigBuilder::is_audio_source)
    }

    /// Content-safe summary for grouped capture plans.
    pub fn to_text(&self) -> String {
        format!(
            "capture config set: {} sources, video {}, audio {}",
            self.source_count(),
            self.has_video(),
            self.has_audio()
        )
    }

    /// Validate the grouped capture request without resolving devices.
    pub fn validate(&self) -> Result<()> {
        anyhow::ensure!(
            !self.sources.is_empty(),
            "at least one capture source must be configured"
        );
        for source in &self.sources {
            source.validate()?;
        }
        Ok(())
    }

    /// Resolve every source against a capture manager.
    pub fn resolve(self, manager: &CaptureManager) -> Result<Vec<CaptureConfig>> {
        self.validate()?;
        self.sources
            .into_iter()
            .map(|source| source.resolve(manager))
            .collect()
    }
}

impl From<CaptureConfigBuilder> for CaptureConfigSetBuilder {
    fn from(source: CaptureConfigBuilder) -> Self {
        Self::new().source(source)
    }
}

/// Runtime consent surfaces implied by a capture handoff.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureConsentKind {
    /// Microphone input permission.
    Microphone,
    /// Camera/video input permission.
    Camera,
    /// Screen, window, or system-audio capture permission.
    ScreenCapture,
}

impl CaptureConsentKind {
    /// Stable lowercase key for generated setup logs.
    pub fn key(&self) -> &'static str {
        match self {
            Self::Microphone => "microphone",
            Self::Camera => "camera",
            Self::ScreenCapture => "screen-capture",
        }
    }
}

/// The next product action needed before capture can start.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureHandoffNextAction {
    /// Review or request OS capture permissions before touching devices.
    PreflightPermissions,
    /// Present source picker UI from a checked capture source query.
    ShowSourcePicker,
    /// Resolve configured capture sources through a capture manager.
    ResolveCaptureConfigs,
    /// The handoff can be sent to `CaptureManager::pipeline_checked` or
    /// `start_pipeline_checked`.
    StartCapturePipeline,
}

impl CaptureHandoffNextAction {
    /// Stable lowercase key for generated setup logs.
    pub fn key(&self) -> &'static str {
        match self {
            Self::PreflightPermissions => "preflight-permissions",
            Self::ShowSourcePicker => "show-source-picker",
            Self::ResolveCaptureConfigs => "resolve-capture-configs",
            Self::StartCapturePipeline => "start-capture-pipeline",
        }
    }
}

/// Builder-facing handoff for native media capture flows.
///
/// The handoff keeps Electron-style `mediaDevices` and `desktopCapturer`
/// replacement work explicit: permission preflight, optional source picker,
/// source resolution, and pipeline startup are separate checked steps.
#[derive(Debug, Clone, PartialEq)]
pub struct CaptureHandoffBuilder {
    source_query: Option<CaptureSourceQueryBuilder>,
    config_set: CaptureConfigSetBuilder,
    require_permission_preflight: bool,
    require_source_picker: bool,
    auto_start: bool,
}

impl CaptureHandoffBuilder {
    /// Create a handoff from a grouped capture config set.
    pub fn new(config_set: impl Into<CaptureConfigSetBuilder>) -> Self {
        Self {
            source_query: None,
            config_set: config_set.into(),
            require_permission_preflight: true,
            require_source_picker: false,
            auto_start: false,
        }
    }

    /// Create a handoff for screen sharing with microphone audio.
    pub fn screen_share_with_microphone() -> Self {
        Self::new(CaptureConfigSetBuilder::screen_with_microphone())
            .source_query(CaptureSourceQueryBuilder::screens_and_windows())
            .require_source_picker(true)
    }

    /// Create a handoff for a camera call with microphone audio.
    pub fn camera_call() -> Self {
        Self::new(CaptureConfigSetBuilder::camera_with_microphone())
            .source_query(CaptureSourceQueryBuilder::cameras())
    }

    /// Create a handoff for screen recording with system audio.
    pub fn screen_recording_with_system_audio() -> Self {
        Self::new(CaptureConfigSetBuilder::screen_with_system_audio())
            .source_query(CaptureSourceQueryBuilder::screens_and_windows())
            .require_source_picker(true)
    }

    /// Attach a source query for picker UI or source availability preflight.
    pub fn source_query(mut self, query: CaptureSourceQueryBuilder) -> Self {
        self.source_query = Some(query);
        self
    }

    /// Require explicit source-picker UI before resolving configs.
    pub fn require_source_picker(mut self, require: bool) -> Self {
        self.require_source_picker = require;
        self
    }

    /// Require permission preflight before source enumeration or capture start.
    pub fn require_permission_preflight(mut self, require: bool) -> Self {
        self.require_permission_preflight = require;
        self
    }

    /// Mark this handoff as ready to start immediately after configs resolve.
    pub fn auto_start(mut self, auto_start: bool) -> Self {
        self.auto_start = auto_start;
        self
    }

    /// Return the source query, when one is configured.
    pub fn source_query_ref(&self) -> Option<&CaptureSourceQueryBuilder> {
        self.source_query.as_ref()
    }

    /// Return the grouped capture config builder.
    pub fn config_set_ref(&self) -> &CaptureConfigSetBuilder {
        &self.config_set
    }

    /// Whether source picker UI is required before capture config resolution.
    pub fn requires_source_picker(&self) -> bool {
        self.require_source_picker
    }

    /// Whether OS permission preflight is required before capture work.
    pub fn requires_permission_preflight(&self) -> bool {
        self.require_permission_preflight
    }

    /// Whether this handoff should start the pipeline after resolution.
    pub fn auto_starts(&self) -> bool {
        self.auto_start
    }

    /// Consent surfaces implied by the configured capture sources.
    pub fn required_consents(&self) -> Vec<CaptureConsentKind> {
        capture_required_consents(&self.config_set)
    }

    /// The next product action implied by this handoff.
    pub fn next_action(&self) -> CaptureHandoffNextAction {
        if self.require_permission_preflight && !self.required_consents().is_empty() {
            CaptureHandoffNextAction::PreflightPermissions
        } else if self.require_source_picker {
            CaptureHandoffNextAction::ShowSourcePicker
        } else if self.auto_start {
            CaptureHandoffNextAction::StartCapturePipeline
        } else {
            CaptureHandoffNextAction::ResolveCaptureConfigs
        }
    }

    /// Content-safe summary for generated capture setup logs.
    pub fn to_text(&self) -> String {
        format!(
            "capture handoff builder: sources {}, query {}, permission-preflight {}, source-picker {}, auto-start {}, consents {}, next action {}",
            self.config_set.source_count(),
            self.source_query.is_some(),
            self.require_permission_preflight,
            self.require_source_picker,
            self.auto_start,
            self.required_consents().len(),
            self.next_action().key()
        )
    }

    /// Validate the handoff before source enumeration or capture startup.
    pub fn validate(&self) -> Result<()> {
        self.config_set.validate()?;
        if let Some(query) = &self.source_query {
            query.validate()?;
        }
        anyhow::ensure!(
            self.config_set.has_video() || self.config_set.has_audio(),
            "capture handoff must include video or audio sources"
        );
        Ok(())
    }

    /// Validate and build the capture handoff.
    pub fn build_checked(self) -> Result<CaptureHandoff> {
        self.validate()?;
        let next_action = self.next_action();
        let required_consents = self.required_consents();
        Ok(CaptureHandoff {
            source_query: self.source_query,
            config_set: self.config_set,
            required_consents,
            next_action,
            require_permission_preflight: self.require_permission_preflight,
            require_source_picker: self.require_source_picker,
            auto_start: self.auto_start,
        })
    }
}

/// Checked native capture setup handoff for source picking, consent, and
/// pipeline creation.
#[derive(Debug, Clone, PartialEq)]
pub struct CaptureHandoff {
    source_query: Option<CaptureSourceQueryBuilder>,
    config_set: CaptureConfigSetBuilder,
    required_consents: Vec<CaptureConsentKind>,
    next_action: CaptureHandoffNextAction,
    require_permission_preflight: bool,
    require_source_picker: bool,
    auto_start: bool,
}

impl CaptureHandoff {
    /// Build a checked screen-share handoff with microphone audio.
    pub fn screen_share_with_microphone() -> Result<Self> {
        CaptureHandoffBuilder::screen_share_with_microphone().build_checked()
    }

    /// Build a checked camera-call handoff with microphone audio.
    pub fn camera_call() -> Result<Self> {
        CaptureHandoffBuilder::camera_call().build_checked()
    }

    /// Build a checked screen-recording handoff with system audio.
    pub fn screen_recording_with_system_audio() -> Result<Self> {
        CaptureHandoffBuilder::screen_recording_with_system_audio().build_checked()
    }

    /// Return the source query, when one is configured.
    pub fn source_query(&self) -> Option<&CaptureSourceQueryBuilder> {
        self.source_query.as_ref()
    }

    /// Return the grouped capture config builder.
    pub fn config_set(&self) -> &CaptureConfigSetBuilder {
        &self.config_set
    }

    /// Return the consent surfaces implied by this handoff.
    pub fn required_consents(&self) -> &[CaptureConsentKind] {
        &self.required_consents
    }

    /// Return whether a consent surface is required.
    pub fn requires_consent(&self, consent: CaptureConsentKind) -> bool {
        self.required_consents.contains(&consent)
    }

    /// Whether source picker UI is required before capture config resolution.
    pub fn requires_source_picker(&self) -> bool {
        self.require_source_picker
    }

    /// Whether OS permission preflight is required before capture work.
    pub fn requires_permission_preflight(&self) -> bool {
        self.require_permission_preflight
    }

    /// Whether this handoff should start the pipeline after resolution.
    pub fn auto_starts(&self) -> bool {
        self.auto_start
    }

    /// The next product action implied by this checked handoff.
    pub fn next_action(&self) -> CaptureHandoffNextAction {
        self.next_action
    }

    /// Resolve source configs through a capture manager.
    pub fn resolve_configs(&self, manager: &CaptureManager) -> Result<Vec<CaptureConfig>> {
        self.config_set.clone().resolve(manager)
    }

    /// Build a capture pipeline through a capture manager without starting it.
    pub fn pipeline_checked(
        &self,
        manager: &CaptureManager,
        callback: FrameCallback,
    ) -> Result<CapturePipeline> {
        manager.pipeline_checked(self.config_set.clone(), callback)
    }

    /// Build and start a capture pipeline through a capture manager.
    pub fn start_pipeline_checked(
        &self,
        manager: &CaptureManager,
        callback: FrameCallback,
    ) -> Result<CapturePipeline> {
        manager.start_pipeline_checked(self.config_set.clone(), callback)
    }

    /// Content-safe summary for generated capture setup logs.
    pub fn to_text(&self) -> String {
        format!(
            "capture handoff: sources {}, query {}, permission-preflight {}, source-picker {}, auto-start {}, consents {}, next action {}",
            self.config_set.source_count(),
            self.source_query.is_some(),
            self.require_permission_preflight,
            self.require_source_picker,
            self.auto_start,
            self.required_consents.len(),
            self.next_action.key()
        )
    }
}

fn capture_required_consents(config_set: &CaptureConfigSetBuilder) -> Vec<CaptureConsentKind> {
    let mut consents = Vec::new();
    for source in config_set.sources() {
        let consent = match source.kind() {
            CaptureDeviceKind::Microphone => Some(CaptureConsentKind::Microphone),
            CaptureDeviceKind::Camera => Some(CaptureConsentKind::Camera),
            CaptureDeviceKind::Screen
            | CaptureDeviceKind::Window
            | CaptureDeviceKind::SystemAudio => Some(CaptureConsentKind::ScreenCapture),
        };
        if let Some(consent) = consent
            && !consents.contains(&consent)
        {
            consents.push(consent);
        }
    }
    consents
}

/// A single frame of captured media.
#[derive(Debug, Clone, PartialEq)]
pub enum CaptureFrame {
    /// A video frame with pixel data.
    Video {
        /// Width in pixels.
        width: u32,
        /// Height in pixels.
        height: u32,
        /// Pixel format descriptor.
        format: PixelFormat,
        /// Raw frame bytes.
        data: Arc<Vec<u8>>,
        /// Presentation timestamp in milliseconds.
        timestamp_ms: u64,
    },
    /// An audio frame with sample data.
    Audio {
        /// Number of audio channels.
        channels: u16,
        /// Sample rate in Hz.
        sample_rate: u32,
        /// Raw PCM sample bytes.
        data: Arc<Vec<u8>>,
        /// Presentation timestamp in milliseconds.
        timestamp_ms: u64,
    },
}

/// Supported pixel formats for video frames.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PixelFormat {
    /// 32-bit BGRA.
    Bgra32,
    /// 32-bit RGBA.
    Rgba32,
    /// NV12 (YUV 4:2:0 semi-planar).
    Nv12,
    /// I420 (YUV 4:2:0 planar).
    I420,
}

/// Callback invoked for each captured frame.
pub type FrameCallback = Arc<dyn Fn(CaptureFrame) + Send + Sync>;

struct CapturePipelineEntry {
    session: Box<dyn CaptureSession>,
    config: CaptureConfig,
    callback: FrameCallback,
    dropped: Arc<AtomicU64>,
    total_latency_ms: Arc<AtomicU64>,
    frame_count: Arc<AtomicU64>,
}

/// A capture session that produces frames from a device.
pub trait CaptureSession: Send {
    /// Start capturing with the given configuration and frame callback.
    fn start(&mut self, config: CaptureConfig, callback: FrameCallback) -> Result<()>;
    /// Pause capturing without releasing resources.
    fn pause(&mut self) -> Result<()>;
    /// Resume a paused session.
    fn resume(&mut self) -> Result<()>;
    /// Stop capturing and release resources.
    fn stop(&mut self) -> Result<()>;
    /// Get the current session state.
    fn state(&self) -> CaptureSessionState;
    /// Number of frames dropped due to backpressure or errors.
    fn dropped_frame_count(&self) -> u64 {
        0
    }
    /// Average processing latency in milliseconds.
    fn latency_ms(&self) -> u64 {
        0
    }
}

fn trace_capture_event(name: &str, phase: TracePhase) {
    if let Some(tracer) = Tracer::global() {
        tracer.record(name, "capture", phase);
    }
}

fn wrap_callback_with_backpressure(
    callback: FrameCallback,
) -> (
    FrameCallback,
    Arc<AtomicU64>,
    Arc<AtomicU64>,
    Arc<AtomicU64>,
) {
    let in_flight = Arc::new(AtomicBool::new(false));
    let dropped = Arc::new(AtomicU64::new(0));
    let total_latency = Arc::new(AtomicU64::new(0));
    let frame_count = Arc::new(AtomicU64::new(0));

    let wrapped = Arc::new({
        let in_flight = Arc::clone(&in_flight);
        let dropped = Arc::clone(&dropped);
        let total_latency = Arc::clone(&total_latency);
        let frame_count = Arc::clone(&frame_count);
        let callback = Arc::clone(&callback);

        move |frame: CaptureFrame| {
            if in_flight.swap(true, Ordering::SeqCst) {
                dropped.fetch_add(1, Ordering::Relaxed);
                trace_capture_event("capture_frame_dropped", TracePhase::Instant);
                return;
            }

            let start = Instant::now();
            callback(frame);
            let elapsed = start.elapsed().as_millis() as u64;

            total_latency.fetch_add(elapsed, Ordering::Relaxed);
            frame_count.fetch_add(1, Ordering::Relaxed);
            in_flight.store(false, Ordering::SeqCst);
        }
    }) as FrameCallback;

    (wrapped, dropped, total_latency, frame_count)
}

// ---------------------------------------------------------------------------
// Capture Pipeline
// ---------------------------------------------------------------------------

/// A high-level capture pipeline that manages one or more capture sessions.
///
/// Pipelines are used to coordinate multi-source capture (e.g., screen +
/// microphone) with synchronized timestamps.
pub struct CapturePipeline {
    sessions: Vec<CapturePipelineEntry>,
    running: bool,
}

impl CapturePipeline {
    /// Create an empty pipeline.
    pub fn new() -> Self {
        Self {
            sessions: Vec::new(),
            running: false,
        }
    }

    /// Add a capture session to the pipeline with its startup config and callback.
    pub fn add_session(
        &mut self,
        session: Box<dyn CaptureSession>,
        config: CaptureConfig,
        callback: FrameCallback,
    ) {
        self.sessions.push(CapturePipelineEntry {
            session,
            config,
            callback,
            dropped: Arc::new(AtomicU64::new(0)),
            total_latency_ms: Arc::new(AtomicU64::new(0)),
            frame_count: Arc::new(AtomicU64::new(0)),
        });
    }

    /// Start all sessions in the pipeline.
    pub fn start_all(&mut self) -> Result<()> {
        let mut started = 0usize;
        for index in 0..self.sessions.len() {
            let should_start = matches!(
                self.sessions[index].session.state(),
                CaptureSessionState::Idle | CaptureSessionState::Stopped
            );

            if should_start {
                let config = self.sessions[index].config.clone();
                let (wrapped_callback, dropped, total_latency, frame_count) =
                    wrap_callback_with_backpressure(Arc::clone(&self.sessions[index].callback));
                self.sessions[index].dropped = dropped;
                self.sessions[index].total_latency_ms = total_latency;
                self.sessions[index].frame_count = frame_count;

                trace_capture_event("capture_pipeline_start", TracePhase::Begin);
                if let Err(error) = self.sessions[index].session.start(config, wrapped_callback) {
                    self.running = false;
                    for started_entry in &mut self.sessions[..started] {
                        let _ = started_entry.session.stop();
                    }
                    trace_capture_event("capture_pipeline_start", TracePhase::End);
                    return Err(error);
                }
                trace_capture_event("capture_pipeline_start", TracePhase::End);
                started += 1;
            }
        }
        self.running = true;
        Ok(())
    }

    /// Stop all sessions in the pipeline.
    pub fn stop_all(&mut self) -> Result<()> {
        trace_capture_event("capture_pipeline_stop", TracePhase::Begin);
        for entry in &mut self.sessions {
            let _ = entry.session.stop();
        }
        trace_capture_event("capture_pipeline_stop", TracePhase::End);
        self.running = false;
        Ok(())
    }

    /// Pause all running sessions in the pipeline.
    pub fn pause_all(&mut self) -> Result<()> {
        for entry in &mut self.sessions {
            if entry.session.state() == CaptureSessionState::Running {
                entry.session.pause()?;
            }
        }
        self.running = false;
        Ok(())
    }

    /// Resume all paused sessions in the pipeline.
    pub fn resume_all(&mut self) -> Result<()> {
        for entry in &mut self.sessions {
            if entry.session.state() == CaptureSessionState::Paused {
                entry.session.resume()?;
            }
        }
        self.running = true;
        Ok(())
    }

    /// Whether the pipeline is running.
    pub fn is_running(&self) -> bool {
        self.running
    }

    /// The number of sessions in the pipeline.
    pub fn session_count(&self) -> usize {
        self.sessions.len()
    }

    /// Return the current state of each session in the pipeline.
    pub fn session_states(&self) -> Vec<CaptureSessionState> {
        self.sessions
            .iter()
            .map(|entry| entry.session.state())
            .collect()
    }

    /// Total frames dropped across all sessions in the pipeline.
    pub fn total_dropped_frames(&self) -> u64 {
        self.sessions
            .iter()
            .map(|entry| {
                entry.dropped.load(Ordering::Relaxed) + entry.session.dropped_frame_count()
            })
            .sum()
    }

    /// Average latency across all sessions in the pipeline.
    pub fn average_latency_ms(&self) -> u64 {
        let mut total = 0u64;
        let mut count = 0u64;
        for entry in &self.sessions {
            let fc = entry.frame_count.load(Ordering::Relaxed);
            total += entry
                .total_latency_ms
                .load(Ordering::Relaxed)
                .checked_div(fc)
                .unwrap_or(0);
            if fc > 0 {
                count += 1;
            }
        }
        total.checked_div(count).unwrap_or(0)
    }
}

impl Default for CapturePipeline {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Timing / Synchronization
// ---------------------------------------------------------------------------

/// A timestamp source for synchronizing multiple capture streams.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ClockSource {
    /// Use the system monotonic clock.
    SystemMonotonic,
    /// Use an audio device clock as the master.
    AudioDevice,
    /// Use a custom external clock.
    External,
}

/// Synchronization primitives for capture streams.
pub struct CaptureSync {
    clock_source: ClockSource,
    base_timestamp_ms: u64,
}

impl CaptureSync {
    /// Create a new sync context with the given clock source.
    pub fn new(clock_source: ClockSource) -> Self {
        Self {
            clock_source,
            base_timestamp_ms: 0,
        }
    }

    /// Set the base timestamp for synchronization.
    pub fn set_base_timestamp(&mut self, timestamp_ms: u64) {
        self.base_timestamp_ms = timestamp_ms;
    }

    /// Convert a raw timestamp to a synchronized timestamp.
    pub fn synchronize(&self, raw_timestamp_ms: u64) -> u64 {
        raw_timestamp_ms.saturating_sub(self.base_timestamp_ms)
    }

    /// The current clock source.
    pub fn clock_source(&self) -> ClockSource {
        self.clock_source
    }
}

/// Create a capture manager with platform-default backends registered.
pub fn default_capture_manager() -> CaptureManager {
    let mut manager = CaptureManager::new();

    #[cfg(target_os = "macos")]
    {
        use crate::platform::MacMediaCaptureBackend;
        #[cfg(feature = "screen-capture")]
        use crate::platform::MacScreenCaptureBackend;

        #[cfg(feature = "screen-capture")]
        manager.register_backend(
            CaptureDeviceKind::Screen,
            Arc::new(MacScreenCaptureBackend::new()),
        );
        manager.register_backend(
            CaptureDeviceKind::Camera,
            Arc::new(MacMediaCaptureBackend::new()),
        );
        manager.register_backend(
            CaptureDeviceKind::Microphone,
            Arc::new(MacMediaCaptureBackend::new()),
        );
    }

    #[cfg(target_os = "windows")]
    {
        use crate::platform::WindowsMicrophoneBackend;
        use crate::platform::WindowsScreenCaptureBackend;
        use crate::platform::WindowsSystemAudioBackend;

        manager.register_backend(
            CaptureDeviceKind::Screen,
            Arc::new(WindowsScreenCaptureBackend::new()),
        );
        manager.register_backend(
            CaptureDeviceKind::Microphone,
            Arc::new(WindowsMicrophoneBackend::new()),
        );
        manager.register_backend(
            CaptureDeviceKind::SystemAudio,
            Arc::new(WindowsSystemAudioBackend::new()),
        );
    }

    #[cfg(any(target_os = "linux", target_os = "freebsd"))]
    {
        use crate::platform::LinuxMicrophoneBackend;
        use crate::platform::PipeWireCaptureBackend;
        use crate::platform::XdgDesktopPortalCaptureBackend;

        manager.register_backend(
            CaptureDeviceKind::Screen,
            Arc::new(XdgDesktopPortalCaptureBackend::new()),
        );
        manager.register_backend(
            CaptureDeviceKind::Camera,
            Arc::new(PipeWireCaptureBackend::new()),
        );
        manager.register_backend(
            CaptureDeviceKind::Microphone,
            Arc::new(LinuxMicrophoneBackend::new()),
        );
    }

    manager
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn test_capture_device_info_serialization() {
        let info = CaptureDeviceInfo {
            id: "screen-0".to_string(),
            name: "Main Display".to_string(),
            kind: CaptureDeviceKind::Screen,
            is_available: true,
        };
        let json = serde_json::to_string(&info).unwrap();
        let decoded: CaptureDeviceInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(info, decoded);
    }

    #[test]
    fn test_capture_config_defaults() {
        let config = CaptureConfig {
            device_id: "mic-1".to_string(),
            kind: CaptureDeviceKind::Microphone,
            frame_rate: None,
            resolution: None,
            include_audio: true,
        };
        assert_eq!(config.kind, CaptureDeviceKind::Microphone);
        assert!(config.include_audio);
    }

    #[test]
    fn test_capture_config_builder_validates_and_builds_explicit_device() {
        let config = CaptureConfigBuilder::screen()
            .device_id("screen-1")
            .frame_rate(30.0)
            .resolution(1280, 720)
            .with_audio()
            .build()
            .unwrap();

        assert_eq!(config.device_id, "screen-1");
        assert_eq!(config.kind, CaptureDeviceKind::Screen);
        assert_eq!(config.frame_rate, Some(30.0));
        assert_eq!(config.resolution, Some((1280, 720)));
        assert!(config.include_audio);
        assert!(CaptureConfigBuilder::camera().build().is_err());
        assert!(
            CaptureConfigBuilder::microphone()
                .device_id(" ")
                .validate()
                .is_err()
        );
        assert!(
            CaptureConfigBuilder::microphone()
                .device_name_contains(" ")
                .validate()
                .is_err()
        );
        assert!(
            CaptureConfigBuilder::screen()
                .device_id("screen-1")
                .frame_rate(0.0)
                .validate()
                .is_err()
        );
        assert!(
            CaptureConfigBuilder::screen()
                .device_id("screen-1")
                .resolution(0, 720)
                .validate()
                .is_err()
        );
    }

    #[test]
    fn test_capture_source_query_filters_and_limits_devices() {
        struct MockBackend {
            kind: CaptureDeviceKind,
            devices: Vec<CaptureDeviceInfo>,
        }

        impl DeviceEnumerator for MockBackend {
            fn devices(&self, kind: CaptureDeviceKind) -> Result<Vec<CaptureDeviceInfo>> {
                assert_eq!(kind, self.kind);
                Ok(self.devices.clone())
            }
        }

        impl CaptureBackend for MockBackend {
            fn create_session(&self, _config: &CaptureConfig) -> Result<Box<dyn CaptureSession>> {
                struct MockSession;
                impl CaptureSession for MockSession {
                    fn start(
                        &mut self,
                        _config: CaptureConfig,
                        _callback: FrameCallback,
                    ) -> Result<()> {
                        Ok(())
                    }

                    fn pause(&mut self) -> Result<()> {
                        Ok(())
                    }

                    fn resume(&mut self) -> Result<()> {
                        Ok(())
                    }

                    fn stop(&mut self) -> Result<()> {
                        Ok(())
                    }

                    fn state(&self) -> CaptureSessionState {
                        CaptureSessionState::Idle
                    }
                }
                Ok(Box::new(MockSession))
            }
        }

        let mut manager = CaptureManager::new();
        manager.register_backend(
            CaptureDeviceKind::Screen,
            Arc::new(MockBackend {
                kind: CaptureDeviceKind::Screen,
                devices: vec![
                    CaptureDeviceInfo {
                        id: "screen-main".to_string(),
                        name: "Main Display".to_string(),
                        kind: CaptureDeviceKind::Screen,
                        is_available: true,
                    },
                    CaptureDeviceInfo {
                        id: "screen-side".to_string(),
                        name: "Side Display".to_string(),
                        kind: CaptureDeviceKind::Screen,
                        is_available: true,
                    },
                ],
            }),
        );
        manager.register_backend(
            CaptureDeviceKind::Window,
            Arc::new(MockBackend {
                kind: CaptureDeviceKind::Window,
                devices: vec![
                    CaptureDeviceInfo {
                        id: "window-editor".to_string(),
                        name: "Code Editor".to_string(),
                        kind: CaptureDeviceKind::Window,
                        is_available: true,
                    },
                    CaptureDeviceInfo {
                        id: "window-hidden".to_string(),
                        name: "Hidden Window".to_string(),
                        kind: CaptureDeviceKind::Window,
                        is_available: false,
                    },
                ],
            }),
        );

        let catalog = manager
            .sources(
                CaptureSourceQueryBuilder::screens_and_windows()
                    .name_contains("display")
                    .limit(1),
            )
            .unwrap();

        assert_eq!(catalog.len(), 1);
        assert_eq!(catalog.available_count(), 1);
        assert_eq!(catalog.unavailable_count(), 0);
        assert_eq!(catalog.kind_count(CaptureDeviceKind::Screen), 1);
        assert_eq!(
            catalog.to_text(),
            "capture source catalog: 1 devices, 1 available, 0 unavailable"
        );
        assert!(!catalog.to_text().contains("screen-main"));
        assert!(!catalog.to_text().contains("Display"));
        assert_eq!(catalog.first().unwrap().id, "screen-main");
        assert_eq!(
            catalog.first().unwrap().to_text(),
            "capture device screen: available true"
        );
        assert!(!catalog.first().unwrap().to_text().contains("screen-main"));
        assert!(!catalog.first().unwrap().to_text().contains("Main Display"));
        let config = catalog.first_config(CaptureDeviceKind::Screen).unwrap();
        assert_eq!(config.device_id, "screen-main");

        let windows = manager
            .sources(CaptureSourceQueryBuilder::windows().include_unavailable())
            .unwrap();
        assert_eq!(windows.len(), 2);
        assert_eq!(windows.devices()[1].id, "window-hidden");

        let mismatch = windows.first_config(CaptureDeviceKind::Screen);
        assert!(mismatch.is_err());
    }

    #[test]
    fn test_capture_source_query_validates_shape() {
        assert!(
            CaptureSourceQueryBuilder::screens()
                .name_contains(" main")
                .validate()
                .is_err()
        );
        assert!(
            CaptureSourceQueryBuilder::screens()
                .name_contains(" ")
                .validate()
                .is_err()
        );
        assert!(
            CaptureSourceQueryBuilder::screens()
                .limit(0)
                .validate()
                .is_err()
        );

        let query = CaptureSourceQueryBuilder::screens()
            .kind(CaptureDeviceKind::Screen)
            .kind(CaptureDeviceKind::Window);
        assert_eq!(
            query.kinds(),
            &[CaptureDeviceKind::Screen, CaptureDeviceKind::Window]
        );
        assert_eq!(
            query.to_text(),
            "capture source query: 2 kinds, name-filter false, require-available true, include-unavailable false, limit false"
        );
        assert!(query.validate().is_ok());

        let filtered = CaptureSourceQueryBuilder::windows()
            .name_contains("Code")
            .include_unavailable()
            .limit(2);
        assert!(filtered.has_name_filter());
        assert!(filtered.has_limit());
        assert_eq!(
            filtered.to_text(),
            "capture source query: 1 kinds, name-filter true, require-available false, include-unavailable true, limit true"
        );
        assert!(!filtered.to_text().contains("Code"));

        let empty = CaptureSourceCatalog { devices: vec![] };
        assert!(empty.is_empty());
        assert!(empty.first_config(CaptureDeviceKind::Screen).is_err());
    }

    #[test]
    fn test_capture_config_builder_resolves_available_devices() {
        struct MockBackend;

        impl DeviceEnumerator for MockBackend {
            fn devices(&self, kind: CaptureDeviceKind) -> Result<Vec<CaptureDeviceInfo>> {
                Ok(vec![
                    CaptureDeviceInfo {
                        id: "screen-unavailable".to_string(),
                        name: "Offline Display".to_string(),
                        kind,
                        is_available: false,
                    },
                    CaptureDeviceInfo {
                        id: "screen-main".to_string(),
                        name: "Main Display".to_string(),
                        kind,
                        is_available: true,
                    },
                ])
            }
        }

        impl CaptureBackend for MockBackend {
            fn create_session(&self, _config: &CaptureConfig) -> Result<Box<dyn CaptureSession>> {
                struct MockSession;

                impl CaptureSession for MockSession {
                    fn start(
                        &mut self,
                        _config: CaptureConfig,
                        _callback: FrameCallback,
                    ) -> Result<()> {
                        Ok(())
                    }

                    fn pause(&mut self) -> Result<()> {
                        Ok(())
                    }

                    fn resume(&mut self) -> Result<()> {
                        Ok(())
                    }

                    fn stop(&mut self) -> Result<()> {
                        Ok(())
                    }

                    fn state(&self) -> CaptureSessionState {
                        CaptureSessionState::Idle
                    }
                }

                Ok(Box::new(MockSession))
            }
        }

        let mut manager = CaptureManager::new();
        manager.register_backend(CaptureDeviceKind::Screen, Arc::new(MockBackend));

        let config = CaptureConfigBuilder::screen()
            .device_name_contains("main")
            .frame_rate(60.0)
            .resolve(&manager)
            .unwrap();
        assert_eq!(config.device_id, "screen-main");
        assert_eq!(config.frame_rate, Some(60.0));
        assert_eq!(
            config.to_text(),
            "capture config screen: frame-rate true, resolution false, audio false"
        );
        assert!(!config.to_text().contains("screen-main"));

        let unavailable = CaptureConfigBuilder::screen()
            .device_id("screen-unavailable")
            .resolve(&manager);
        assert!(unavailable.is_err());

        let unavailable_allowed = CaptureConfigBuilder::screen()
            .device_id("screen-unavailable")
            .allow_unavailable()
            .resolve(&manager)
            .unwrap();
        assert_eq!(unavailable_allowed.device_id, "screen-unavailable");

        let (config, session) = manager
            .create_session_with(CaptureConfigBuilder::screen().device_name_contains("main"))
            .unwrap();
        assert_eq!(config.device_id, "screen-main");
        assert_eq!(session.state(), CaptureSessionState::Idle);
    }

    #[test]
    fn test_capture_config_set_builder_resolves_common_presets() {
        struct MockBackend;

        impl DeviceEnumerator for MockBackend {
            fn devices(&self, kind: CaptureDeviceKind) -> Result<Vec<CaptureDeviceInfo>> {
                let (id, name) = match kind {
                    CaptureDeviceKind::Screen => ("screen-main", "Main Display"),
                    CaptureDeviceKind::Window => ("window-editor", "Editor Window"),
                    CaptureDeviceKind::Microphone => ("mic-built-in", "Built-in Microphone"),
                    CaptureDeviceKind::Camera => ("camera-front", "Front Camera"),
                    CaptureDeviceKind::SystemAudio => ("system-audio", "System Audio"),
                };
                Ok(vec![CaptureDeviceInfo {
                    id: id.to_string(),
                    name: name.to_string(),
                    kind,
                    is_available: true,
                }])
            }
        }

        impl CaptureBackend for MockBackend {
            fn create_session(&self, _config: &CaptureConfig) -> Result<Box<dyn CaptureSession>> {
                struct MockSession;

                impl CaptureSession for MockSession {
                    fn start(
                        &mut self,
                        _config: CaptureConfig,
                        _callback: FrameCallback,
                    ) -> Result<()> {
                        Ok(())
                    }

                    fn pause(&mut self) -> Result<()> {
                        Ok(())
                    }

                    fn resume(&mut self) -> Result<()> {
                        Ok(())
                    }

                    fn stop(&mut self) -> Result<()> {
                        Ok(())
                    }

                    fn state(&self) -> CaptureSessionState {
                        CaptureSessionState::Idle
                    }
                }

                Ok(Box::new(MockSession))
            }
        }

        let mut manager = CaptureManager::new();
        let backend = Arc::new(MockBackend);
        for kind in [
            CaptureDeviceKind::Screen,
            CaptureDeviceKind::Window,
            CaptureDeviceKind::Microphone,
            CaptureDeviceKind::Camera,
            CaptureDeviceKind::SystemAudio,
        ] {
            manager.register_backend(kind, backend.clone());
        }

        let configs = manager
            .configs(
                CaptureConfigSetBuilder::screen_with_microphone()
                    .video_frame_rate(30.0)
                    .video_resolution(1280, 720),
            )
            .unwrap();

        assert_eq!(configs.len(), 2);
        assert_eq!(configs[0].kind, CaptureDeviceKind::Screen);
        assert_eq!(configs[0].device_id, "screen-main");
        assert_eq!(configs[0].frame_rate, Some(30.0));
        assert_eq!(configs[0].resolution, Some((1280, 720)));
        assert_eq!(configs[1].kind, CaptureDeviceKind::Microphone);
        assert_eq!(configs[1].device_id, "mic-built-in");
        assert_eq!(configs[1].frame_rate, None);
        assert_eq!(configs[1].resolution, None);

        let camera_call = CaptureConfigSetBuilder::camera_with_microphone()
            .resolve(&manager)
            .unwrap();
        assert_eq!(
            camera_call
                .iter()
                .map(|config| config.kind)
                .collect::<Vec<_>>(),
            vec![CaptureDeviceKind::Camera, CaptureDeviceKind::Microphone]
        );

        let system_audio = CaptureConfigSetBuilder::screen_with_system_audio()
            .resolve(&manager)
            .unwrap();
        assert_eq!(
            system_audio
                .iter()
                .map(|config| config.kind)
                .collect::<Vec<_>>(),
            vec![CaptureDeviceKind::Screen, CaptureDeviceKind::SystemAudio]
        );

        assert!(CaptureConfigSetBuilder::new().validate().is_err());
        assert!(
            CaptureConfigSetBuilder::new()
                .source(CaptureConfigBuilder::screen().frame_rate(f64::NAN))
                .validate()
                .is_err()
        );
    }

    #[test]
    fn test_capture_builders_have_safe_summaries() {
        assert_eq!(CaptureDeviceKind::SystemAudio.key(), "system-audio");

        let builder = CaptureConfigBuilder::window()
            .device_id("window-private")
            .device_name_contains("Secret Project")
            .frame_rate(30.0)
            .resolution(1920, 1080)
            .with_audio();
        assert_eq!(builder.kind(), CaptureDeviceKind::Window);
        assert!(builder.has_device_id());
        assert!(builder.has_device_name_filter());
        assert!(builder.has_frame_rate());
        assert!(builder.has_resolution());
        assert!(builder.is_video_source());
        assert!(builder.is_audio_source());

        let summary = builder.to_text();
        assert_eq!(
            summary,
            "capture config builder window: device-id true, name-filter true, require-available true, frame-rate true, resolution true, audio true"
        );
        assert!(!summary.contains("window-private"));
        assert!(!summary.contains("Secret Project"));
        assert!(!summary.contains("1920"));

        let config = CaptureConfig::new("camera-private", CaptureDeviceKind::Camera)
            .frame_rate(24.0)
            .resolution(1280, 720);
        assert!(config.is_video_source());
        assert!(!config.is_audio_source());
        let config_summary = config.to_text();
        assert_eq!(
            config_summary,
            "capture config camera: frame-rate true, resolution true, audio false"
        );
        assert!(!config_summary.contains("camera-private"));

        let set = CaptureConfigSetBuilder::screen_with_microphone()
            .source(CaptureConfigBuilder::system_audio());
        assert_eq!(set.source_count(), 3);
        assert_eq!(set.kind_count(CaptureDeviceKind::Screen), 1);
        assert!(set.has_video());
        assert!(set.has_audio());
        assert_eq!(
            set.to_text(),
            "capture config set: 3 sources, video true, audio true"
        );
    }

    #[test]
    fn test_capture_handoff_guides_permission_picker_and_startup_actions() {
        let builder = CaptureHandoffBuilder::screen_share_with_microphone()
            .auto_start(true)
            .source_query(
                CaptureSourceQueryBuilder::screens_and_windows()
                    .name_contains("Main Display")
                    .limit(2),
            );
        assert_eq!(
            builder.required_consents(),
            vec![
                CaptureConsentKind::ScreenCapture,
                CaptureConsentKind::Microphone
            ]
        );
        assert_eq!(
            builder.next_action(),
            CaptureHandoffNextAction::PreflightPermissions
        );
        assert!(builder.requires_permission_preflight());
        assert!(builder.requires_source_picker());
        assert!(builder.auto_starts());
        assert_eq!(CaptureConsentKind::ScreenCapture.key(), "screen-capture");
        assert_eq!(
            CaptureHandoffNextAction::ShowSourcePicker.key(),
            "show-source-picker"
        );
        let summary = builder.to_text();
        assert_eq!(
            summary,
            "capture handoff builder: sources 2, query true, permission-preflight true, source-picker true, auto-start true, consents 2, next action preflight-permissions"
        );
        assert!(!summary.contains("Main Display"));

        let handoff = builder.build_checked().unwrap();
        assert_eq!(
            handoff.next_action(),
            CaptureHandoffNextAction::PreflightPermissions
        );
        assert!(handoff.source_query().is_some());
        assert_eq!(handoff.config_set().source_count(), 2);
        assert!(handoff.requires_consent(CaptureConsentKind::ScreenCapture));
        assert!(handoff.requires_consent(CaptureConsentKind::Microphone));
        assert!(!handoff.requires_consent(CaptureConsentKind::Camera));
        assert_eq!(handoff.required_consents().len(), 2);
        assert_eq!(
            handoff.to_text(),
            "capture handoff: sources 2, query true, permission-preflight true, source-picker true, auto-start true, consents 2, next action preflight-permissions"
        );
        assert!(!handoff.to_text().contains("Main Display"));

        let picker = CaptureHandoffBuilder::screen_share_with_microphone()
            .require_permission_preflight(false)
            .build_checked()
            .unwrap();
        assert_eq!(
            picker.next_action(),
            CaptureHandoffNextAction::ShowSourcePicker
        );

        let resolve = CaptureHandoffBuilder::camera_call()
            .require_permission_preflight(false)
            .require_source_picker(false)
            .build_checked()
            .unwrap();
        assert_eq!(
            resolve.next_action(),
            CaptureHandoffNextAction::ResolveCaptureConfigs
        );
        assert!(resolve.requires_consent(CaptureConsentKind::Camera));
        assert!(resolve.requires_consent(CaptureConsentKind::Microphone));

        let start = CaptureHandoffBuilder::new(CaptureConfigSetBuilder::camera_with_microphone())
            .require_permission_preflight(false)
            .auto_start(true)
            .build_checked()
            .unwrap();
        assert_eq!(
            start.next_action(),
            CaptureHandoffNextAction::StartCapturePipeline
        );
    }

    #[test]
    fn test_capture_handoff_resolves_configs_and_pipeline() {
        struct MockBackend;

        impl DeviceEnumerator for MockBackend {
            fn devices(&self, kind: CaptureDeviceKind) -> Result<Vec<CaptureDeviceInfo>> {
                let (id, name) = match kind {
                    CaptureDeviceKind::Screen => ("screen-main", "Main Display"),
                    CaptureDeviceKind::Window => ("window-editor", "Editor Window"),
                    CaptureDeviceKind::Microphone => ("mic-built-in", "Built-in Microphone"),
                    CaptureDeviceKind::Camera => ("camera-front", "Front Camera"),
                    CaptureDeviceKind::SystemAudio => ("system-audio", "System Audio"),
                };
                Ok(vec![CaptureDeviceInfo {
                    id: id.to_string(),
                    name: name.to_string(),
                    kind,
                    is_available: true,
                }])
            }
        }

        impl CaptureBackend for MockBackend {
            fn create_session(&self, _config: &CaptureConfig) -> Result<Box<dyn CaptureSession>> {
                struct MockSession {
                    state: CaptureSessionState,
                }

                impl CaptureSession for MockSession {
                    fn start(
                        &mut self,
                        _config: CaptureConfig,
                        _callback: FrameCallback,
                    ) -> Result<()> {
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
                        Ok(())
                    }

                    fn state(&self) -> CaptureSessionState {
                        self.state
                    }
                }

                Ok(Box::new(MockSession {
                    state: CaptureSessionState::Idle,
                }))
            }
        }

        let mut manager = CaptureManager::new();
        let backend = Arc::new(MockBackend);
        for kind in [
            CaptureDeviceKind::Camera,
            CaptureDeviceKind::Microphone,
            CaptureDeviceKind::Screen,
        ] {
            manager.register_backend(kind, backend.clone());
        }

        let handoff = CaptureHandoffBuilder::camera_call()
            .require_permission_preflight(false)
            .auto_start(true)
            .build_checked()
            .unwrap();
        let configs = handoff.resolve_configs(&manager).unwrap();
        assert_eq!(configs.len(), 2);
        assert_eq!(configs[0].kind, CaptureDeviceKind::Camera);
        assert_eq!(configs[1].kind, CaptureDeviceKind::Microphone);

        let pipeline = handoff
            .pipeline_checked(&manager, Arc::new(|_| {}))
            .unwrap();
        assert_eq!(pipeline.session_count(), 2);
        assert!(!pipeline.is_running());

        let started = handoff
            .start_pipeline_checked(&manager, Arc::new(|_| {}))
            .unwrap();
        assert_eq!(started.session_count(), 2);
        assert!(started.is_running());
    }

    #[test]
    fn test_capture_handoff_rejects_invalid_generated_shapes() {
        assert!(
            CaptureHandoffBuilder::new(CaptureConfigSetBuilder::new())
                .build_checked()
                .is_err()
        );
        assert!(
            CaptureHandoffBuilder::new(
                CaptureConfigSetBuilder::new()
                    .source(CaptureConfigBuilder::screen().frame_rate(f64::NAN))
            )
            .build_checked()
            .is_err()
        );
        assert!(
            CaptureHandoffBuilder::new(CaptureConfigSetBuilder::camera_with_microphone())
                .source_query(CaptureSourceQueryBuilder::cameras().name_contains(" "))
                .build_checked()
                .is_err()
        );
    }

    #[test]
    fn test_capture_manager_starts_checked_pipeline() {
        struct MockBackend;

        impl DeviceEnumerator for MockBackend {
            fn devices(&self, kind: CaptureDeviceKind) -> Result<Vec<CaptureDeviceInfo>> {
                let (id, name) = match kind {
                    CaptureDeviceKind::Screen => ("screen-main", "Main Display"),
                    CaptureDeviceKind::Window => ("window-editor", "Editor Window"),
                    CaptureDeviceKind::Microphone => ("mic-built-in", "Built-in Microphone"),
                    CaptureDeviceKind::Camera => ("camera-front", "Front Camera"),
                    CaptureDeviceKind::SystemAudio => ("system-audio", "System Audio"),
                };
                Ok(vec![CaptureDeviceInfo {
                    id: id.to_string(),
                    name: name.to_string(),
                    kind,
                    is_available: true,
                }])
            }
        }

        impl CaptureBackend for MockBackend {
            fn create_session(&self, _config: &CaptureConfig) -> Result<Box<dyn CaptureSession>> {
                struct MockSession {
                    state: CaptureSessionState,
                }

                impl CaptureSession for MockSession {
                    fn start(
                        &mut self,
                        _config: CaptureConfig,
                        _callback: FrameCallback,
                    ) -> Result<()> {
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
                        Ok(())
                    }

                    fn state(&self) -> CaptureSessionState {
                        self.state
                    }
                }

                Ok(Box::new(MockSession {
                    state: CaptureSessionState::Idle,
                }))
            }
        }

        let mut manager = CaptureManager::new();
        let backend = Arc::new(MockBackend);
        for kind in [
            CaptureDeviceKind::Screen,
            CaptureDeviceKind::Microphone,
            CaptureDeviceKind::Camera,
        ] {
            manager.register_backend(kind, backend.clone());
        }

        let pipeline = manager
            .pipeline_checked(
                CaptureConfigSetBuilder::camera_with_microphone(),
                Arc::new(|_| {}),
            )
            .unwrap();
        assert_eq!(pipeline.session_count(), 2);
        assert!(!pipeline.is_running());
        assert_eq!(
            pipeline.session_states(),
            vec![CaptureSessionState::Idle, CaptureSessionState::Idle]
        );

        let mut started = manager
            .start_pipeline_checked(
                CaptureConfigSetBuilder::screen_with_microphone()
                    .video_frame_rate(30.0)
                    .video_resolution(1280, 720),
                Arc::new(|_| {}),
            )
            .unwrap();
        assert_eq!(started.session_count(), 2);
        assert!(started.is_running());
        assert_eq!(
            started.session_states(),
            vec![CaptureSessionState::Running, CaptureSessionState::Running]
        );
        started.stop_all().unwrap();
        assert!(!started.is_running());
    }

    #[test]
    fn test_capture_pipeline_lifecycle() {
        let mut pipeline = CapturePipeline::new();
        assert!(!pipeline.is_running());
        assert_eq!(pipeline.session_count(), 0);

        struct MockSession {
            state: CaptureSessionState,
        }

        impl CaptureSession for MockSession {
            fn start(&mut self, _config: CaptureConfig, _callback: FrameCallback) -> Result<()> {
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
                Ok(())
            }

            fn state(&self) -> CaptureSessionState {
                self.state
            }
        }

        pipeline.add_session(
            Box::new(MockSession {
                state: CaptureSessionState::Idle,
            }),
            CaptureConfig {
                device_id: "screen-0".to_string(),
                kind: CaptureDeviceKind::Screen,
                frame_rate: Some(60.0),
                resolution: Some((1920, 1080)),
                include_audio: false,
            },
            Arc::new(|_| {}),
        );

        pipeline.start_all().unwrap();
        assert!(pipeline.is_running());

        pipeline.stop_all().unwrap();
        assert!(!pipeline.is_running());
    }

    #[test]
    fn test_capture_pipeline_restart_preserves_callback() {
        struct MockSession {
            state: CaptureSessionState,
        }

        impl CaptureSession for MockSession {
            fn start(&mut self, _config: CaptureConfig, callback: FrameCallback) -> Result<()> {
                self.state = CaptureSessionState::Running;
                callback(CaptureFrame::Audio {
                    channels: 2,
                    sample_rate: 48_000,
                    data: Arc::new(Vec::new()),
                    timestamp_ms: 0,
                });
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
                Ok(())
            }

            fn state(&self) -> CaptureSessionState {
                self.state
            }
        }

        let callback_count = Arc::new(AtomicUsize::new(0));
        let mut pipeline = CapturePipeline::new();
        pipeline.add_session(
            Box::new(MockSession {
                state: CaptureSessionState::Idle,
            }),
            CaptureConfig {
                device_id: "screen-1".to_string(),
                kind: CaptureDeviceKind::Screen,
                frame_rate: Some(60.0),
                resolution: Some((1920, 1080)),
                include_audio: false,
            },
            Arc::new({
                let callback_count = Arc::clone(&callback_count);
                move |_| {
                    callback_count.fetch_add(1, Ordering::Relaxed);
                }
            }),
        );

        pipeline.start_all().unwrap();
        pipeline.stop_all().unwrap();
        pipeline.start_all().unwrap();

        assert_eq!(callback_count.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn test_capture_pipeline_pause_and_resume() {
        struct MockSession {
            state: CaptureSessionState,
        }

        impl CaptureSession for MockSession {
            fn start(&mut self, _config: CaptureConfig, _callback: FrameCallback) -> Result<()> {
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
                Ok(())
            }

            fn state(&self) -> CaptureSessionState {
                self.state
            }
        }

        let mut pipeline = CapturePipeline::new();
        pipeline.add_session(
            Box::new(MockSession {
                state: CaptureSessionState::Idle,
            }),
            CaptureConfig {
                device_id: "screen-2".to_string(),
                kind: CaptureDeviceKind::Screen,
                frame_rate: Some(30.0),
                resolution: Some((1280, 720)),
                include_audio: false,
            },
            Arc::new(|_| {}),
        );

        pipeline.start_all().unwrap();
        pipeline.pause_all().unwrap();
        assert_eq!(pipeline.session_states(), vec![CaptureSessionState::Paused]);

        pipeline.resume_all().unwrap();
        assert_eq!(
            pipeline.session_states(),
            vec![CaptureSessionState::Running]
        );
    }

    #[test]
    fn test_capture_manager_uses_registered_backend() {
        struct MockBackend;

        impl DeviceEnumerator for MockBackend {
            fn devices(&self, kind: CaptureDeviceKind) -> Result<Vec<CaptureDeviceInfo>> {
                Ok(vec![CaptureDeviceInfo {
                    id: "device-1".to_string(),
                    name: format!("{:?} Device", kind),
                    kind,
                    is_available: true,
                }])
            }
        }

        impl CaptureBackend for MockBackend {
            fn create_session(&self, _config: &CaptureConfig) -> Result<Box<dyn CaptureSession>> {
                struct MockSession;

                impl CaptureSession for MockSession {
                    fn start(
                        &mut self,
                        _config: CaptureConfig,
                        _callback: FrameCallback,
                    ) -> Result<()> {
                        Ok(())
                    }

                    fn pause(&mut self) -> Result<()> {
                        Ok(())
                    }

                    fn resume(&mut self) -> Result<()> {
                        Ok(())
                    }

                    fn stop(&mut self) -> Result<()> {
                        Ok(())
                    }

                    fn state(&self) -> CaptureSessionState {
                        CaptureSessionState::Idle
                    }
                }

                Ok(Box::new(MockSession))
            }
        }

        let mut manager = CaptureManager::new();
        manager.register_backend(CaptureDeviceKind::Screen, Arc::new(MockBackend));

        let devices = manager.devices(CaptureDeviceKind::Screen).unwrap();
        assert_eq!(devices.len(), 1);

        let session = manager.create_session(&CaptureConfig {
            device_id: "device-1".to_string(),
            kind: CaptureDeviceKind::Screen,
            frame_rate: None,
            resolution: None,
            include_audio: false,
        });
        assert!(session.is_ok());
    }

    #[test]
    fn test_capture_sync() {
        let mut sync = CaptureSync::new(ClockSource::SystemMonotonic);
        sync.set_base_timestamp(1000);
        assert_eq!(sync.synchronize(1500), 500);
        assert_eq!(sync.synchronize(800), 0);
    }

    #[test]
    fn test_pixel_format_equality() {
        assert_eq!(PixelFormat::Bgra32, PixelFormat::Bgra32);
        assert_ne!(PixelFormat::Bgra32, PixelFormat::Rgba32);
    }

    #[test]
    fn test_mock_capture_backend_devices() {
        struct MockBackend {
            devices: Vec<CaptureDeviceInfo>,
        }

        impl DeviceEnumerator for MockBackend {
            fn devices(&self, _kind: CaptureDeviceKind) -> Result<Vec<CaptureDeviceInfo>> {
                Ok(self.devices.clone())
            }
        }

        impl CaptureBackend for MockBackend {
            fn create_session(&self, _config: &CaptureConfig) -> Result<Box<dyn CaptureSession>> {
                struct MockSession;
                impl CaptureSession for MockSession {
                    fn start(
                        &mut self,
                        _config: CaptureConfig,
                        _callback: FrameCallback,
                    ) -> Result<()> {
                        Ok(())
                    }
                    fn pause(&mut self) -> Result<()> {
                        Ok(())
                    }
                    fn resume(&mut self) -> Result<()> {
                        Ok(())
                    }
                    fn stop(&mut self) -> Result<()> {
                        Ok(())
                    }
                    fn state(&self) -> CaptureSessionState {
                        CaptureSessionState::Idle
                    }
                }
                Ok(Box::new(MockSession))
            }
        }

        let backend = MockBackend {
            devices: vec![
                CaptureDeviceInfo {
                    id: "mic-0".into(),
                    name: "Built-in Microphone".into(),
                    kind: CaptureDeviceKind::Microphone,
                    is_available: true,
                },
                CaptureDeviceInfo {
                    id: "mic-1".into(),
                    name: "USB Microphone".into(),
                    kind: CaptureDeviceKind::Microphone,
                    is_available: true,
                },
            ],
        };

        let devices = backend.devices(CaptureDeviceKind::Microphone).unwrap();
        assert_eq!(devices.len(), 2);
        assert_eq!(devices[0].id, "mic-0");
        assert_eq!(devices[1].id, "mic-1");
    }

    #[test]
    fn test_pipeline_dropped_frame_accounting() {
        struct SlowMockSession {
            state: CaptureSessionState,
            dropped: AtomicU64,
        }

        impl CaptureSession for SlowMockSession {
            fn start(&mut self, _config: CaptureConfig, callback: FrameCallback) -> Result<()> {
                self.state = CaptureSessionState::Running;
                for i in 0..3 {
                    callback(CaptureFrame::Audio {
                        channels: 2,
                        sample_rate: 48_000,
                        data: Arc::new(vec![i]),
                        timestamp_ms: i as u64,
                    });
                }
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
                Ok(())
            }

            fn state(&self) -> CaptureSessionState {
                self.state
            }

            fn dropped_frame_count(&self) -> u64 {
                self.dropped.load(Ordering::Relaxed)
            }
        }

        let mut pipeline = CapturePipeline::new();
        let received = Arc::new(AtomicUsize::new(0));
        pipeline.add_session(
            Box::new(SlowMockSession {
                state: CaptureSessionState::Idle,
                dropped: AtomicU64::new(0),
            }),
            CaptureConfig {
                device_id: "audio-0".to_string(),
                kind: CaptureDeviceKind::SystemAudio,
                frame_rate: None,
                resolution: None,
                include_audio: false,
            },
            Arc::new({
                let received = Arc::clone(&received);
                move |_| {
                    std::thread::sleep(std::time::Duration::from_millis(10));
                    received.fetch_add(1, Ordering::Relaxed);
                }
            }),
        );

        pipeline.start_all().unwrap();
        pipeline.stop_all().unwrap();

        let received_count = received.load(Ordering::Relaxed);
        let dropped = pipeline.total_dropped_frames();
        assert_eq!(received_count + dropped as usize, 3);
    }

    #[test]
    fn test_pipeline_session_count() {
        struct MockSession;
        impl CaptureSession for MockSession {
            fn start(&mut self, _config: CaptureConfig, _callback: FrameCallback) -> Result<()> {
                Ok(())
            }
            fn pause(&mut self) -> Result<()> {
                Ok(())
            }
            fn resume(&mut self) -> Result<()> {
                Ok(())
            }
            fn stop(&mut self) -> Result<()> {
                Ok(())
            }
            fn state(&self) -> CaptureSessionState {
                CaptureSessionState::Idle
            }
        }

        let mut pipeline = CapturePipeline::new();
        assert_eq!(pipeline.session_count(), 0);
        pipeline.add_session(
            Box::new(MockSession),
            CaptureConfig::new("d1", CaptureDeviceKind::Screen),
            Arc::new(|_| {}),
        );
        assert_eq!(pipeline.session_count(), 1);
        pipeline.add_session(
            Box::new(MockSession),
            CaptureConfig::new("d2", CaptureDeviceKind::Microphone),
            Arc::new(|_| {}),
        );
        assert_eq!(pipeline.session_count(), 2);
    }
}
