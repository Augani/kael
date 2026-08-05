#![doc = include_str!("../README.md")]
#![deny(missing_docs)]

use std::{
    fmt::{self, Display},
    str::FromStr,
};

use serde::{Deserialize, Serialize, de::Error};

/// An error returned while parsing a stable semantic-version triplet.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ParseSemanticVersionError {
    /// A required version component was absent or empty.
    MissingComponent {
        /// The absent component name.
        component: &'static str,
    },
    /// A component contained something other than ASCII digits.
    NonNumericComponent {
        /// The invalid component name.
        component: &'static str,
    },
    /// A multi-digit component began with zero.
    LeadingZero {
        /// The invalid component name.
        component: &'static str,
    },
    /// A component exceeded the fixed-width representation.
    ComponentOverflow {
        /// The overflowing component name.
        component: &'static str,
    },
    /// More than three dot-separated components were supplied.
    UnexpectedComponent,
}

impl Display for ParseSemanticVersionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingComponent { component } => {
                write!(f, "missing {component} version number")
            }
            Self::NonNumericComponent { component } => {
                write!(
                    f,
                    "{component} version number must contain only ASCII digits"
                )
            }
            Self::LeadingZero { component } => {
                write!(
                    f,
                    "{component} version number must not contain a leading zero"
                )
            }
            Self::ComponentOverflow { component } => {
                write!(f, "{component} version number is too large")
            }
            Self::UnexpectedComponent => {
                f.write_str("unexpected version component after patch number")
            }
        }
    }
}

impl std::error::Error for ParseSemanticVersionError {}

/// A [semantic version](https://semver.org/) number.
#[derive(Clone, Copy, Debug, Default, Eq, Ord, PartialEq, PartialOrd)]
pub struct SemanticVersion {
    major: u64,
    minor: u64,
    patch: u64,
}

impl SemanticVersion {
    /// Returns a new [`SemanticVersion`] from the given components.
    pub const fn new(major: u64, minor: u64, patch: u64) -> Self {
        Self {
            major,
            minor,
            patch,
        }
    }

    /// Returns the major version number.
    #[inline(always)]
    pub fn major(&self) -> u64 {
        self.major
    }

    /// Returns the minor version number.
    #[inline(always)]
    pub fn minor(&self) -> u64 {
        self.minor
    }

    /// Returns the patch version number.
    #[inline(always)]
    pub fn patch(&self) -> u64 {
        self.patch
    }
}

impl FromStr for SemanticVersion {
    type Err = ParseSemanticVersionError;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        let mut components = s.trim().split('.');
        let major = parse_component(required_component(components.next(), "major")?, "major")?;
        let minor = parse_component(required_component(components.next(), "minor")?, "minor")?;
        let patch = parse_component(required_component(components.next(), "patch")?, "patch")?;
        if components.next().is_some() {
            return Err(ParseSemanticVersionError::UnexpectedComponent);
        }
        Ok(Self {
            major,
            minor,
            patch,
        })
    }
}

fn required_component<'a>(
    component: Option<&'a str>,
    name: &'static str,
) -> std::result::Result<&'a str, ParseSemanticVersionError> {
    component.ok_or(ParseSemanticVersionError::MissingComponent { component: name })
}

fn parse_component(
    component: &str,
    name: &'static str,
) -> std::result::Result<u64, ParseSemanticVersionError> {
    if component.is_empty() {
        return Err(ParseSemanticVersionError::MissingComponent { component: name });
    }
    if !component.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(ParseSemanticVersionError::NonNumericComponent { component: name });
    }
    if component.len() > 1 && component.starts_with('0') {
        return Err(ParseSemanticVersionError::LeadingZero { component: name });
    }
    component
        .parse()
        .map_err(|_| ParseSemanticVersionError::ComponentOverflow { component: name })
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
        Self::from_str(&string).map_err(Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::{ParseSemanticVersionError, SemanticVersion};

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
        let maximum = format!("{}.0.0", u64::MAX);
        assert_eq!(
            maximum.parse::<SemanticVersion>().unwrap().major(),
            u64::MAX
        );

        let error = "18446744073709551616.0.0"
            .parse::<SemanticVersion>()
            .unwrap_err();
        assert_eq!(
            error,
            ParseSemanticVersionError::ComponentOverflow { component: "major" }
        );
    }

    #[test]
    fn parser_reports_structured_component_errors() {
        assert_eq!(
            "1.2".parse::<SemanticVersion>().unwrap_err(),
            ParseSemanticVersionError::MissingComponent { component: "patch" }
        );
        assert_eq!(
            "1.x.3".parse::<SemanticVersion>().unwrap_err(),
            ParseSemanticVersionError::NonNumericComponent { component: "minor" }
        );
        assert_eq!(
            "1.02.3".parse::<SemanticVersion>().unwrap_err(),
            ParseSemanticVersionError::LeadingZero { component: "minor" }
        );
    }
}
