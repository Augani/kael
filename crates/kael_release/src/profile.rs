//! Release profiles and build target configuration.

use serde::{Deserialize, Serialize};

const MAX_PROFILE_NAME_BYTES: usize = 128;
const MAX_FEATURES: usize = 256;
const MAX_FEATURE_BYTES: usize = 128;

/// Supported build targets for release packaging.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum BuildTarget {
    /// macOS on Apple Silicon (aarch64).
    MacosArm64,
    /// macOS on Intel (x86_64).
    MacosX64,
    /// macOS universal binary (arm64 + x86_64).
    MacosUniversal,
    /// Windows on x86_64.
    WindowsX64,
    /// Windows on ARM64.
    WindowsArm64,
    /// Linux on x86_64.
    LinuxX64,
    /// Linux on ARM64.
    LinuxArm64,
}

impl BuildTarget {
    /// Returns the Rust target triple for this build target.
    pub fn triple(&self) -> &'static str {
        match self {
            Self::MacosArm64 => "aarch64-apple-darwin",
            Self::MacosX64 => "x86_64-apple-darwin",
            Self::MacosUniversal => "universal-apple-darwin",
            Self::WindowsX64 => "x86_64-pc-windows-msvc",
            Self::WindowsArm64 => "aarch64-pc-windows-msvc",
            Self::LinuxX64 => "x86_64-unknown-linux-gnu",
            Self::LinuxArm64 => "aarch64-unknown-linux-gnu",
        }
    }

    /// Returns whether this target is for macOS.
    pub fn is_macos(&self) -> bool {
        matches!(
            self,
            Self::MacosArm64 | Self::MacosX64 | Self::MacosUniversal
        )
    }
}

/// Configuration for a release build profile.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReleaseProfile {
    /// Profile name (e.g. "production", "staging").
    pub name: String,
    /// The build target platform/architecture.
    pub target: BuildTarget,
    /// Whether to strip debug symbols from the binary.
    pub strip_symbols: bool,
    /// Whether to enable link-time optimization.
    pub lto: bool,
    /// Optimization level (e.g. "3", "s", "z").
    pub opt_level: String,
    /// Whether to include debug info.
    pub debug_info: bool,
    /// Cargo features to enable for this profile.
    pub features: Vec<String>,
}

impl ReleaseProfile {
    /// Creates a new builder for constructing a release profile.
    pub fn builder(name: impl Into<String>, target: BuildTarget) -> ReleaseProfileBuilder {
        ReleaseProfileBuilder {
            name: name.into(),
            target,
            strip_symbols: true,
            lto: true,
            opt_level: "3".to_string(),
            debug_info: false,
            features: Vec::new(),
        }
    }

    /// Validates the release profile configuration.
    pub fn validate(&self) -> anyhow::Result<()> {
        if self.name.is_empty()
            || self.name.len() > MAX_PROFILE_NAME_BYTES
            || !self
                .name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        {
            anyhow::bail!("profile name is invalid");
        }

        let valid_opt_levels = ["0", "1", "2", "3", "s", "z"];
        if !valid_opt_levels.contains(&self.opt_level.as_str()) {
            anyhow::bail!(
                "invalid opt_level '{}': must be one of {:?}",
                self.opt_level,
                valid_opt_levels
            );
        }
        if self.features.len() > MAX_FEATURES
            || self.features.iter().any(|feature| {
                feature.is_empty()
                    || feature.len() > MAX_FEATURE_BYTES
                    || !feature
                        .as_bytes()
                        .first()
                        .is_some_and(u8::is_ascii_alphanumeric)
                    || !feature
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
            })
        {
            anyhow::bail!("Cargo feature list contains invalid or excessive entries");
        }

        Ok(())
    }

    /// Returns whether this profile is configured for production use.
    pub fn is_production(&self) -> bool {
        self.strip_symbols && self.lto && !self.debug_info && self.opt_level == "3"
    }
}

/// Builder for constructing [`ReleaseProfile`] instances.
#[derive(Debug)]
pub struct ReleaseProfileBuilder {
    name: String,
    target: BuildTarget,
    strip_symbols: bool,
    lto: bool,
    opt_level: String,
    debug_info: bool,
    features: Vec<String>,
}

impl ReleaseProfileBuilder {
    /// Sets whether to strip debug symbols.
    pub fn strip_symbols(mut self, strip: bool) -> Self {
        self.strip_symbols = strip;
        self
    }

    /// Sets whether to enable LTO.
    pub fn lto(mut self, lto: bool) -> Self {
        self.lto = lto;
        self
    }

    /// Sets the optimization level.
    pub fn opt_level(mut self, level: impl Into<String>) -> Self {
        self.opt_level = level.into();
        self
    }

    /// Sets whether to include debug info.
    pub fn debug_info(mut self, debug: bool) -> Self {
        self.debug_info = debug;
        self
    }

    /// Adds a feature to enable.
    pub fn feature(mut self, feature: impl Into<String>) -> Self {
        self.features.push(feature.into());
        self
    }

    /// Adds multiple features to enable.
    pub fn features(mut self, features: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.features.extend(features.into_iter().map(|f| f.into()));
        self
    }

    /// Builds the [`ReleaseProfile`], validating the configuration.
    pub fn build(self) -> anyhow::Result<ReleaseProfile> {
        let profile = ReleaseProfile {
            name: self.name,
            target: self.target,
            strip_symbols: self.strip_symbols,
            lto: self.lto,
            opt_level: self.opt_level,
            debug_info: self.debug_info,
            features: self.features,
        };
        profile.validate()?;
        Ok(profile)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_default_production_profile() {
        let profile = ReleaseProfile::builder("production", BuildTarget::MacosArm64)
            .build()
            .unwrap();
        assert!(profile.is_production());
        assert_eq!(profile.name, "production");
    }

    #[test]
    fn build_debug_profile_is_not_production() {
        let profile = ReleaseProfile::builder("debug", BuildTarget::LinuxX64)
            .debug_info(true)
            .strip_symbols(false)
            .lto(false)
            .opt_level("0")
            .build()
            .unwrap();
        assert!(!profile.is_production());
    }

    #[test]
    fn invalid_opt_level_rejected() {
        let result = ReleaseProfile::builder("test", BuildTarget::WindowsX64)
            .opt_level("invalid")
            .build();
        assert!(result.is_err());
    }

    #[test]
    fn empty_name_rejected() {
        let result = ReleaseProfile::builder("", BuildTarget::LinuxArm64).build();
        assert!(result.is_err());
    }

    #[test]
    fn features_accumulated() {
        let profile = ReleaseProfile::builder("release", BuildTarget::MacosUniversal)
            .feature("gpu")
            .features(["audio", "video"])
            .build()
            .unwrap();
        assert_eq!(profile.features, vec!["gpu", "audio", "video"]);
    }

    #[test]
    fn rejects_argument_like_features() {
        let result = ReleaseProfile::builder("release", BuildTarget::LinuxX64)
            .feature("--manifest-path")
            .build();
        assert!(result.is_err());
    }

    #[test]
    fn build_target_triple() {
        assert_eq!(BuildTarget::MacosArm64.triple(), "aarch64-apple-darwin");
        assert_eq!(BuildTarget::WindowsX64.triple(), "x86_64-pc-windows-msvc");
        assert_eq!(BuildTarget::LinuxX64.triple(), "x86_64-unknown-linux-gnu");
    }

    #[test]
    fn build_target_is_macos() {
        assert!(BuildTarget::MacosArm64.is_macos());
        assert!(BuildTarget::MacosX64.is_macos());
        assert!(BuildTarget::MacosUniversal.is_macos());
        assert!(!BuildTarget::WindowsX64.is_macos());
        assert!(!BuildTarget::LinuxX64.is_macos());
    }

    #[test]
    fn profile_serialization_roundtrip() {
        let profile = ReleaseProfile::builder("prod", BuildTarget::MacosArm64)
            .feature("telemetry")
            .build()
            .unwrap();
        let json = serde_json::to_string(&profile).unwrap();
        let restored: ReleaseProfile = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.name, "prod");
        assert_eq!(restored.target, BuildTarget::MacosArm64);
    }
}
