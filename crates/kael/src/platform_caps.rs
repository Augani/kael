use serde::{Deserialize, Serialize};

/// A platform feature that may or may not be available.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PlatformFeature {
    /// Local notification delivery.
    Notifications,
    /// Notification action buttons.
    NotificationActions,
    /// Push notification registration.
    PushNotifications,
    /// Share sheet / share receiver.
    ShareSheet,
    /// Screen capture.
    ScreenCapture,
    /// Microphone capture.
    MicrophoneCapture,
    /// System audio loopback capture.
    SystemAudioLoopback,
    /// Global hotkeys outside the app.
    GlobalHotkeys,
    /// System tray / status bar item.
    StatusBarItem,
    /// Native print dialog.
    Printing,
    /// Taskbar / dock progress indicator.
    ProgressIndicator,
    /// Biometric authentication (Touch ID, Windows Hello).
    Biometrics,
    /// Secure keychain / credential storage.
    SecureKeychain,
    /// File bookmark / scoped access tokens.
    FileBookmarks,
    /// WebView embedding.
    WebView,
    /// Hardware-accelerated GPU rendering.
    GpuRendering,
    /// Spatial audio.
    SpatialAudio,
    /// App activation / hide semantics.
    AppActivation,
    /// Auxiliary executable lookup.
    AuxiliaryExecutable,
    /// Auto-update mechanism.
    AutoUpdate,
    /// Hardened runtime (macOS).
    HardenedRuntime,
    /// Sandboxed execution.
    Sandboxing,
}

/// The level of support for a platform feature.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SupportLevel {
    /// Fully supported on this platform.
    Full,
    /// Partially supported with limitations.
    Partial,
    /// Not supported on this platform.
    Unsupported,
    /// Requires explicit runtime initialization.
    RequiresInit,
    /// Available but explicitly disabled by policy.
    Disabled,
}

/// A capability report entry for a single feature.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeatureReport {
    /// The feature being reported on.
    pub feature: PlatformFeature,
    /// The level of support.
    pub support: SupportLevel,
    /// An optional human-readable note about limitations.
    pub note: Option<String>,
}

/// A full platform capability report for runtime queries.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CapabilityReport {
    /// The OS name.
    pub os: String,
    /// The OS version.
    pub os_version: String,
    /// Feature reports.
    pub features: Vec<FeatureReport>,
}

impl CapabilityReport {
    /// Generate a report for the current platform.
    pub fn current() -> Self {
        let mut report = Self {
            os: std::env::consts::OS.to_string(),
            os_version: String::new(),
            features: Vec::new(),
        };
        report.populate_features();
        report
    }

    /// Check the support level for a specific feature.
    pub fn support_for(&self, feature: PlatformFeature) -> SupportLevel {
        self.features
            .iter()
            .find(|f| f.feature == feature)
            .map(|f| f.support)
            .unwrap_or(SupportLevel::Unsupported)
    }

    /// Check if a feature is fully supported.
    pub fn is_supported(&self, feature: PlatformFeature) -> bool {
        matches!(self.support_for(feature), SupportLevel::Full)
    }

    /// Returns all features at a given support level.
    pub fn features_at_level(&self, level: SupportLevel) -> Vec<PlatformFeature> {
        self.features
            .iter()
            .filter(|f| f.support == level)
            .map(|f| f.feature)
            .collect()
    }

    /// Returns all unsupported features.
    pub fn unsupported_features(&self) -> Vec<PlatformFeature> {
        self.features_at_level(SupportLevel::Unsupported)
    }

    /// Serialize the report to JSON.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    fn populate_features(&mut self) {
        #[cfg(target_os = "macos")]
        self.populate_macos();
        #[cfg(target_os = "windows")]
        self.populate_windows();
        #[cfg(target_os = "linux")]
        self.populate_linux();
    }

    #[cfg(target_os = "macos")]
    fn populate_macos(&mut self) {
        self.add(PlatformFeature::Notifications, SupportLevel::Full, None);
        self.add(
            PlatformFeature::NotificationActions,
            SupportLevel::Full,
            None,
        );
        self.add(PlatformFeature::PushNotifications, SupportLevel::Full, None);
        self.add(PlatformFeature::ShareSheet, SupportLevel::Full, None);
        self.add(
            PlatformFeature::ScreenCapture,
            SupportLevel::RequiresInit,
            Some("Requires screen recording permission"),
        );
        self.add(
            PlatformFeature::MicrophoneCapture,
            SupportLevel::RequiresInit,
            Some("Requires microphone permission"),
        );
        self.add(
            PlatformFeature::SystemAudioLoopback,
            SupportLevel::Partial,
            Some("Requires ScreenCaptureKit on macOS 13+"),
        );
        self.add(PlatformFeature::GlobalHotkeys, SupportLevel::Full, None);
        self.add(PlatformFeature::StatusBarItem, SupportLevel::Full, None);
        self.add(PlatformFeature::Printing, SupportLevel::Full, None);
        self.add(PlatformFeature::ProgressIndicator, SupportLevel::Full, None);
        self.add(PlatformFeature::Biometrics, SupportLevel::Full, None);
        self.add(PlatformFeature::SecureKeychain, SupportLevel::Full, None);
        self.add(PlatformFeature::FileBookmarks, SupportLevel::Full, None);
        self.add(PlatformFeature::WebView, SupportLevel::Full, None);
        self.add(PlatformFeature::GpuRendering, SupportLevel::Full, None);
        self.add(PlatformFeature::SpatialAudio, SupportLevel::Full, None);
        self.add(PlatformFeature::AppActivation, SupportLevel::Full, None);
        self.add(
            PlatformFeature::AuxiliaryExecutable,
            SupportLevel::Full,
            None,
        );
        self.add(PlatformFeature::AutoUpdate, SupportLevel::Full, None);
        self.add(PlatformFeature::HardenedRuntime, SupportLevel::Full, None);
        self.add(PlatformFeature::Sandboxing, SupportLevel::Full, None);
    }

    #[cfg(target_os = "windows")]
    fn populate_windows(&mut self) {
        self.add(PlatformFeature::Notifications, SupportLevel::Full, None);
        self.add(
            PlatformFeature::NotificationActions,
            SupportLevel::Partial,
            Some("Requires Windows 10+"),
        );
        self.add(
            PlatformFeature::PushNotifications,
            SupportLevel::Partial,
            Some("WNS integration required"),
        );
        self.add(
            PlatformFeature::ShareSheet,
            SupportLevel::Partial,
            Some("Requires UWP share contract"),
        );
        self.add(
            PlatformFeature::ScreenCapture,
            SupportLevel::RequiresInit,
            Some("Requires Desktop Duplication API init"),
        );
        self.add(
            PlatformFeature::MicrophoneCapture,
            SupportLevel::RequiresInit,
            Some("Requires WASAPI init"),
        );
        self.add(
            PlatformFeature::SystemAudioLoopback,
            SupportLevel::Full,
            None,
        );
        self.add(PlatformFeature::GlobalHotkeys, SupportLevel::Full, None);
        self.add(PlatformFeature::StatusBarItem, SupportLevel::Full, None);
        self.add(PlatformFeature::Printing, SupportLevel::Full, None);
        self.add(PlatformFeature::ProgressIndicator, SupportLevel::Full, None);
        self.add(
            PlatformFeature::Biometrics,
            SupportLevel::Partial,
            Some("Windows Hello required"),
        );
        self.add(PlatformFeature::SecureKeychain, SupportLevel::Full, None);
        self.add(
            PlatformFeature::FileBookmarks,
            SupportLevel::Unsupported,
            None,
        );
        self.add(PlatformFeature::WebView, SupportLevel::Full, None);
        self.add(PlatformFeature::GpuRendering, SupportLevel::Full, None);
        self.add(
            PlatformFeature::SpatialAudio,
            SupportLevel::Partial,
            Some("Requires Windows Sonic"),
        );
        self.add(PlatformFeature::AppActivation, SupportLevel::Full, None);
        self.add(
            PlatformFeature::AuxiliaryExecutable,
            SupportLevel::Full,
            None,
        );
        self.add(PlatformFeature::AutoUpdate, SupportLevel::Full, None);
        self.add(
            PlatformFeature::HardenedRuntime,
            SupportLevel::Unsupported,
            None,
        );
        self.add(
            PlatformFeature::Sandboxing,
            SupportLevel::Partial,
            Some("MSIX sandbox only"),
        );
    }

    #[cfg(target_os = "linux")]
    fn populate_linux(&mut self) {
        self.add(PlatformFeature::Notifications, SupportLevel::Full, None);
        self.add(
            PlatformFeature::NotificationActions,
            SupportLevel::Partial,
            Some("Depends on notification daemon"),
        );
        self.add(
            PlatformFeature::PushNotifications,
            SupportLevel::Unsupported,
            None,
        );
        self.add(
            PlatformFeature::ShareSheet,
            SupportLevel::Partial,
            Some("Portal-based sharing"),
        );
        self.add(
            PlatformFeature::ScreenCapture,
            SupportLevel::RequiresInit,
            Some("PipeWire or X11 required"),
        );
        self.add(
            PlatformFeature::MicrophoneCapture,
            SupportLevel::RequiresInit,
            Some("PulseAudio/PipeWire required"),
        );
        self.add(
            PlatformFeature::SystemAudioLoopback,
            SupportLevel::Partial,
            Some("PulseAudio monitor source"),
        );
        self.add(
            PlatformFeature::GlobalHotkeys,
            SupportLevel::Partial,
            Some(
                "X11: direct key grab. Wayland: via the GlobalShortcuts desktop portal \
                 (xdg-desktop-portal with a backend implementing GlobalShortcuts v1+); \
                 binding is interactive and may prompt for consent, the compositor may \
                 assign a different trigger than requested, and registration resolves \
                 asynchronously. Returns a descriptive error if the portal is unavailable.",
            ),
        );
        self.add(
            PlatformFeature::StatusBarItem,
            SupportLevel::Partial,
            Some("Requires libappindicator or SNI"),
        );
        self.add(PlatformFeature::Printing, SupportLevel::Full, None);
        self.add(
            PlatformFeature::ProgressIndicator,
            SupportLevel::Partial,
            Some("Unity launcher API"),
        );
        self.add(PlatformFeature::Biometrics, SupportLevel::Unsupported, None);
        self.add(
            PlatformFeature::SecureKeychain,
            SupportLevel::Partial,
            Some("libsecret/GNOME Keyring"),
        );
        self.add(
            PlatformFeature::FileBookmarks,
            SupportLevel::Unsupported,
            None,
        );
        self.add(PlatformFeature::WebView, SupportLevel::Full, None);
        self.add(PlatformFeature::GpuRendering, SupportLevel::Full, None);
        self.add(
            PlatformFeature::SpatialAudio,
            SupportLevel::Unsupported,
            None,
        );
        self.add(
            PlatformFeature::AppActivation,
            SupportLevel::Partial,
            Some("Limited on Wayland"),
        );
        self.add(
            PlatformFeature::AuxiliaryExecutable,
            SupportLevel::Partial,
            Some("No standard bundle location"),
        );
        self.add(
            PlatformFeature::AutoUpdate,
            SupportLevel::Partial,
            Some("AppImage only"),
        );
        self.add(
            PlatformFeature::HardenedRuntime,
            SupportLevel::Unsupported,
            None,
        );
        self.add(
            PlatformFeature::Sandboxing,
            SupportLevel::Partial,
            Some("Flatpak/Snap sandbox"),
        );
    }

    fn add(&mut self, feature: PlatformFeature, support: SupportLevel, note: Option<&str>) {
        self.features.push(FeatureReport {
            feature,
            support,
            note: note.map(|s| s.to_string()),
        });
    }
}

/// Audit log entry for sensitive operations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEntry {
    /// Timestamp in milliseconds since epoch.
    pub timestamp_ms: u64,
    /// The operation that was performed.
    pub operation: String,
    /// The process or component that performed it.
    pub source: String,
    /// Whether the operation was granted.
    pub granted: bool,
    /// Additional context.
    pub details: Option<String>,
}

/// An append-only audit log for sensitive operations.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AuditLog {
    entries: Vec<AuditEntry>,
    max_entries: usize,
}

impl AuditLog {
    /// Create a new audit log with the given capacity.
    pub fn new(max_entries: usize) -> Self {
        Self {
            entries: Vec::new(),
            max_entries,
        }
    }

    /// Record an audit entry.
    pub fn record(&mut self, entry: AuditEntry) {
        if self.entries.len() >= self.max_entries {
            self.entries.remove(0);
        }
        self.entries.push(entry);
    }

    /// Return all entries.
    pub fn entries(&self) -> &[AuditEntry] {
        &self.entries
    }

    /// Return entries for a specific source.
    pub fn entries_for_source(&self, source: &str) -> Vec<&AuditEntry> {
        self.entries.iter().filter(|e| e.source == source).collect()
    }

    /// Return entries for denied operations.
    pub fn denied_entries(&self) -> Vec<&AuditEntry> {
        self.entries.iter().filter(|e| !e.granted).collect()
    }

    /// Total number of entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the log is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Clear all entries.
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// Export the log to JSON.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(&self.entries)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_capability_report_current() {
        let report = CapabilityReport::current();
        assert!(!report.os.is_empty());
        assert!(!report.features.is_empty());
    }

    #[test]
    fn test_support_for_feature() {
        let report = CapabilityReport::current();
        let support = report.support_for(PlatformFeature::GpuRendering);
        assert_eq!(support, SupportLevel::Full);
    }

    #[test]
    fn test_is_supported() {
        let report = CapabilityReport::current();
        assert!(report.is_supported(PlatformFeature::Notifications));
    }

    #[test]
    fn test_unsupported_features() {
        let report = CapabilityReport::current();
        let unsupported = report.unsupported_features();
        for feature in &unsupported {
            assert!(!report.is_supported(*feature));
        }
    }

    #[test]
    fn test_report_serialization() {
        let report = CapabilityReport::current();
        let json = report.to_json().unwrap();
        let decoded: CapabilityReport = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.os, report.os);
        assert_eq!(decoded.features.len(), report.features.len());
    }

    #[test]
    fn test_features_at_level() {
        let report = CapabilityReport::current();
        let full = report.features_at_level(SupportLevel::Full);
        assert!(!full.is_empty());
    }

    #[test]
    fn test_audit_log() {
        let mut log = AuditLog::new(100);
        assert!(log.is_empty());

        log.record(AuditEntry {
            timestamp_ms: 1000,
            operation: "file_read".to_string(),
            source: "extension-a".to_string(),
            granted: true,
            details: None,
        });
        log.record(AuditEntry {
            timestamp_ms: 2000,
            operation: "network_access".to_string(),
            source: "extension-b".to_string(),
            granted: false,
            details: Some("host not allowed".to_string()),
        });

        assert_eq!(log.len(), 2);
        assert_eq!(log.entries_for_source("extension-a").len(), 1);
        assert_eq!(log.denied_entries().len(), 1);
    }

    #[test]
    fn test_audit_log_capacity() {
        let mut log = AuditLog::new(3);
        for i in 0..5 {
            log.record(AuditEntry {
                timestamp_ms: i,
                operation: format!("op_{}", i),
                source: "test".to_string(),
                granted: true,
                details: None,
            });
        }
        assert_eq!(log.len(), 3);
        assert_eq!(log.entries()[0].timestamp_ms, 2);
    }

    #[test]
    fn test_audit_log_serialization() {
        let mut log = AuditLog::new(10);
        log.record(AuditEntry {
            timestamp_ms: 1000,
            operation: "test".to_string(),
            source: "src".to_string(),
            granted: true,
            details: None,
        });
        let json = log.to_json().unwrap();
        let entries: Vec<AuditEntry> = serde_json::from_str(&json).unwrap();
        assert_eq!(entries.len(), 1);
    }
}
