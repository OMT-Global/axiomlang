//! Strict release-only semantic versions used by the package resolver.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct ReleaseVersion {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
}

impl ReleaseVersion {
    pub const fn new(major: u64, minor: u64, patch: u64) -> Self {
        Self {
            major,
            minor,
            patch,
        }
    }

    pub fn parse(value: &str) -> Result<Self, VersionParseError> {
        value.parse()
    }
}

impl fmt::Display for ReleaseVersion {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

impl FromStr for ReleaseVersion {
    type Err = VersionParseError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if value.is_empty() || !value.is_ascii() {
            return Err(VersionParseError::InvalidRelease(value.to_owned()));
        }
        let mut parts = value.split('.');
        let major = parse_component(parts.next(), value)?;
        let minor = parse_component(parts.next(), value)?;
        let patch = parse_component(parts.next(), value)?;
        if parts.next().is_some() {
            return Err(VersionParseError::InvalidRelease(value.to_owned()));
        }
        Ok(Self::new(major, minor, patch))
    }
}

fn parse_component(component: Option<&str>, original: &str) -> Result<u64, VersionParseError> {
    let component =
        component.ok_or_else(|| VersionParseError::InvalidRelease(original.to_owned()))?;
    if component.is_empty()
        || !component.bytes().all(|byte| byte.is_ascii_digit())
        || (component.len() > 1 && component.starts_with('0'))
    {
        return Err(VersionParseError::InvalidRelease(original.to_owned()));
    }
    component
        .parse()
        .map_err(|_| VersionParseError::ComponentOverflow(original.to_owned()))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", content = "version", rename_all = "snake_case")]
pub enum VersionRequirement {
    Exact(ReleaseVersion),
    Caret(ReleaseVersion),
}

impl VersionRequirement {
    pub fn parse(value: &str) -> Result<Self, VersionParseError> {
        value.parse()
    }

    pub const fn minimum(self) -> ReleaseVersion {
        match self {
            Self::Exact(version) | Self::Caret(version) => version,
        }
    }

    pub fn matches(self, candidate: ReleaseVersion) -> bool {
        let minimum = self.minimum();
        if candidate < minimum {
            return false;
        }
        match self {
            Self::Exact(version) => candidate == version,
            Self::Caret(version) if version.major > 0 => candidate.major == version.major,
            Self::Caret(version) if version.minor > 0 => {
                candidate.major == 0 && candidate.minor == version.minor
            }
            Self::Caret(version) => {
                candidate.major == 0 && candidate.minor == 0 && candidate.patch == version.patch
            }
        }
    }
}

impl fmt::Display for VersionRequirement {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Exact(version) => version.fmt(formatter),
            Self::Caret(version) => write!(formatter, "^{version}"),
        }
    }
}

impl FromStr for VersionRequirement {
    type Err = VersionParseError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if let Some(version) = value.strip_prefix('^') {
            if version.starts_with('^') {
                return Err(VersionParseError::InvalidRequirement(value.to_owned()));
            }
            return ReleaseVersion::parse(version)
                .map(Self::Caret)
                .map_err(|_| VersionParseError::InvalidRequirement(value.to_owned()));
        }
        ReleaseVersion::parse(value)
            .map(Self::Exact)
            .map_err(|_| VersionParseError::InvalidRequirement(value.to_owned()))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VersionParseError {
    InvalidRelease(String),
    ComponentOverflow(String),
    InvalidRequirement(String),
}

impl fmt::Display for VersionParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidRelease(value) => {
                write!(formatter, "{value:?} is not a strict release SemVer")
            }
            Self::ComponentOverflow(value) => {
                write!(
                    formatter,
                    "{value:?} contains an overflowing SemVer component"
                )
            }
            Self::InvalidRequirement(value) => {
                write!(formatter, "{value:?} is not an exact or caret requirement")
            }
        }
    }
}

impl std::error::Error for VersionParseError {}

#[cfg(test)]
mod tests {
    use super::*;

    fn version(value: &str) -> ReleaseVersion {
        ReleaseVersion::parse(value).expect("version fixture")
    }

    #[test]
    fn strict_release_versions_reject_noncanonical_or_extended_semver() {
        assert_eq!(version("0.0.0"), ReleaseVersion::new(0, 0, 0));
        assert_eq!(version("12.34.56").to_string(), "12.34.56");
        for invalid in [
            "",
            "1",
            "1.2",
            "1.2.3.4",
            "01.2.3",
            "1.02.3",
            "1.2.03",
            "1.2.3-alpha",
            "1.2.3+build",
            " 1.2.3",
            "1.2.3 ",
            "１.2.3",
        ] {
            assert!(
                ReleaseVersion::parse(invalid).is_err(),
                "{invalid:?} must fail"
            );
        }
    }

    #[test]
    fn exact_and_caret_requirements_follow_release_semver_boundaries() {
        let exact = VersionRequirement::parse("1.2.3").expect("exact");
        assert!(exact.matches(version("1.2.3")));
        assert!(!exact.matches(version("1.2.4")));

        let stable = VersionRequirement::parse("^1.2.3").expect("stable caret");
        assert!(stable.matches(version("1.2.3")));
        assert!(stable.matches(version("1.99.99")));
        assert!(!stable.matches(version("1.2.2")));
        assert!(!stable.matches(version("2.0.0")));

        let zero_minor = VersionRequirement::parse("^0.2.3").expect("zero-minor caret");
        assert!(zero_minor.matches(version("0.2.99")));
        assert!(!zero_minor.matches(version("0.3.0")));
        assert!(!zero_minor.matches(version("0.2.2")));

        let zero_patch = VersionRequirement::parse("^0.0.3").expect("zero-patch caret");
        assert!(zero_patch.matches(version("0.0.3")));
        assert!(!zero_patch.matches(version("0.0.4")));
    }

    #[test]
    fn requirements_reject_wildcards_ranges_and_overflow() {
        for invalid in ["*", "~1.2.3", ">=1.2.3", "^^1.2.3", "^", "^01.2.3"] {
            assert!(VersionRequirement::parse(invalid).is_err(), "{invalid}");
        }
        assert!(
            ReleaseVersion::parse("18446744073709551616.0.0").is_err(),
            "u64 overflow must fail"
        );
    }
}
