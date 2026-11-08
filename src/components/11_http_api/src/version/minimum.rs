use std::collections::BTreeMap;

use super::client::{ClientFamily, Semver};

/// Why a `min_client_version` value cannot be used.
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum MinimumError {
    #[error("min_client_version is empty")]
    Empty,
    #[error("`{0}` is not MAJOR.MINOR.PATCH")]
    Version(String),
    #[error("`{0}` is not a client family (web, android, ios, desktop)")]
    Family(String),
    #[error("`{0}` is not family=semver")]
    Entry(String),
    #[error("family `{0}` is listed twice")]
    Duplicate(&'static str),
}

/// The configured minimum client version: one semver for every family, or one per listed family
/// (`server_info.md`, `family=semver,...`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Minimum {
    All(Semver),
    PerFamily(BTreeMap<ClientFamily, Semver>),
}

impl Minimum {
    pub fn parse(text: &str) -> Result<Minimum, MinimumError> {
        let text = text.trim();
        if text.is_empty() {
            return Err(MinimumError::Empty);
        }
        if !text.contains('=') {
            return version(text).map(Minimum::All);
        }
        let mut listed = BTreeMap::new();
        for entry in text.split(',').map(str::trim) {
            let (family, semver) = entry
                .split_once('=')
                .ok_or_else(|| MinimumError::Entry(entry.to_owned()))?;
            let family = ClientFamily::parse(family.trim())
                .ok_or_else(|| MinimumError::Family(family.trim().to_owned()))?;
            if listed.insert(family, version(semver.trim())?).is_some() {
                return Err(MinimumError::Duplicate(family.as_str()));
            }
        }
        Ok(Minimum::PerFamily(listed))
    }

    /// The minimum `family` must meet; `None` when a per-family list does not name it (the document leaves
    /// that case open, so such a family has no minimum).
    pub fn for_family(&self, family: ClientFamily) -> Option<Semver> {
        match self {
            Minimum::All(version) => Some(*version),
            Minimum::PerFamily(listed) => listed.get(&family).copied(),
        }
    }
}

fn version(text: &str) -> Result<Semver, MinimumError> {
    Semver::parse(text).ok_or_else(|| MinimumError::Version(text.to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn semver(text: &str) -> Semver {
        Semver::parse(text).expect("test semver")
    }

    #[test]
    fn a_single_version_applies_to_every_family() {
        // Arrange
        let minimum = Minimum::parse("1.2.0").expect("parses");

        // Act
        let per_family: Vec<_> = ClientFamily::ALL
            .into_iter()
            .map(|family| minimum.for_family(family))
            .collect();

        // Assert
        assert_eq!(per_family, vec![Some(semver("1.2.0")); 4]);
    }

    #[test]
    fn a_list_gives_each_named_family_its_own_minimum_and_others_none() {
        // Arrange
        let minimum = Minimum::parse("web=1.0.0, android=1.5.0").expect("parses");

        // Act
        let web = minimum.for_family(ClientFamily::Web);
        let android = minimum.for_family(ClientFamily::Android);
        let ios = minimum.for_family(ClientFamily::Ios);

        // Assert
        assert_eq!(web, Some(semver("1.0.0")));
        assert_eq!(android, Some(semver("1.5.0")));
        assert_eq!(ios, None);
    }

    #[test]
    fn unusable_values_say_what_is_wrong() {
        // Arrange
        let cases = [
            ("", MinimumError::Empty),
            ("  ", MinimumError::Empty),
            ("1.2", MinimumError::Version("1.2".to_owned())),
            ("web=1.x.0", MinimumError::Version("1.x.0".to_owned())),
            ("tv=1.0.0", MinimumError::Family("tv".to_owned())),
            ("web=1.0.0,1.1.0", MinimumError::Entry("1.1.0".to_owned())),
            ("web=1.0.0,", MinimumError::Entry(String::new())),
            ("web=1.0.0,web=1.1.0", MinimumError::Duplicate("web")),
        ];

        // Act
        let outcomes: Vec<_> = cases
            .iter()
            .map(|(text, _)| Minimum::parse(text).err())
            .collect();

        // Assert
        let expected: Vec<_> = cases.into_iter().map(|(_, error)| Some(error)).collect();
        assert_eq!(outcomes, expected);
    }
}
