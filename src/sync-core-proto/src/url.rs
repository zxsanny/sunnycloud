use std::fmt;

use thiserror::Error;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Error)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Error))]
pub enum UrlError {
    /// The scheme is `http` and the caller's build does not allow plain HTTP (AC-54).
    #[error("plain http is not allowed")]
    InsecureScheme,
    /// Not an absolute `https` (or allowed `http`) URL with a host, or a rule below is broken.
    #[error("not a valid server URL")]
    Invalid,
}

/// A validated server base URL.
///
/// Rules (decision for `sync_core_proto.md` open point 3):
/// - the scheme is `https`, or `http` when the caller allows plain HTTP; any other scheme is `Invalid`;
/// - the scheme is case-insensitive and stored in lower case, as is the host;
/// - a host is required: a registered name or IPv4 literal (letters, digits, `-`, `.`), or a bracketed
///   IPv6 literal; an optional port is 1..=65535;
/// - user info, a query and a fragment are `Invalid` (a base URL carries no credentials and no state);
/// - a path is kept (the server may sit behind a prefix) without its trailing slashes;
/// - whitespace and control characters anywhere are `Invalid`.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ServerUrl {
    https: bool,
    text: String,
}

fn valid_host(host: &str) -> bool {
    if let Some(inner) = host
        .strip_prefix('[')
        .and_then(|rest| rest.strip_suffix(']'))
    {
        return !inner.is_empty()
            && inner.contains(':')
            && inner
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() || byte == b':' || byte == b'.');
    }
    !host.is_empty()
        && host
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'.')
}

fn valid_port(port: &str) -> bool {
    !port.is_empty()
        && port.bytes().all(|byte| byte.is_ascii_digit())
        && port.parse::<u16>().is_ok_and(|value| value != 0)
}

/// Splits `host[:port]`; an IPv6 literal ends at `]`.
fn split_port(authority: &str) -> (&str, Option<&str>) {
    let search_from = authority.rfind(']').map_or(0, |end| end + 1);
    match authority[search_from..].rfind(':') {
        Some(offset) => {
            let at = search_from + offset;
            (&authority[..at], Some(&authority[at + 1..]))
        }
        None => (authority, None),
    }
}

impl ServerUrl {
    pub fn parse(text: &str, allow_plain_http: bool) -> Result<Self, UrlError> {
        if text.chars().any(|c| c.is_whitespace() || c.is_control()) {
            return Err(UrlError::Invalid);
        }
        let (scheme, rest) = text.split_once("://").ok_or(UrlError::Invalid)?;
        let https = if scheme.eq_ignore_ascii_case("https") {
            true
        } else if scheme.eq_ignore_ascii_case("http") {
            false
        } else {
            return Err(UrlError::Invalid);
        };
        if rest.contains(['?', '#']) {
            return Err(UrlError::Invalid);
        }
        let (authority, path) = rest.find('/').map_or((rest, ""), |at| rest.split_at(at));
        if authority.contains('@') {
            return Err(UrlError::Invalid);
        }
        let (host, port) = split_port(authority);
        if !valid_host(host) || port.is_some_and(|port| !valid_port(port)) {
            return Err(UrlError::Invalid);
        }
        if !https && !allow_plain_http {
            return Err(UrlError::InsecureScheme);
        }
        let scheme = if https { "https" } else { "http" };
        let path = path.trim_end_matches('/');
        Ok(Self {
            https,
            text: format!("{scheme}://{}{path}", authority.to_ascii_lowercase()),
        })
    }

    pub fn is_https(&self) -> bool {
        self.https
    }

    /// The normalised URL, without a trailing slash.
    pub fn as_str(&self) -> &str {
        &self.text
    }
}

impl fmt::Display for ServerUrl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}
