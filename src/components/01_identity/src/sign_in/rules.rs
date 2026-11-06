//! The pure rules of sign-in: the return path, the public URL and host, and the state hash.

use reqwest::Url;
use sc_ids::{TokenHash, sha256_token};

/// A path the browser may be sent to after sign-in: one leading slash, no second slash or backslash right
/// after it (`//host` and `/\host` are network-path references), no control character anywhere (a CR or LF
/// would split a header, and browsers drop a tab or newline from a URL before parsing it).
pub(crate) fn is_relative_return_path(path: &str) -> bool {
    path.strip_prefix('/').is_some_and(|rest| {
        !rest.starts_with(['/', '\\'])
            && !path.contains('\\')
            && !path.chars().any(char::is_control)
    })
}

/// `SHA-256(state || browser_binding)`: a state presented without the matching binding hashes to a value
/// that is not stored.
pub(crate) fn state_hash(state: &str, browser_binding: &str) -> TokenHash {
    sha256_token(format!("{state}{browser_binding}"))
}

/// The configured `public_url`: the only source of the redirect URI and of the expected host.
#[derive(Debug, Clone)]
pub(crate) struct PublicUrl {
    base: String,
    authority: String,
    default_port_authority: Option<String>,
}

impl PublicUrl {
    pub(crate) fn parse(public_url: &str) -> Option<Self> {
        let url = Url::parse(public_url).ok()?;
        let host = url.host_str()?.to_ascii_lowercase();
        let (authority, default_port_authority) = match (url.port(), url.port_or_known_default()) {
            (Some(port), _) => (format!("{host}:{port}"), None),
            (None, Some(port)) => (host.clone(), Some(format!("{host}:{port}"))),
            (None, None) => (host, None),
        };
        Some(PublicUrl {
            base: public_url.trim_end_matches('/').to_owned(),
            authority,
            default_port_authority,
        })
    }

    pub(crate) fn callback_url(&self, provider_key: &str) -> String {
        format!(
            "{}/api/v1/auth/providers/{provider_key}/callback",
            self.base
        )
    }

    /// The request's `Host` value is the public host, with or without the scheme's default port.
    pub(crate) fn matches_host(&self, host: &str) -> bool {
        host.eq_ignore_ascii_case(&self.authority)
            || self
                .default_port_authority
                .as_deref()
                .is_some_and(|authority| host.eq_ignore_ascii_case(authority))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_relative_single_slash_path_without_control_characters_passes() {
        // Arrange
        let accepted = ["/", "/maps", "/files/a%2Fb?x=1#top"];
        let refused = [
            "",
            "maps",
            "https://evil.test/",
            "//evil.test",
            "/\\evil.test",
            "/a\\b",
            "/x\r\nSet-Cookie: a=b",
            "/a\tb",
            "/\u{7f}",
        ];

        // Act
        let accepted_results = accepted.map(is_relative_return_path);
        let refused_results = refused.map(is_relative_return_path);

        // Assert
        assert_eq!(accepted_results, [true; 3]);
        assert_eq!(refused_results, [false; 9]);
    }

    #[test]
    fn redirect_uri_is_built_from_the_public_url_only() {
        // Arrange
        let public = PublicUrl::parse("https://sunnycloud.test/").expect("public url");

        // Act
        let uri = public.callback_url("github");

        // Assert
        assert_eq!(
            uri,
            "https://sunnycloud.test/api/v1/auth/providers/github/callback"
        );
    }

    #[test]
    fn host_must_be_the_public_host_with_or_without_the_default_port() {
        // Arrange
        let public = PublicUrl::parse("https://Sunnycloud.test").expect("public url");
        let with_port = PublicUrl::parse("https://sunnycloud.test:8443").expect("public url");

        // Act
        let results = [
            public.matches_host("sunnycloud.test"),
            public.matches_host("SUNNYCLOUD.test:443"),
            public.matches_host("evil.test"),
            public.matches_host("sunnycloud.test:8443"),
            with_port.matches_host("sunnycloud.test:8443"),
            with_port.matches_host("sunnycloud.test"),
        ];

        // Assert
        assert_eq!(results, [true, true, false, false, true, false]);
    }

    #[test]
    fn state_hash_is_sha256_of_state_then_binding_and_differs_per_binding() {
        // Arrange: SHA-256("abc") is the published test vector
        let abc = [
            0xba, 0x78, 0x16, 0xbf, 0x8f, 0x01, 0xcf, 0xea, 0x41, 0x41, 0x40, 0xde, 0x5d, 0xae,
            0x22, 0x23, 0xb0, 0x03, 0x61, 0xa3, 0x96, 0x17, 0x7a, 0x9c, 0xb4, 0x10, 0xff, 0x61,
            0xf2, 0x00, 0x15, 0xad,
        ];

        // Act
        let hash = state_hash("ab", "c");
        let other_binding = state_hash("ab", "d");

        // Assert
        assert_eq!(hash.as_bytes(), &abc);
        assert_ne!(hash, other_binding);
    }
}
