//! Which redirects a release download may follow (owner decision 2026-11-12): only to a compiled-in host
//! list, only over HTTPS, at most [`MAX_HOPS`] hops. Anything else fails the check at `source`. The redirect
//! target is never trusted for content: the signed size and SHA-256 still decide.

use reqwest::Url;

pub const MAX_HOPS: usize = 3;

/// The hosts GitHub Releases answers from: `github.com` itself (`/releases/latest/download/…` redirects to
/// `/releases/download/<tag>/…`), `release-assets.githubusercontent.com` (where GitHub has sent release
/// assets since 2025) and `objects.githubusercontent.com` (the earlier asset host, still seen for older
/// releases). Port 443 only.
pub const RELEASE_REDIRECT_HOSTS: [&str; 3] = [
    "github.com",
    "release-assets.githubusercontent.com",
    "objects.githubusercontent.com",
];

/// A host and port a hop may land on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AllowedHost {
    pub host: String,
    pub port: u16,
}

#[derive(Debug, Clone)]
pub struct RedirectRule {
    pub hosts: Vec<AllowedHost>,
    pub require_https: bool,
}

impl RedirectRule {
    /// The release rule: [`RELEASE_REDIRECT_HOSTS`] on 443, HTTPS only.
    pub fn release() -> Self {
        Self {
            hosts: RELEASE_REDIRECT_HOSTS
                .iter()
                .map(|host| AllowedHost {
                    host: (*host).to_owned(),
                    port: 443,
                })
                .collect(),
            require_https: true,
        }
    }

    /// Whether hop number `hop` (1 for the first redirect) to `target` may be followed.
    pub fn check(&self, target: &Url, hop: usize) -> Result<(), String> {
        if hop > MAX_HOPS {
            return Err(format!("more than {MAX_HOPS} redirects"));
        }
        if self.require_https && target.scheme() != "https" {
            return Err("a redirect left HTTPS".to_owned());
        }
        let listed = match (target.host_str(), target.port_or_known_default()) {
            (Some(host), Some(port)) => self
                .hosts
                .iter()
                .any(|allowed| allowed.port == port && allowed.host.eq_ignore_ascii_case(host)),
            _ => false,
        };
        if listed {
            Ok(())
        } else {
            Err("a redirect to a host outside the release list".to_owned())
        }
    }
}
