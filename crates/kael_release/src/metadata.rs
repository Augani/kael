//! Application metadata validation for packaging.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Application metadata required for packaging and distribution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppMetadata {
    /// Application name (e.g. "kael-editor").
    pub name: String,
    /// Semantic version string (e.g. "1.2.3").
    pub version: String,
    /// Bundle identifier (e.g. "com.company.app").
    pub identifier: String,
    /// Human-readable display name.
    pub display_name: String,
    /// Short description of the application.
    pub description: String,
    /// Author or organization name.
    pub author: String,
    /// Map of icon size labels to file paths (e.g. "128x128" -> "icons/128.png").
    pub icon_paths: HashMap<String, String>,
    /// macOS entitlements to request.
    pub entitlements: Vec<String>,
}

impl AppMetadata {
    /// Validates all metadata fields.
    pub fn validate(&self) -> anyhow::Result<()> {
        if self.name.is_empty() {
            anyhow::bail!("app name must not be empty");
        }
        if self.display_name.is_empty() {
            anyhow::bail!("display name must not be empty");
        }
        if self.description.is_empty() {
            anyhow::bail!("description must not be empty");
        }
        if self.author.is_empty() {
            anyhow::bail!("author must not be empty");
        }

        Self::validate_version_str(&self.version)?;
        Self::validate_identifier_str(&self.identifier)?;

        Ok(())
    }

    /// Validates that icon paths are non-empty and have valid extensions.
    pub fn validate_icon_paths(&self) -> anyhow::Result<()> {
        if self.icon_paths.is_empty() {
            anyhow::bail!("at least one icon path is required");
        }

        let valid_extensions = ["png", "icns", "ico", "svg"];
        for (label, path) in &self.icon_paths {
            if path.is_empty() {
                anyhow::bail!("icon path for '{}' must not be empty", label);
            }

            let has_valid_ext = valid_extensions
                .iter()
                .any(|ext| path.ends_with(&format!(".{ext}")));
            if !has_valid_ext {
                anyhow::bail!(
                    "icon path '{}' for '{}' must have a valid extension ({:?})",
                    path,
                    label,
                    valid_extensions
                );
            }
        }

        Ok(())
    }

    /// Validates a version string as semver (MAJOR.MINOR.PATCH).
    pub fn validate_version(version: &str) -> anyhow::Result<()> {
        Self::validate_version_str(version)
    }

    fn validate_version_str(version: &str) -> anyhow::Result<()> {
        let parts: Vec<&str> = version.split('.').collect();
        if parts.len() != 3 {
            anyhow::bail!("version '{}' must be in MAJOR.MINOR.PATCH format", version);
        }
        for part in &parts {
            if part.parse::<u64>().is_err() {
                anyhow::bail!(
                    "version component '{}' in '{}' must be a non-negative integer",
                    part,
                    version
                );
            }
        }
        Ok(())
    }

    fn validate_identifier_str(identifier: &str) -> anyhow::Result<()> {
        if identifier.is_empty() {
            anyhow::bail!("identifier must not be empty");
        }

        let parts: Vec<&str> = identifier.split('.').collect();
        if parts.len() < 3 {
            anyhow::bail!(
                "identifier '{}' must have at least 3 segments (e.g. com.company.app)",
                identifier
            );
        }

        for part in &parts {
            if part.is_empty() {
                anyhow::bail!("identifier '{}' contains empty segment", identifier);
            }
            if !part
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
            {
                anyhow::bail!("identifier segment '{}' contains invalid characters", part);
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_metadata() -> AppMetadata {
        let mut icon_paths = HashMap::new();
        icon_paths.insert("128x128".to_string(), "icons/128.png".to_string());
        AppMetadata {
            name: "kael-editor".to_string(),
            version: "1.0.0".to_string(),
            identifier: "com.kael.editor".to_string(),
            display_name: "Kael Editor".to_string(),
            description: "A desktop editor".to_string(),
            author: "Kael Team".to_string(),
            icon_paths,
            entitlements: vec!["com.apple.security.network.client".to_string()],
        }
    }

    #[test]
    fn valid_metadata_passes() {
        valid_metadata().validate().unwrap();
    }

    #[test]
    fn empty_name_fails() {
        let mut m = valid_metadata();
        m.name = String::new();
        assert!(m.validate().is_err());
    }

    #[test]
    fn empty_author_fails() {
        let mut m = valid_metadata();
        m.author = String::new();
        assert!(m.validate().is_err());
    }

    #[test]
    fn invalid_version_fails() {
        let mut m = valid_metadata();
        m.version = "1.0".to_string();
        assert!(m.validate().is_err());
    }

    #[test]
    fn non_numeric_version_fails() {
        let mut m = valid_metadata();
        m.version = "1.0.beta".to_string();
        assert!(m.validate().is_err());
    }

    #[test]
    fn identifier_too_few_segments() {
        let mut m = valid_metadata();
        m.identifier = "com.kael".to_string();
        assert!(m.validate().is_err());
    }

    #[test]
    fn identifier_empty_segment() {
        let mut m = valid_metadata();
        m.identifier = "com..editor".to_string();
        assert!(m.validate().is_err());
    }

    #[test]
    fn identifier_invalid_chars() {
        let mut m = valid_metadata();
        m.identifier = "com.kael.ed itor".to_string();
        assert!(m.validate().is_err());
    }

    #[test]
    fn validate_icon_paths_valid() {
        let m = valid_metadata();
        m.validate_icon_paths().unwrap();
    }

    #[test]
    fn validate_icon_paths_empty_map() {
        let mut m = valid_metadata();
        m.icon_paths.clear();
        assert!(m.validate_icon_paths().is_err());
    }

    #[test]
    fn validate_icon_paths_bad_extension() {
        let mut m = valid_metadata();
        m.icon_paths
            .insert("64x64".to_string(), "icons/bad.bmp".to_string());
        assert!(m.validate_icon_paths().is_err());
    }

    #[test]
    fn validate_version_static() {
        assert!(AppMetadata::validate_version("1.2.3").is_ok());
        assert!(AppMetadata::validate_version("0.0.0").is_ok());
        assert!(AppMetadata::validate_version("1.2").is_err());
    }

    #[test]
    fn metadata_serialization_roundtrip() {
        let m = valid_metadata();
        let json = serde_json::to_string(&m).unwrap();
        let restored: AppMetadata = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.name, m.name);
        assert_eq!(restored.identifier, m.identifier);
    }
}
