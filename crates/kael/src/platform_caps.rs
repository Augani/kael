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
    /// Native outbound file drags / promised files.
    FileExportDrag,
    /// Native app-window visual capture / screenshot requests.
    AppWindowCapture,
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
    /// Always-on-top / overlay windows that render above other windows,
    /// including fullscreen surfaces.
    AlwaysOnTopWindows,
    /// Native window tabbing (grouping windows into a single tabbed window,
    /// tab bar, tab overview, merge-all-windows). A macOS AppKit feature.
    WindowTabbing,
    /// High-precision pointer devices such as mice and trackpads.
    PrecisionPointerInput,
    /// Direct touch input streams from touchscreens.
    TouchInput,
    /// Pen/stylus input streams with tablet-specific metadata.
    PenInput,
    /// Native gesture events such as magnify/pinch, momentum scroll, and swipe.
    GestureInput,
    /// Native or bundled text spellchecking dictionaries.
    SpellChecking,
    /// Native geolocation/location services.
    Geolocation,
    /// USB device discovery and access.
    UsbDevices,
    /// HID device discovery and access.
    HidDevices,
    /// Serial port discovery and access.
    SerialPorts,
    /// Bluetooth device/service discovery and access.
    BluetoothDevices,
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

impl SupportLevel {
    /// Returns true when the feature can be used without a fallback.
    pub fn is_full(self) -> bool {
        matches!(self, Self::Full)
    }

    /// Returns true when the feature is available but callers should handle
    /// setup, platform caveats, or policy conditions.
    pub fn is_available(self) -> bool {
        matches!(self, Self::Full | Self::Partial | Self::RequiresInit)
    }
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

impl FeatureReport {
    /// Returns true when this feature can be used without a fallback.
    pub fn is_full(&self) -> bool {
        self.support.is_full()
    }

    /// Returns true when this feature is available in some usable form.
    pub fn is_available(&self) -> bool {
        self.support.is_available()
    }
}

/// A required or preferred platform feature for a capability check.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityRequirement {
    /// The feature being checked.
    pub feature: PlatformFeature,
    /// Whether partial or initialization-gated support is acceptable.
    pub allow_partial: bool,
    /// Whether failing this requirement should block the app path.
    pub required: bool,
}

impl CapabilityRequirement {
    /// Require full support for a feature.
    pub fn required(feature: PlatformFeature) -> Self {
        Self {
            feature,
            allow_partial: false,
            required: true,
        }
    }

    /// Require support for a feature, accepting Partial or RequiresInit.
    pub fn required_available(feature: PlatformFeature) -> Self {
        Self {
            feature,
            allow_partial: true,
            required: true,
        }
    }

    /// Prefer full support for a feature without blocking the app path.
    pub fn preferred(feature: PlatformFeature) -> Self {
        Self {
            feature,
            allow_partial: false,
            required: false,
        }
    }

    /// Prefer support for a feature, accepting Partial or RequiresInit.
    pub fn preferred_available(feature: PlatformFeature) -> Self {
        Self {
            feature,
            allow_partial: true,
            required: false,
        }
    }

    /// Returns true when the given support level satisfies this requirement.
    pub fn is_satisfied_by(&self, support: SupportLevel) -> bool {
        if self.allow_partial {
            support.is_available()
        } else {
            support.is_full()
        }
    }
}

/// Result for one entry in a platform capability check.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityCheckItem {
    /// The feature that was checked.
    pub feature: PlatformFeature,
    /// The current support level.
    pub support: SupportLevel,
    /// Whether Partial or RequiresInit was acceptable for this check.
    pub allow_partial: bool,
    /// Whether this feature was required.
    pub required: bool,
    /// Whether the requirement was satisfied.
    pub satisfied: bool,
    /// Platform note copied from the capability report, when available.
    pub note: Option<String>,
}

/// Aggregate result for a platform capability check.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CapabilityCheckResult {
    /// Results for required features.
    pub required: Vec<CapabilityCheckItem>,
    /// Results for preferred features.
    pub preferred: Vec<CapabilityCheckItem>,
}

impl CapabilityCheckResult {
    /// Returns true when every required feature passed.
    pub fn is_ok(&self) -> bool {
        self.missing_required().is_empty()
    }

    /// Required features that did not satisfy the check.
    pub fn missing_required(&self) -> Vec<&CapabilityCheckItem> {
        self.required
            .iter()
            .filter(|item| !item.satisfied)
            .collect()
    }

    /// Preferred features that did not satisfy the check.
    pub fn missing_preferred(&self) -> Vec<&CapabilityCheckItem> {
        self.preferred
            .iter()
            .filter(|item| !item.satisfied)
            .collect()
    }

    /// Human-readable required-feature failure summary for logs or UI.
    pub fn required_failure_summary(&self) -> Option<String> {
        let missing = self.missing_required();
        if missing.is_empty() {
            return None;
        }

        Some(
            missing
                .into_iter()
                .map(format_check_item)
                .collect::<Vec<_>>()
                .join("; "),
        )
    }
}

/// Builder for checking a group of platform capabilities.
#[derive(Debug, Clone, Default)]
pub struct CapabilityCheck {
    requirements: Vec<CapabilityRequirement>,
}

impl CapabilityCheck {
    /// Create an empty capability check.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a required feature that must have full support.
    pub fn require(mut self, feature: PlatformFeature) -> Self {
        self.requirements
            .push(CapabilityRequirement::required(feature));
        self
    }

    /// Add a required feature where Partial or RequiresInit support is usable.
    pub fn require_available(mut self, feature: PlatformFeature) -> Self {
        self.requirements
            .push(CapabilityRequirement::required_available(feature));
        self
    }

    /// Add a preferred feature that must have full support to pass.
    pub fn prefer(mut self, feature: PlatformFeature) -> Self {
        self.requirements
            .push(CapabilityRequirement::preferred(feature));
        self
    }

    /// Add a preferred feature where Partial or RequiresInit support is usable.
    pub fn prefer_available(mut self, feature: PlatformFeature) -> Self {
        self.requirements
            .push(CapabilityRequirement::preferred_available(feature));
        self
    }

    /// Evaluate these requirements against a report.
    pub fn evaluate(&self, report: &CapabilityReport) -> CapabilityCheckResult {
        report.evaluate(self.requirements.iter().copied())
    }
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
        self.feature_report(feature)
            .map(|report| report.support)
            .unwrap_or(SupportLevel::Unsupported)
    }

    /// Returns the full report entry for a specific feature.
    pub fn feature_report(&self, feature: PlatformFeature) -> Option<&FeatureReport> {
        self.features.iter().find(|f| f.feature == feature)
    }

    /// Check if a feature is fully supported.
    pub fn is_supported(&self, feature: PlatformFeature) -> bool {
        matches!(self.support_for(feature), SupportLevel::Full)
    }

    /// Check if a feature is available in any usable form.
    pub fn is_available(&self, feature: PlatformFeature) -> bool {
        self.support_for(feature).is_available()
    }

    /// Require full support for one feature, returning a readable error if missing.
    pub fn require(&self, feature: PlatformFeature) -> Result<(), String> {
        self.requirement(CapabilityRequirement::required(feature))
    }

    /// Require usable support for one feature, accepting Partial or RequiresInit.
    pub fn require_available(&self, feature: PlatformFeature) -> Result<(), String> {
        self.requirement(CapabilityRequirement::required_available(feature))
    }

    /// Require full support for every feature.
    pub fn require_all(
        &self,
        features: impl IntoIterator<Item = PlatformFeature>,
    ) -> Result<(), String> {
        self.evaluate(features.into_iter().map(CapabilityRequirement::required))
            .required_failure_summary()
            .map_or(Ok(()), Err)
    }

    /// Evaluate a list of required or preferred feature checks.
    pub fn evaluate(
        &self,
        requirements: impl IntoIterator<Item = CapabilityRequirement>,
    ) -> CapabilityCheckResult {
        let mut result = CapabilityCheckResult::default();

        for requirement in requirements {
            let report = self.feature_report(requirement.feature);
            let support = report.map_or(SupportLevel::Unsupported, |report| report.support);
            let item = CapabilityCheckItem {
                feature: requirement.feature,
                support,
                allow_partial: requirement.allow_partial,
                required: requirement.required,
                satisfied: requirement.is_satisfied_by(support),
                note: report.and_then(|report| report.note.clone()),
            };

            if requirement.required {
                result.required.push(item);
            } else {
                result.preferred.push(item);
            }
        }

        result
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

    fn requirement(&self, requirement: CapabilityRequirement) -> Result<(), String> {
        let result = self.evaluate([requirement]);
        result.required_failure_summary().map_or(Ok(()), Err)
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
        self.add(
            PlatformFeature::FileExportDrag,
            SupportLevel::Partial,
            Some("Checked outbound file drag descriptors are available; native file-promise session backend is not exposed yet"),
        );
        self.add(
            PlatformFeature::AppWindowCapture,
            SupportLevel::Partial,
            Some("Checked app-window capture descriptors are available; native snapshot backend is not exposed yet"),
        );
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
        self.add(
            PlatformFeature::AlwaysOnTopWindows,
            SupportLevel::Full,
            Some("NSWindow screen-saver/floating window levels"),
        );
        self.add(PlatformFeature::WindowTabbing, SupportLevel::Full, None);
        self.add(
            PlatformFeature::PrecisionPointerInput,
            SupportLevel::Full,
            Some("Mouse, trackpad, precise scroll, and magnify gesture events"),
        );
        self.add(
            PlatformFeature::TouchInput,
            SupportLevel::Unsupported,
            Some("macOS desktop has no direct touchscreen event stream in Kael"),
        );
        self.add(
            PlatformFeature::PenInput,
            SupportLevel::Partial,
            Some("Tablet devices may appear as pointer input; pressure/tilt streams are not exposed yet"),
        );
        self.add(
            PlatformFeature::GestureInput,
            SupportLevel::Partial,
            Some("Scroll phases, momentum, and magnify gestures are exposed; raw multi-touch streams are not"),
        );
        self.add(
            PlatformFeature::SpellChecking,
            SupportLevel::Partial,
            Some("NSSpellChecker-backed integration is planned; checked text-checking requests are available now"),
        );
        self.add(
            PlatformFeature::Geolocation,
            SupportLevel::RequiresInit,
            Some("Requires CoreLocation permission and usage-description metadata"),
        );
        self.add(
            PlatformFeature::UsbDevices,
            SupportLevel::Partial,
            Some("Checked request descriptors are available; native discovery/IO backend is not exposed yet"),
        );
        self.add(
            PlatformFeature::HidDevices,
            SupportLevel::Partial,
            Some("Checked request descriptors are available; IOKit HID backend is not exposed yet"),
        );
        self.add(
            PlatformFeature::SerialPorts,
            SupportLevel::Partial,
            Some("Checked request descriptors are available; native serial backend is not exposed yet"),
        );
        self.add(
            PlatformFeature::BluetoothDevices,
            SupportLevel::RequiresInit,
            Some("Requires Bluetooth usage-description metadata and user consent"),
        );
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
        self.add(
            PlatformFeature::FileExportDrag,
            SupportLevel::Partial,
            Some("Checked outbound file drag descriptors are available; OLE drag source backend is not exposed yet"),
        );
        self.add(
            PlatformFeature::AppWindowCapture,
            SupportLevel::Partial,
            Some("Checked app-window capture descriptors are available; HWND snapshot backend is not exposed yet"),
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
        self.add(
            PlatformFeature::AlwaysOnTopWindows,
            SupportLevel::Full,
            Some("HWND_TOPMOST z-order"),
        );
        self.add(
            PlatformFeature::WindowTabbing,
            SupportLevel::Unsupported,
            Some("No native window tabbing; tabbing_identifier is ignored"),
        );
        self.add(
            PlatformFeature::PrecisionPointerInput,
            SupportLevel::Full,
            Some("Mouse, touchpad, and high-resolution wheel input"),
        );
        self.add(
            PlatformFeature::TouchInput,
            SupportLevel::Partial,
            Some("Windows supports touch, but Kael does not expose raw touch contacts yet"),
        );
        self.add(
            PlatformFeature::PenInput,
            SupportLevel::Partial,
            Some("Windows Pointer/Ink APIs exist, but Kael does not expose pressure/tilt streams yet"),
        );
        self.add(
            PlatformFeature::GestureInput,
            SupportLevel::Partial,
            Some(
                "Pointer-derived scrolling and zoom gestures are available; raw multi-touch is not",
            ),
        );
        self.add(
            PlatformFeature::SpellChecking,
            SupportLevel::Partial,
            Some("Windows spellchecking APIs are available, but Kael currently exposes checked request descriptors only"),
        );
        self.add(
            PlatformFeature::Geolocation,
            SupportLevel::RequiresInit,
            Some("Requires Windows location services and user consent"),
        );
        self.add(
            PlatformFeature::UsbDevices,
            SupportLevel::Partial,
            Some("Checked request descriptors are available; WinUSB backend is not exposed yet"),
        );
        self.add(
            PlatformFeature::HidDevices,
            SupportLevel::Partial,
            Some(
                "Checked request descriptors are available; Windows HID backend is not exposed yet",
            ),
        );
        self.add(
            PlatformFeature::SerialPorts,
            SupportLevel::Partial,
            Some("Checked request descriptors are available; native serial backend is not exposed yet"),
        );
        self.add(
            PlatformFeature::BluetoothDevices,
            SupportLevel::Partial,
            Some("Checked request descriptors are available; Windows Bluetooth backend is not exposed yet"),
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
        self.add(
            PlatformFeature::FileExportDrag,
            SupportLevel::Partial,
            Some("Checked outbound file drag descriptors are available; X11/Wayland drag source backend is not exposed yet"),
        );
        self.add(
            PlatformFeature::AppWindowCapture,
            SupportLevel::Partial,
            Some("Checked app-window capture descriptors are available; compositor snapshot backend is not exposed yet"),
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
        self.add(
            PlatformFeature::AlwaysOnTopWindows,
            SupportLevel::Partial,
            Some(
                "X11: _NET_WM_STATE_ABOVE. Wayland: overlay windows use the wlr-layer-shell \
                 protocol (overlay layer) when the compositor implements it (wlroots-based \
                 compositors, KDE Plasma); compositors without it (e.g. GNOME/Mutter) fall \
                 back to a regular window with no always-on-top guarantee.",
            ),
        );
        self.add(
            PlatformFeature::WindowTabbing,
            SupportLevel::Unsupported,
            Some("No native window tabbing; tabbing_identifier is ignored"),
        );
        self.add(
            PlatformFeature::PrecisionPointerInput,
            SupportLevel::Full,
            Some("Mouse, touchpad, and wheel input through X11/Wayland backends"),
        );
        self.add(
            PlatformFeature::TouchInput,
            SupportLevel::Partial,
            Some(
                "Compositors may expose touch, but Kael currently routes pointer-style input only",
            ),
        );
        self.add(
            PlatformFeature::PenInput,
            SupportLevel::Partial,
            Some("Tablet devices depend on compositor/libinput support; pressure/tilt streams are not exposed yet"),
        );
        self.add(
            PlatformFeature::GestureInput,
            SupportLevel::Partial,
            Some("Scroll and pointer-derived gestures are available; raw multi-touch varies by compositor"),
        );
        self.add(
            PlatformFeature::SpellChecking,
            SupportLevel::Partial,
            Some("Depends on installed dictionaries; Kael currently exposes checked request descriptors only"),
        );
        self.add(
            PlatformFeature::Geolocation,
            SupportLevel::Partial,
            Some("Portal/geoclue support varies by desktop environment and sandbox"),
        );
        self.add(
            PlatformFeature::UsbDevices,
            SupportLevel::Partial,
            Some(
                "Checked request descriptors are available; udev/libusb backend is not exposed yet",
            ),
        );
        self.add(
            PlatformFeature::HidDevices,
            SupportLevel::Partial,
            Some("Checked request descriptors are available; hidraw/libudev backend is not exposed yet"),
        );
        self.add(
            PlatformFeature::SerialPorts,
            SupportLevel::Partial,
            Some("Checked request descriptors are available; native serial backend is not exposed yet"),
        );
        self.add(
            PlatformFeature::BluetoothDevices,
            SupportLevel::Partial,
            Some("Checked request descriptors are available; BlueZ backend is not exposed yet"),
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

fn format_check_item(item: &CapabilityCheckItem) -> String {
    match &item.note {
        Some(note) => format!("{:?}: {:?} ({})", item.feature, item.support, note),
        None => format!("{:?}: {:?}", item.feature, item.support),
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
    fn test_feature_report_and_available_support() {
        let report = CapabilityReport::current();
        let webview = report
            .feature_report(PlatformFeature::WebView)
            .expect("WebView should be present in the current report");

        assert_eq!(webview.support, SupportLevel::Full);
        assert!(webview.is_full());
        assert!(report.is_available(PlatformFeature::WebView));
    }

    #[test]
    fn test_input_capabilities_are_reported() {
        let report = CapabilityReport::current();
        for feature in [
            PlatformFeature::PrecisionPointerInput,
            PlatformFeature::TouchInput,
            PlatformFeature::PenInput,
            PlatformFeature::GestureInput,
            PlatformFeature::SpellChecking,
            PlatformFeature::Geolocation,
            PlatformFeature::FileExportDrag,
            PlatformFeature::AppWindowCapture,
            PlatformFeature::UsbDevices,
            PlatformFeature::HidDevices,
            PlatformFeature::SerialPorts,
            PlatformFeature::BluetoothDevices,
        ] {
            assert!(
                report.feature_report(feature).is_some(),
                "{feature:?} should be present in capability reports"
            );
        }
        assert!(report.is_supported(PlatformFeature::PrecisionPointerInput));
    }

    #[test]
    fn test_require_all_reports_missing_feature() {
        let report = CapabilityReport {
            os: "test".into(),
            os_version: "1".into(),
            features: vec![FeatureReport {
                feature: PlatformFeature::FileBookmarks,
                support: SupportLevel::Unsupported,
                note: Some("scoped bookmarks are unavailable".into()),
            }],
        };

        let error = report
            .require_all([PlatformFeature::FileBookmarks])
            .expect_err("unsupported feature should fail strict requirements");

        assert!(error.contains("FileBookmarks"));
        assert!(error.contains("Unsupported"));
        assert!(error.contains("scoped bookmarks are unavailable"));
    }

    #[test]
    fn test_capability_check_separates_required_and_preferred() {
        let report = CapabilityReport {
            os: "test".into(),
            os_version: "1".into(),
            features: vec![
                FeatureReport {
                    feature: PlatformFeature::WebView,
                    support: SupportLevel::Full,
                    note: None,
                },
                FeatureReport {
                    feature: PlatformFeature::GlobalHotkeys,
                    support: SupportLevel::Partial,
                    note: Some("interactive consent required".into()),
                },
                FeatureReport {
                    feature: PlatformFeature::FileBookmarks,
                    support: SupportLevel::Unsupported,
                    note: None,
                },
                FeatureReport {
                    feature: PlatformFeature::TouchInput,
                    support: SupportLevel::Partial,
                    note: Some("raw contacts require fallback UI".into()),
                },
                FeatureReport {
                    feature: PlatformFeature::SpellChecking,
                    support: SupportLevel::Partial,
                    note: Some("descriptor only".into()),
                },
                FeatureReport {
                    feature: PlatformFeature::Geolocation,
                    support: SupportLevel::RequiresInit,
                    note: Some("permission required".into()),
                },
                FeatureReport {
                    feature: PlatformFeature::FileExportDrag,
                    support: SupportLevel::Partial,
                    note: Some("descriptor only".into()),
                },
                FeatureReport {
                    feature: PlatformFeature::AppWindowCapture,
                    support: SupportLevel::Partial,
                    note: Some("descriptor only".into()),
                },
                FeatureReport {
                    feature: PlatformFeature::UsbDevices,
                    support: SupportLevel::Partial,
                    note: Some("descriptor only".into()),
                },
                FeatureReport {
                    feature: PlatformFeature::BluetoothDevices,
                    support: SupportLevel::Partial,
                    note: Some("descriptor only".into()),
                },
            ],
        };

        let result = CapabilityCheck::new()
            .require(PlatformFeature::WebView)
            .require_available(PlatformFeature::GlobalHotkeys)
            .prefer_available(PlatformFeature::TouchInput)
            .prefer_available(PlatformFeature::SpellChecking)
            .prefer_available(PlatformFeature::Geolocation)
            .prefer_available(PlatformFeature::FileExportDrag)
            .prefer_available(PlatformFeature::AppWindowCapture)
            .prefer_available(PlatformFeature::UsbDevices)
            .prefer_available(PlatformFeature::BluetoothDevices)
            .prefer(PlatformFeature::FileBookmarks)
            .evaluate(&report);

        assert!(result.is_ok());
        assert!(result.missing_required().is_empty());
        assert_eq!(result.missing_preferred().len(), 1);
        assert_eq!(
            result.missing_preferred()[0].feature,
            PlatformFeature::FileBookmarks
        );
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
