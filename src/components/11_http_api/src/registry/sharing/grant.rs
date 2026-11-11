//! The access grant of a password-protected link, as a cookie the visitor's scripts cannot read.
//!
//! 06 hands out `AccessGrant{value, expires_at}` and wants both back (the value is an HMAC over the link and the
//! expiry), so the cookie value is `<expires_at in unix microseconds>.<value>`. The cookie is scoped to the
//! path of its own link, so no other route and no other link ever receives it. It cannot use the `__Host-`
//! prefix, which requires `Path=/`; `__Secure-` asks for `Secure` only.

use axum::http::{HeaderMap, HeaderValue, header};
use contracts::AccessGrant;
use sc_clock::Timestamp;

use crate::registry::request::is_cookie_octet;

pub(super) const COOKIE: &str = "__Secure-share-grant";
const LINK_PATH: &str = "/api/v1/public-shares/";

/// A grant or a link token 06 produced that cannot be written into a `Set-Cookie` header: 06 broke its contract.
#[derive(Debug, thiserror::Error)]
pub(super) enum NotCookieSafe {
    #[error("the grant value has a character a cookie value cannot hold")]
    Value,
    #[error("the link token has a character a cookie path cannot hold")]
    Path,
}

/// The grant the request's `Cookie` headers carry for this link; a cookie that is not of this form is no grant.
pub(super) fn from_headers(headers: &HeaderMap) -> Option<AccessGrant> {
    let value = headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|line| line.to_str().ok())
        .flat_map(|line| line.split(';'))
        .filter_map(|pair| pair.trim().split_once('='))
        .find(|(name, _)| *name == COOKIE)
        .map(|(_, value)| value)?;
    let (expires, value) = value.split_once('.')?;
    Some(AccessGrant {
        value: value.to_owned(),
        expires_at: Timestamp::from_unix_micros(expires.parse().ok()?),
    })
}

/// `Set-Cookie` for `grant` on the link `token`, living until the grant expires (`at` is the current time).
pub(super) fn set_cookie(
    grant: &AccessGrant,
    token: &str,
    at: Timestamp,
) -> Result<HeaderValue, NotCookieSafe> {
    if !grant.value.bytes().all(is_cookie_octet) {
        return Err(NotCookieSafe::Value);
    }
    if !token.bytes().all(is_path_octet) {
        return Err(NotCookieSafe::Path);
    }
    let max_age = grant
        .expires_at
        .checked_duration_since(at)
        .map_or(0, |left| left.as_secs());
    let cookie = format!(
        "{COOKIE}={}.{}; HttpOnly; Secure; SameSite=Lax; Path={LINK_PATH}{token}; Max-Age={max_age}",
        grant.expires_at.as_unix_micros(),
        grant.value,
    );
    HeaderValue::try_from(cookie).map_err(|_| NotCookieSafe::Value)
}

fn is_path_octet(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~')
}

#[cfg(test)]
mod tests {
    use super::*;

    const GRANT: &str = "AbC-123_x=";

    fn grant() -> AccessGrant {
        AccessGrant {
            value: GRANT.to_owned(),
            expires_at: Timestamp::from_unix_micros(1_700_000_600_000_000),
        }
    }

    fn at() -> Timestamp {
        Timestamp::from_unix_micros(1_700_000_000_000_000)
    }

    #[test]
    fn the_cookie_is_http_only_secure_and_scoped_to_the_path_of_its_own_link() {
        // Arrange
        let grant = grant();

        // Act
        let cookie = set_cookie(&grant, "sc_shr_Tok-en", at()).expect("a cookie");

        // Assert
        assert_eq!(
            cookie.to_str().expect("text"),
            "__Secure-share-grant=1700000600000000.AbC-123_x=; HttpOnly; Secure; SameSite=Lax; \
             Path=/api/v1/public-shares/sc_shr_Tok-en; Max-Age=600"
        );
    }

    #[test]
    fn a_grant_that_has_already_expired_gets_a_zero_max_age() {
        // Arrange
        let later = Timestamp::from_unix_micros(1_700_000_900_000_000);

        // Act
        let cookie = set_cookie(&grant(), "token", later).expect("a cookie");

        // Assert
        assert!(cookie.to_str().expect("text").ends_with("Max-Age=0"));
    }

    #[test]
    fn a_value_or_a_token_a_header_cannot_hold_is_refused() {
        // Arrange
        let semicolon = AccessGrant {
            value: "a;b".to_owned(),
            ..grant()
        };

        // Act
        let value = set_cookie(&semicolon, "token", at());
        let path = set_cookie(&grant(), "to;ken", at());

        // Assert
        assert!(matches!(value, Err(NotCookieSafe::Value)));
        assert!(matches!(path, Err(NotCookieSafe::Path)));
    }

    #[test]
    fn the_grant_is_read_back_from_among_other_cookies() {
        // Arrange
        let mut headers = HeaderMap::new();
        headers.append(header::COOKIE, HeaderValue::from_static("a=1; b=2"));
        headers.append(
            header::COOKIE,
            HeaderValue::from_static("__Secure-share-grant=1700000600000000.AbC-123_x=; c=3"),
        );

        // Act
        let read = from_headers(&headers);

        // Assert
        assert_eq!(read, Some(grant()));
    }

    #[test]
    fn a_missing_or_malformed_grant_cookie_is_no_grant() {
        // Arrange
        let none = HeaderMap::new();
        let mut malformed = HeaderMap::new();
        malformed.insert(
            header::COOKIE,
            HeaderValue::from_static("__Secure-share-grant=no-expiry"),
        );

        // Act
        let (absent, bad) = (from_headers(&none), from_headers(&malformed));

        // Assert
        assert_eq!((absent, bad), (None, None));
    }
}
