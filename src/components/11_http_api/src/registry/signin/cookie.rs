//! What a sign-in answer puts into headers: the session cookie, the browser-binding cookie and the redirect.
//!
//! 01 hands out tokens and a binding value, and its scope excludes cookie attributes, so the attributes are the
//! ones `api_v1_surface.md` fixes for a browser credential and none other: the configured `__Host-` name,
//! `Secure`, `HttpOnly`, `SameSite=Lax`, `Path=/`, no `Domain`. The session cookie lives as long as the token the
//! port reports; the binding cookie has no lifetime in its value, so it ends with the browser session.

use axum::http::HeaderValue;
use contracts::{BrowserBinding, SessionTokens};

use crate::auth::CredentialFormat;
use crate::registry::request::{is_cookie_octet, seconds};

const ATTRIBUTES: &str = "Secure; HttpOnly; SameSite=Lax; Path=/";

/// A value 01 produced that cannot be written into a header: 01 broke its contract.
#[derive(Debug, thiserror::Error)]
#[error("the {0} has a character a header value cannot hold")]
pub(super) struct NotHeaderSafe(pub(super) &'static str);

/// The names of the two cookies, from the configuration of 12.
#[derive(Debug, Clone)]
pub(super) struct CookieNames {
    session: String,
    binding: String,
}

impl CookieNames {
    pub(super) fn new(credentials: &CredentialFormat) -> CookieNames {
        CookieNames {
            session: credentials.session_cookie.clone(),
            binding: credentials.binding_cookie.clone(),
        }
    }

    pub(super) fn binding_name(&self) -> &str {
        &self.binding
    }

    /// The session cookie carries the access token for as long as the port says it lives.
    pub(super) fn session_cookie(
        &self,
        tokens: &SessionTokens,
    ) -> Result<HeaderValue, NotHeaderSafe> {
        render(
            &self.session,
            tokens.access.expose(),
            Some(seconds(tokens.expires_in)),
            "access token",
        )
    }

    pub(super) fn binding_cookie(
        &self,
        binding: &BrowserBinding,
    ) -> Result<HeaderValue, NotHeaderSafe> {
        render(&self.binding, &binding.0, None, "browser binding")
    }
}

fn render(
    name: &str,
    value: &str,
    max_age: Option<u64>,
    what: &'static str,
) -> Result<HeaderValue, NotHeaderSafe> {
    if value.is_empty() || !value.bytes().all(is_cookie_octet) {
        return Err(NotHeaderSafe(what));
    }
    let cookie = match max_age {
        Some(seconds) => format!("{name}={value}; {ATTRIBUTES}; Max-Age={seconds}"),
        None => format!("{name}={value}; {ATTRIBUTES}"),
    };
    HeaderValue::try_from(cookie).map_err(|_| NotHeaderSafe(what))
}

/// `Location` for a redirect to `path`, which 01 has checked is a relative return path.
pub(super) fn location(path: &str) -> Result<HeaderValue, NotHeaderSafe> {
    HeaderValue::try_from(path).map_err(|_| NotHeaderSafe("return path"))
}

/// `return_to` with the one-time mobile `code` added to its query (before any fragment).
pub(super) fn with_code(return_to: &str, code: &str) -> String {
    let (path, fragment) = match return_to.split_once('#') {
        Some((path, fragment)) => (path, Some(fragment)),
        None => (return_to, None),
    };
    let separator = if path.contains('?') { '&' } else { '?' };
    match fragment {
        Some(fragment) => format!("{path}{separator}code={code}#{fragment}"),
        None => format!("{path}{separator}code={code}"),
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use sc_ids::{RawToken, TokenKind};

    use super::*;

    fn names() -> CookieNames {
        CookieNames::new(&CredentialFormat {
            session_cookie: "__Host-sid".to_owned(),
            binding_cookie: "__Host-bind".to_owned(),
        })
    }

    fn tokens(access: &str, lives: Duration) -> SessionTokens {
        SessionTokens {
            access: RawToken::from_presented(TokenKind::Session, access.to_owned()),
            refresh: RawToken::from_presented(TokenKind::Refresh, "refresh".to_owned()),
            expires_in: lives,
        }
    }

    #[test]
    fn the_session_cookie_has_the_host_name_the_fixed_attributes_and_the_life_of_the_token() {
        // Arrange
        let tokens = tokens("acc-Ess_1", Duration::from_secs(900));

        // Act
        let cookie = names().session_cookie(&tokens).expect("a cookie");

        // Assert
        assert_eq!(
            cookie.to_str().expect("text"),
            "__Host-sid=acc-Ess_1; Secure; HttpOnly; SameSite=Lax; Path=/; Max-Age=900"
        );
    }

    #[test]
    fn the_binding_cookie_adds_no_lifetime_and_no_domain() {
        // Arrange
        let binding = BrowserBinding("b1nd-ing_9".to_owned());

        // Act
        let cookie = names().binding_cookie(&binding).expect("a cookie");

        // Assert
        assert_eq!(
            cookie.to_str().expect("text"),
            "__Host-bind=b1nd-ing_9; Secure; HttpOnly; SameSite=Lax; Path=/"
        );
    }

    #[test]
    fn a_value_a_header_cannot_hold_is_refused_not_repaired() {
        // Arrange
        let semicolon = tokens("a;b", Duration::from_secs(1));
        let empty = BrowserBinding(String::new());

        // Act
        let session = names().session_cookie(&semicolon);
        let binding = names().binding_cookie(&empty);

        // Assert
        assert!(session.is_err() && binding.is_err());
    }

    #[test]
    fn the_mobile_code_joins_the_query_of_the_return_path_before_its_fragment() {
        // Arrange
        let cases = [
            ("/app/done", "/app/done?code=c1"),
            ("/app/done?x=1", "/app/done?x=1&code=c1"),
            ("/app/done#top", "/app/done?code=c1#top"),
        ];

        for (return_to, expected) in cases {
            // Act
            let joined = with_code(return_to, "c1");

            // Assert
            assert_eq!(joined, expected);
        }
    }
}
