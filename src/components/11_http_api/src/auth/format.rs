use axum::http::HeaderMap;
use axum::http::header::{AUTHORIZATION, COOKIE};
use contracts::Credential;
use sc_ids::{RawToken, TokenKind};

use crate::error::HttpServerError;

/// The prefix every `__Host-` cookie name carries (`api_v1_surface.md`, credential transport).
const HOST_PREFIX: &str = "__Host-";

/// How a presented credential is read. Neither value is fixed by the documents (`sc_ids.md` open point 1 for
/// the token prefixes, none for the cookie name), so 12 supplies both from configuration and 11 invents none.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CredentialFormat {
    /// The full name of the browser session cookie, `__Host-` prefix included.
    pub session_cookie: String,
    /// The marker a device token starts with; any other bearer is a session access token.
    pub device_token_prefix: String,
}

/// The transport a request's credential arrived in. Cookie-authenticated mutations are the ones CSRF checks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CredentialSource {
    Cookie,
    Bearer,
}

impl CredentialFormat {
    pub(crate) fn check(&self) -> Result<(), HttpServerError> {
        let invalid = |reason: &str| Err(HttpServerError::InvalidConfig(reason.to_owned()));
        let name = self.session_cookie.strip_prefix(HOST_PREFIX).unwrap_or("");
        if name.is_empty()
            || !name
                .bytes()
                .all(|byte| byte.is_ascii_graphic() && !b"=;,\"\\()<>@:/[]?{}".contains(&byte))
        {
            return invalid("session_cookie must be `__Host-` followed by a cookie-name token");
        }
        if self.device_token_prefix.is_empty() {
            return invalid("device_token_prefix must not be empty");
        }
        Ok(())
    }

    /// The credential of a user route: a bearer wins over the cookie. `None` is "no credential".
    pub(crate) fn credential(&self, headers: &HeaderMap) -> Option<(Credential, CredentialSource)> {
        if let Some(token) = bearer(headers) {
            let credential = if token.starts_with(&self.device_token_prefix) {
                Credential::Device(RawToken::from_presented(
                    TokenKind::DeviceToken,
                    token.to_owned(),
                ))
            } else {
                Credential::Session(RawToken::from_presented(
                    TokenKind::Session,
                    token.to_owned(),
                ))
            };
            return Some((credential, CredentialSource::Bearer));
        }
        let value = self.cookie(headers)?;
        Some((
            Credential::Session(RawToken::from_presented(
                TokenKind::Session,
                value.to_owned(),
            )),
            CredentialSource::Cookie,
        ))
    }

    /// True when the request carries the browser session cookie.
    pub(crate) fn has_session_cookie(&self, headers: &HeaderMap) -> bool {
        self.cookie(headers).is_some()
    }

    fn cookie<'a>(&self, headers: &'a HeaderMap) -> Option<&'a str> {
        headers
            .get_all(COOKIE)
            .iter()
            .filter_map(|value| text(value))
            .flat_map(|line| line.split(';'))
            .filter_map(|pair| pair.trim().split_once('='))
            .find(|(name, value)| *name == self.session_cookie && !value.is_empty())
            .map(|(_, value)| value)
    }
}

/// The token of an `Authorization: Bearer` header; `None` for no header, another scheme or an empty token.
pub(crate) fn bearer(headers: &HeaderMap) -> Option<&str> {
    let (scheme, token) = text(headers.get(AUTHORIZATION)?)?.split_once(' ')?;
    let token = token.trim();
    (scheme.eq_ignore_ascii_case("bearer") && !token.is_empty()).then_some(token)
}

/// A header value that is not text cannot be a token or a cookie list of ours, so it counts as absent.
fn text(value: &axum::http::HeaderValue) -> Option<&str> {
    value.to_str().ok()
}
