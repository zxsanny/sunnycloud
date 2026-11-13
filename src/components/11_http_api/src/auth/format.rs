use axum::http::HeaderMap;
use axum::http::header::{AUTHORIZATION, COOKIE};
use contracts::Credential;
use sc_ids::{RawToken, TokenKind};

use crate::error::HttpServerError;

/// The prefix every `__Host-` cookie name carries (`api_v1_surface.md`, credential transport).
const HOST_PREFIX: &str = "__Host-";

/// How a presented credential is read. The cookie names come from configuration (12); a bearer is a device token
/// when it carries `TokenKind::DeviceToken`'s prefix (`sc_ids`, AZ-4268), otherwise a session access token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CredentialFormat {
    /// The full name of the browser session cookie, `__Host-` prefix included.
    pub session_cookie: String,
    /// The full name of the browser-binding cookie of a sign-in attempt, `__Host-` prefix included.
    pub binding_cookie: String,
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
        if !is_host_cookie(&self.session_cookie) {
            return invalid("session_cookie must be `__Host-` followed by a cookie-name token");
        }
        if !is_host_cookie(&self.binding_cookie) {
            return invalid("binding_cookie must be `__Host-` followed by a cookie-name token");
        }
        if self.binding_cookie == self.session_cookie {
            return invalid("binding_cookie and session_cookie must differ");
        }
        Ok(())
    }

    /// The credential of a user route: a bearer wins over the cookie. `None` is "no credential".
    pub(crate) fn credential(&self, headers: &HeaderMap) -> Option<(Credential, CredentialSource)> {
        if let Some(token) = bearer(headers) {
            let credential = if token.starts_with(TokenKind::DeviceToken.prefix()) {
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

fn is_host_cookie(name: &str) -> bool {
    let token = name.strip_prefix(HOST_PREFIX).unwrap_or("");
    !token.is_empty()
        && token
            .bytes()
            .all(|byte| byte.is_ascii_graphic() && !b"=;,\"\\()<>@:/[]?{}".contains(&byte))
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
