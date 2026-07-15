//! Constructs for working with [semantic versions](https://semver.org/).

#![deny(missing_docs)]

use std::{
    fmt::{self, Display},
    str::FromStr,
};

use anyhow::{Context as _, Result, bail};
use serde::{Deserialize, Serialize, de::Error};

/// A [semantic version](https://semver.org/) number.
#[derive(Clone, Copy, Debug, Default, Eq, Ord, PartialEq, PartialOrd)]
pub struct SemanticVersion {
    major: usize,
    minor: usize,
    patch: usize,
}

impl SemanticVersion {
    /// Returns a new [`SemanticVersion`] from the given components.
    pub const fn new(major: usize, minor: usize, patch: usize) -> Self {
        Self {
            major,
            minor,
            patch,
        }
    }

    /// Returns the major version number.
    #[inline(always)]
    pub fn major(&self) -> usize {
        self.major
    }

    /// Returns the minor version number.
    #[inline(always)]
    pub fn minor(&self) -> usize {
        self.minor
    }

    /// Returns the patch version number.
    #[inline(always)]
    pub fn patch(&self) -> usize {
        self.patch
    }
}

impl FromStr for SemanticVersion {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self> {
        let mut components = s.trim().split('.');
        let major = parse_component(
            components.next().context("missing major version number")?,
            "major",
        )?;
        let minor = parse_component(
            components.next().context("missing minor version number")?,
            "minor",
        )?;
        let patch = parse_component(
            components.next().context("missing patch version number")?,
            "patch",
        )?;
        if components.next().is_some() {
            bail!("unexpected version component after patch number");
        }
        Ok(Self {
            major,
            minor,
            patch,
        })
    }
}

fn parse_component(component: &str, name: &str) -> Result<usize> {
    if component.is_empty() {
        bail!("missing {name} version number");
    }
    if !component.bytes().all(|byte| byte.is_ascii_digit()) {
        bail!("{name} version number must contain only ASCII digits");
    }
    if component.len() > 1 && component.starts_with('0') {
        bail!("{name} version number must not contain a leading zero");
    }
    component
        .parse()
        .with_context(|| format!("{name} version number is too large"))
}

impl Display for SemanticVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

impl Serialize for SemanticVersion {
    fn serialize<S>(&self, serializer: S) -> std::prelude::v1::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for SemanticVersion {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let string = String::deserialize(deserializer)?;
        Self::from_str(&string)
            .map_err(|_| Error::custom(format!("Invalid version string \"{string}\"")))
    }
}

#[cfg(test)]
mod tests {
    use super::SemanticVersion;

    #[test]
    fn stable_core_version_round_trips() {
        let version = " 12.34.56 ".parse::<SemanticVersion>().unwrap();
        assert_eq!(version, SemanticVersion::new(12, 34, 56));
        assert_eq!(version.to_string(), "12.34.56");
    }

    #[test]
    fn parser_rejects_non_core_and_ambiguous_versions() {
        for invalid in [
            "",
            "1",
            "1.2",
            "1.2.3.4",
            "1.2.3-beta.1",
            "1.2.3+build",
            "01.2.3",
            "1.02.3",
            "1.2.03",
            "1.-2.3",
            "1.２.3",
        ] {
            assert!(
                invalid.parse::<SemanticVersion>().is_err(),
                "accepted invalid version {invalid:?}"
            );
        }
    }

    #[test]
    fn parser_reports_component_overflow() {
        let too_large = format!("{}.0.0", "9".repeat(usize::BITS as usize));
        assert!(too_large.parse::<SemanticVersion>().is_err());
    }
}
