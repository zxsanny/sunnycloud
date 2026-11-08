use axum::http::Uri;

const HTTP: &str = "http";
const HTTPS: &str = "https";

/// The origin of `text` as `scheme://host[:port]`, lowercase, with the default port of the scheme dropped, so
/// two spellings of one origin compare equal. `None` for anything that is not exactly an origin: `null`, a
/// path, a query, user info, another scheme or no host.
pub(super) fn canonical(text: &str) -> Option<String> {
    let uri: Uri = text.parse().ok()?;
    let scheme = uri.scheme_str()?.to_ascii_lowercase();
    let default_port = match scheme.as_str() {
        HTTP => 80,
        HTTPS => 443,
        _ => return None,
    };
    let authority = uri.authority()?;
    let spelled = text.get(scheme.len() + "://".len()..)?;
    if authority.as_str().contains('@') || !spelled.eq_ignore_ascii_case(authority.as_str()) {
        return None;
    }
    let has_port_text = authority
        .as_str()
        .strip_prefix(authority.host())
        .is_some_and(|rest| !rest.is_empty());
    if has_port_text && authority.port_u16().is_none() {
        return None;
    }
    let host = authority.host().to_ascii_lowercase();
    Some(match authority.port_u16() {
        Some(port) if port != default_port => format!("{scheme}://{host}:{port}"),
        _ => format!("{scheme}://{host}"),
    })
}

/// The origin of a configured `public_url`: an origin with at most one trailing slash.
pub(super) fn from_public_url(url: &str) -> Option<String> {
    canonical(url.strip_suffix('/').unwrap_or(url))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spellings_of_one_origin_are_equal() {
        // Arrange
        let spellings = [
            "https://sunnycloud.test",
            "HTTPS://SunnyCloud.Test",
            "https://sunnycloud.test:443",
        ];

        // Act
        let canonical: Vec<_> = spellings.map(canonical).into();

        // Assert
        assert!(
            canonical
                .iter()
                .all(|origin| origin.as_deref() == Some("https://sunnycloud.test"))
        );
    }

    #[test]
    fn a_different_scheme_host_or_port_is_a_different_origin() {
        // Arrange
        let foreign = [
            "http://sunnycloud.test",
            "https://sunnycloud.test:8443",
            "https://sunnycloud.test.evil.example",
            "https://evil.example",
            "https://app.sunnycloud.test",
        ];

        // Act
        let origins: Vec<_> = foreign.map(canonical).into();

        // Assert
        assert!(
            origins
                .iter()
                .all(|origin| origin.as_deref() != Some("https://sunnycloud.test"))
        );
    }

    #[test]
    fn text_that_is_not_exactly_an_origin_has_none() {
        // Arrange
        let malformed = [
            "null",
            "",
            "sunnycloud.test",
            "https://",
            "https://sunnycloud.test/",
            "https://sunnycloud.test/path",
            "https://sunnycloud.test?x=1",
            "https://user@sunnycloud.test",
            "ftp://sunnycloud.test",
            "https://sunnycloud.test:99999",
            "https://sunnycloud.test:",
        ];

        // Act
        let origins: Vec<_> = malformed.map(canonical).into();

        // Assert
        assert!(origins.iter().all(Option::is_none), "{origins:?}");
    }

    #[test]
    fn a_public_url_may_end_in_one_slash() {
        // Arrange
        let urls = ["https://sunnycloud.test", "https://sunnycloud.test/"];

        // Act
        let origins = urls.map(from_public_url);

        // Assert
        let expected = Some("https://sunnycloud.test".to_owned());
        assert_eq!(origins, [expected.clone(), expected]);
        assert_eq!(from_public_url("https://sunnycloud.test//"), None);
        assert_eq!(from_public_url("https://sunnycloud.test/app"), None);
    }
}
