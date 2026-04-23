use std::cmp::Ordering;
use std::fmt::{Display, Formatter};
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::error::{RaisError, Result};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Version(String);

impl Version {
    pub fn parse(input: impl AsRef<str>) -> Result<Self> {
        let raw = input.as_ref().trim();
        if raw.is_empty() || !raw.chars().any(|ch| ch.is_ascii_digit()) {
            return Err(RaisError::InvalidVersion(input.as_ref().to_string()));
        }

        Ok(Self(raw.to_string()))
    }

    pub fn raw(&self) -> &str {
        &self.0
    }

    pub fn numeric_parts(&self) -> Vec<u64> {
        let mut parts = Vec::new();
        let mut current = String::new();

        for ch in self.0.chars() {
            if ch.is_ascii_digit() {
                current.push(ch);
            } else if !current.is_empty() {
                if let Ok(number) = current.parse() {
                    parts.push(number);
                }
                current.clear();
            }
        }

        if !current.is_empty() {
            if let Ok(number) = current.parse() {
                parts.push(number);
            }
        }

        parts
    }

    pub fn cmp_lenient(&self, other: &Self) -> Ordering {
        let left = self.numeric_parts();
        let right = other.numeric_parts();
        let max_len = left.len().max(right.len());

        for index in 0..max_len {
            let left_part = left.get(index).copied().unwrap_or_default();
            let right_part = right.get(index).copied().unwrap_or_default();
            match left_part.cmp(&right_part) {
                Ordering::Equal => {}
                ordering => return ordering,
            }
        }

        self.0.cmp(&other.0)
    }
}

impl Display for Version {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for Version {
    type Err = RaisError;

    fn from_str(s: &str) -> Result<Self> {
        Self::parse(s)
    }
}

#[cfg(test)]
mod tests {
    use super::Version;

    #[test]
    fn parses_reaper_style_versions() {
        let version = Version::parse("7.69").unwrap();
        assert_eq!(version.numeric_parts(), vec![7, 69]);
    }

    #[test]
    fn compares_four_part_extension_versions() {
        let old = Version::parse("2.14.0.6").unwrap();
        let new = Version::parse("2.14.0.7").unwrap();
        assert!(old.cmp_lenient(&new).is_lt());
    }

    #[test]
    fn compares_snapshot_style_versions_best_effort() {
        let old = Version::parse("2024.1pre-10").unwrap();
        let new = Version::parse("2024.1pre-11").unwrap();
        assert!(old.cmp_lenient(&new).is_lt());
    }
}
