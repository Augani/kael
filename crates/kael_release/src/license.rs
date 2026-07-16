//! License tracking, SBOM generation, and compliance reporting.

use serde::{Deserialize, Serialize};

/// A single dependency's license information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DependencyLicense {
    /// Crate or package name.
    pub name: String,
    /// Version string.
    pub version: String,
    /// SPDX license identifier (e.g. "MIT", "Apache-2.0").
    pub license: String,
    /// Optional source repository URL.
    pub source: Option<String>,
}

/// Aggregated license report for all dependencies.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LicenseReport {
    /// All tracked dependency licenses.
    pub dependencies: Vec<DependencyLicense>,
    /// Unix timestamp when the report was generated.
    pub generated_at: u64,
}

impl LicenseReport {
    /// Creates an empty report with the given generation timestamp.
    pub fn new(generated_at: u64) -> Self {
        Self {
            dependencies: Vec::new(),
            generated_at,
        }
    }

    /// Adds a dependency license entry.
    pub fn add(&mut self, entry: DependencyLicense) {
        self.dependencies.push(entry);
    }

    /// Returns all dependencies matching the given SPDX license identifier.
    pub fn find_by_license(&self, license: &str) -> Vec<&DependencyLicense> {
        self.dependencies
            .iter()
            .filter(|dep| dep.license == license)
            .collect()
    }

    /// Returns whether any dependency uses a copyleft license.
    pub fn has_copyleft(&self) -> bool {
        const COPYLEFT_LICENSES: &[&str] = &[
            "GPL-2.0",
            "GPL-2.0-only",
            "GPL-2.0-or-later",
            "GPL-3.0",
            "GPL-3.0-only",
            "GPL-3.0-or-later",
            "AGPL-3.0",
            "AGPL-3.0-only",
            "AGPL-3.0-or-later",
            "LGPL-2.1",
            "LGPL-2.1-only",
            "LGPL-2.1-or-later",
            "LGPL-3.0",
            "LGPL-3.0-only",
            "LGPL-3.0-or-later",
            "MPL-2.0",
        ];
        self.dependencies.iter().any(|dep| {
            dep.license
                .split(|character: char| {
                    !(character.is_ascii_alphanumeric() || matches!(character, '.' | '-'))
                })
                .any(|identifier| COPYLEFT_LICENSES.contains(&identifier))
        })
    }

    /// Returns a summary mapping license identifiers to their dependency count.
    pub fn summary(&self) -> std::collections::HashMap<String, usize> {
        let mut counts = std::collections::HashMap::new();
        for dep in &self.dependencies {
            *counts.entry(dep.license.clone()).or_insert(0) += 1;
        }
        counts
    }

    /// Serializes the report to a JSON string.
    pub fn to_json(&self) -> anyhow::Result<String> {
        Ok(serde_json::to_string_pretty(self)?)
    }
}

/// A single entry in a Software Bill of Materials.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SbomEntry {
    /// Package name.
    pub name: String,
    /// Version string.
    pub version: String,
    /// Package URL (purl) identifier.
    pub purl: Option<String>,
    /// SHA-256 hash of the package artifact.
    pub sha256: Option<String>,
}

/// Software Bill of Materials for the application.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Sbom {
    /// All component entries.
    pub entries: Vec<SbomEntry>,
    /// SBOM format version.
    pub format_version: String,
}

impl Sbom {
    /// Creates an empty SBOM with the given format version.
    pub fn new(format_version: impl Into<String>) -> Self {
        Self {
            entries: Vec::new(),
            format_version: format_version.into(),
        }
    }

    /// Adds a component entry.
    pub fn add(&mut self, entry: SbomEntry) {
        self.entries.push(entry);
    }

    /// Serializes the SBOM to a JSON string.
    pub fn to_json(&self) -> anyhow::Result<String> {
        Ok(serde_json::to_string_pretty(self)?)
    }

    /// Finds all entries matching the given package name.
    pub fn find_by_name(&self, name: &str) -> Vec<&SbomEntry> {
        self.entries
            .iter()
            .filter(|entry| entry.name == name)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mit_dep() -> DependencyLicense {
        DependencyLicense {
            name: "serde".to_string(),
            version: "1.0.0".to_string(),
            license: "MIT".to_string(),
            source: Some("https://github.com/serde-rs/serde".to_string()),
        }
    }

    fn gpl_dep() -> DependencyLicense {
        DependencyLicense {
            name: "copyleft-lib".to_string(),
            version: "0.1.0".to_string(),
            license: "GPL-3.0".to_string(),
            source: None,
        }
    }

    #[test]
    fn add_and_find_by_license() {
        let mut report = LicenseReport::new(1000);
        report.add(mit_dep());
        report.add(gpl_dep());
        let mit_deps = report.find_by_license("MIT");
        assert_eq!(mit_deps.len(), 1);
        assert_eq!(mit_deps[0].name, "serde");
    }

    #[test]
    fn has_copyleft_true() {
        let mut report = LicenseReport::new(1000);
        report.add(gpl_dep());
        assert!(report.has_copyleft());
    }

    #[test]
    fn has_copyleft_false() {
        let mut report = LicenseReport::new(1000);
        report.add(mit_dep());
        assert!(!report.has_copyleft());
    }

    #[test]
    fn detects_copyleft_inside_spdx_expression() {
        let mut report = LicenseReport::new(1000);
        let mut dependency = mit_dep();
        dependency.license = "MIT OR GPL-3.0-only".to_string();
        report.add(dependency);
        assert!(report.has_copyleft());
    }

    #[test]
    fn summary_counts() {
        let mut report = LicenseReport::new(1000);
        report.add(mit_dep());
        report.add(DependencyLicense {
            name: "tokio".to_string(),
            version: "1.0.0".to_string(),
            license: "MIT".to_string(),
            source: None,
        });
        report.add(gpl_dep());
        let summary = report.summary();
        assert_eq!(summary["MIT"], 2);
        assert_eq!(summary["GPL-3.0"], 1);
    }

    #[test]
    fn report_to_json() {
        let mut report = LicenseReport::new(1000);
        report.add(mit_dep());
        let json = report.to_json().unwrap();
        assert!(json.contains("serde"));
        assert!(json.contains("MIT"));
    }

    #[test]
    fn sbom_add_and_find() {
        let mut sbom = Sbom::new("1.0");
        sbom.add(SbomEntry {
            name: "serde".to_string(),
            version: "1.0.0".to_string(),
            purl: Some("pkg:cargo/serde@1.0.0".to_string()),
            sha256: None,
        });
        sbom.add(SbomEntry {
            name: "tokio".to_string(),
            version: "1.0.0".to_string(),
            purl: None,
            sha256: None,
        });
        let found = sbom.find_by_name("serde");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].version, "1.0.0");
    }

    #[test]
    fn sbom_to_json() {
        let mut sbom = Sbom::new("1.0");
        sbom.add(SbomEntry {
            name: "anyhow".to_string(),
            version: "1.0.0".to_string(),
            purl: None,
            sha256: Some("abcd1234".to_string()),
        });
        let json = sbom.to_json().unwrap();
        assert!(json.contains("anyhow"));
    }

    #[test]
    fn sbom_find_by_name_empty() {
        let sbom = Sbom::new("1.0");
        assert!(sbom.find_by_name("nonexistent").is_empty());
    }

    #[test]
    fn license_report_serialization_roundtrip() {
        let mut report = LicenseReport::new(1700000000);
        report.add(mit_dep());
        let json = report.to_json().unwrap();
        let restored: LicenseReport = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.dependencies.len(), 1);
        assert_eq!(restored.generated_at, 1700000000);
    }
}
