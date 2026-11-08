use std::fmt;

/// The client families of `X-Sunnycloud-Client` (`server_info.md`). Also the `client` label of
/// `sync_client_versions_total`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ClientFamily {
    Web,
    Android,
    Ios,
    Desktop,
}

impl ClientFamily {
    pub const ALL: [ClientFamily; 4] = [
        ClientFamily::Web,
        ClientFamily::Android,
        ClientFamily::Ios,
        ClientFamily::Desktop,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            ClientFamily::Web => "web",
            ClientFamily::Android => "android",
            ClientFamily::Ios => "ios",
            ClientFamily::Desktop => "desktop",
        }
    }

    pub fn parse(text: &str) -> Option<ClientFamily> {
        ClientFamily::ALL
            .into_iter()
            .find(|family| family.as_str() == text)
    }
}

/// `MAJOR.MINOR.PATCH`: three decimal components without sign, leading zeros, pre-release or build part.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Semver {
    major: u64,
    minor: u64,
    patch: u64,
}

impl Semver {
    pub fn parse(text: &str) -> Option<Semver> {
        let mut parts = text.split('.');
        let (Some(major), Some(minor), Some(patch), None) =
            (parts.next(), parts.next(), parts.next(), parts.next())
        else {
            return None;
        };
        Some(Semver {
            major: component(major)?,
            minor: component(minor)?,
            patch: component(patch)?,
        })
    }
}

fn component(text: &str) -> Option<u64> {
    let canonical = !text.is_empty()
        && text.bytes().all(|byte| byte.is_ascii_digit())
        && (text == "0" || !text.starts_with('0'));
    if canonical { text.parse().ok() } else { None }
}

impl fmt::Display for Semver {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// The value of `X-Sunnycloud-Client`: `<family>/<semver>`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClientVersion {
    pub family: ClientFamily,
    pub version: Semver,
}

impl ClientVersion {
    /// `None` for any value that is not exactly `<family>/<semver>`.
    pub fn parse(text: &str) -> Option<ClientVersion> {
        let (family, version) = text.split_once('/')?;
        Some(ClientVersion {
            family: ClientFamily::parse(family)?,
            version: Semver::parse(version)?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_family_and_a_semver_parse() {
        // Arrange
        let values = [
            "web/0.1.0",
            "android/1.1.9",
            "ios/10.20.30",
            "desktop/0.0.0",
        ];

        // Act
        let parsed: Vec<_> = values.iter().map(|v| ClientVersion::parse(v)).collect();

        // Assert
        let families: Vec<_> = parsed.iter().map(|p| p.map(|c| c.family)).collect();
        assert_eq!(
            families,
            [
                Some(ClientFamily::Web),
                Some(ClientFamily::Android),
                Some(ClientFamily::Ios),
                Some(ClientFamily::Desktop)
            ]
        );
        assert_eq!(
            parsed[1].map(|c| c.version.to_string()),
            Some("1.1.9".to_owned())
        );
    }

    #[test]
    fn malformed_values_are_rejected() {
        // Arrange
        let values = [
            "foo",
            "web/x.y",
            "web/1.2",
            "web/1.2.3.4",
            "web/1.2.x",
            "web/01.2.3",
            "web/1.2.3-beta",
            "web/-1.2.3",
            "web/+1.2.3",
            "web/1.2.3/extra",
            "web/ 1.2.3",
            "Web/1.2.3",
            "tv/1.2.3",
            "/1.2.3",
            "web/",
            "",
            "web/99999999999999999999.0.0",
        ];

        // Act
        let accepted: Vec<_> = values
            .iter()
            .filter(|value| ClientVersion::parse(value).is_some())
            .collect();

        // Assert
        assert!(accepted.is_empty(), "accepted {accepted:?}");
    }

    #[test]
    fn versions_order_by_component_value_not_by_text() {
        // Arrange
        let (nine, ten) = (Semver::parse("1.9.0"), Semver::parse("1.10.0"));
        let (low, high) = (Semver::parse("1.2.0"), Semver::parse("2.0.0"));

        // Act
        let numeric = nine < ten;
        let major_first = low < high;

        // Assert
        assert!(numeric);
        assert!(major_first);
    }
}
