use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;

use axum::extract::{Request, State};
use axum::http::header::HeaderName;
use axum::middleware::Next;
use axum::response::Response;
use sc_observability::log_error;

use super::{Layers, internal_failure};
use crate::error::HttpServerError;

const X_FORWARDED_FOR: HeaderName = HeaderName::from_static("x-forwarded-for");

/// The address of the TCP peer, put into every request by the accept loop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PeerAddr(pub(crate) SocketAddr);

/// The address abuse limits are counted for: the proxy-set `X-Forwarded-For` entry when the peer is a trusted
/// proxy, otherwise the peer. Handlers read it with `Extension<ClientIp>`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClientIp(pub IpAddr);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Cidr {
    network: u128,
    prefix: u32,
    width: u32,
}

impl Cidr {
    fn parse(text: &str) -> Result<Cidr, HttpServerError> {
        let invalid = || {
            HttpServerError::InvalidConfig(format!(
                "trusted_proxies entry `{text}` is not an address with a prefix length (10.0.0.0/24)"
            ))
        };
        let (address, prefix) = text.trim().split_once('/').ok_or_else(invalid)?;
        let address: IpAddr = address.trim().parse().map_err(|_| invalid())?;
        let prefix: u32 = prefix.trim().parse().map_err(|_| invalid())?;
        let (network, width) = bits(address);
        if prefix > width {
            return Err(invalid());
        }
        Ok(Cidr {
            network,
            prefix,
            width,
        })
    }

    fn contains(&self, address: IpAddr) -> bool {
        let (value, width) = bits(address);
        if width != self.width {
            return false;
        }
        let host_bits = self.width - self.prefix;
        host_bits >= u128::BITS || (value ^ self.network) >> host_bits == 0
    }
}

fn bits(address: IpAddr) -> (u128, u32) {
    match address {
        IpAddr::V4(v4) => (u128::from(u32::from(v4)), 32),
        IpAddr::V6(v6) => (u128::from(v6), 128),
    }
}

/// The `trusted_proxies` ranges. Empty trusts nobody (default deny).
#[derive(Debug, Clone, Default)]
pub(crate) struct TrustedProxies(Vec<Cidr>);

impl TrustedProxies {
    pub(crate) fn parse(ranges: &[String]) -> Result<TrustedProxies, HttpServerError> {
        ranges
            .iter()
            .map(|range| Cidr::parse(range))
            .collect::<Result<Vec<_>, _>>()
            .map(TrustedProxies)
    }

    fn contains(&self, address: IpAddr) -> bool {
        self.0.iter().any(|range| range.contains(address))
    }

    /// The identity of the caller. A peer outside the ranges is the identity, whatever the header says. A
    /// trusted peer is a proxy that appended the address it saw, so the entries are read from the right and
    /// the first one that is not itself a trusted proxy is the client; entries further left are the client's
    /// own claims. A malformed entry, or a header with no such entry, falls back to the peer.
    pub(crate) fn resolve<'a>(
        &self,
        peer: IpAddr,
        forwarded: impl DoubleEndedIterator<Item = &'a str>,
    ) -> IpAddr {
        let peer = peer.to_canonical();
        if !self.contains(peer) {
            return peer;
        }
        for line in forwarded.rev() {
            for entry in line.rsplit(',') {
                let Ok(address) = entry.trim().parse::<IpAddr>() else {
                    return peer;
                };
                let address = address.to_canonical();
                if !self.contains(address) {
                    return address;
                }
            }
        }
        peer
    }
}

/// Layer 2.
pub(super) async fn client_ip(
    State(layers): State<Arc<Layers>>,
    mut request: Request,
    next: Next,
) -> Response {
    let Some(PeerAddr(peer)) = request.extensions().get::<PeerAddr>().copied() else {
        let request_id = layers.fresh_id();
        log_error!(
            "request reached the stack without a peer address",
            request_id = request_id
        );
        return internal_failure(request_id, "peer address missing");
    };
    // A header line that is not text is unreadable, so none of the header is believed.
    let lines: Vec<&str> = request
        .headers()
        .get_all(X_FORWARDED_FOR)
        .iter()
        .map(|value| value.to_str())
        .collect::<Result<_, _>>()
        .unwrap_or_default();
    let address = layers.trusted.resolve(peer.ip(), lines.into_iter());
    request.extensions_mut().insert(ClientIp(address));
    next.run(request).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn proxies() -> TrustedProxies {
        TrustedProxies::parse(&["10.0.0.0/24".to_owned(), "fd00::/8".to_owned()])
            .expect("ranges parse")
    }

    fn resolve(proxies: &TrustedProxies, peer: &str, forwarded: &[&str]) -> IpAddr {
        proxies.resolve(peer.parse().expect("peer"), forwarded.iter().copied())
    }

    #[test]
    fn a_peer_inside_the_range_is_believed_about_the_forwarded_client() {
        // Arrange
        let proxies = proxies();

        // Act
        let client = resolve(&proxies, "10.0.0.9", &["198.51.100.7"]);

        // Assert
        assert_eq!(client, "198.51.100.7".parse::<IpAddr>().expect("ip"));
    }

    #[test]
    fn a_peer_outside_the_range_keeps_its_own_address() {
        // Arrange
        let proxies = proxies();

        // Act
        let client = resolve(&proxies, "10.0.1.9", &["198.51.100.7"]);

        // Assert
        assert_eq!(client, "10.0.1.9".parse::<IpAddr>().expect("ip"));
    }

    #[test]
    fn an_empty_list_trusts_nobody() {
        // Arrange
        let proxies = TrustedProxies::parse(&[]).expect("empty list parses");

        // Act
        let client = resolve(&proxies, "10.0.0.9", &["198.51.100.7"]);

        // Assert
        assert_eq!(client, "10.0.0.9".parse::<IpAddr>().expect("ip"));
    }

    #[test]
    fn ipv6_peers_follow_the_same_rule_and_a_mapped_ipv4_peer_is_read_as_ipv4() {
        // Arrange
        let proxies = proxies();

        // Act
        let inside = resolve(&proxies, "fd12::1", &["2001:db8::5"]);
        let outside = resolve(&proxies, "2001:db8::9", &["198.51.100.7"]);
        let mapped = resolve(&proxies, "::ffff:10.0.0.9", &["198.51.100.7"]);

        // Assert
        assert_eq!(inside, "2001:db8::5".parse::<IpAddr>().expect("ip"));
        assert_eq!(outside, "2001:db8::9".parse::<IpAddr>().expect("ip"));
        assert_eq!(mapped, "198.51.100.7".parse::<IpAddr>().expect("ip"));
    }

    #[test]
    fn the_entry_the_proxy_appended_wins_over_the_clients_own_claims() {
        // Arrange
        let proxies = proxies();

        // Act
        let client = resolve(&proxies, "10.0.0.9", &["1.1.1.1, 198.51.100.7, 10.0.0.3"]);

        // Assert
        assert_eq!(client, "198.51.100.7".parse::<IpAddr>().expect("ip"));
    }

    #[test]
    fn a_later_header_line_is_read_before_an_earlier_one() {
        // Arrange
        let proxies = proxies();

        // Act
        let client = resolve(&proxies, "10.0.0.9", &["1.1.1.1", "198.51.100.7"]);

        // Assert
        assert_eq!(client, "198.51.100.7".parse::<IpAddr>().expect("ip"));
    }

    #[test]
    fn a_malformed_or_missing_forwarded_value_falls_back_to_the_peer() {
        // Arrange
        let proxies = proxies();
        let peer = "10.0.0.9".parse::<IpAddr>().expect("ip");

        // Act
        let malformed = resolve(&proxies, "10.0.0.9", &["198.51.100.7, not-an-ip"]);
        let missing = resolve(&proxies, "10.0.0.9", &[]);
        let only_proxies = resolve(&proxies, "10.0.0.9", &["10.0.0.3"]);

        // Assert
        assert_eq!(malformed, peer);
        assert_eq!(missing, peer);
        assert_eq!(only_proxies, peer);
    }

    #[test]
    fn a_range_without_a_prefix_or_with_a_prefix_too_long_is_refused() {
        // Arrange
        let ranges = ["10.0.0.1", "10.0.0.0/33", "nonsense/8", "fd00::/129"];

        // Act
        let results: Vec<_> = ranges
            .iter()
            .map(|range| TrustedProxies::parse(&[(*range).to_owned()]))
            .collect();

        // Assert
        assert!(
            results
                .iter()
                .all(|result| matches!(result, Err(HttpServerError::InvalidConfig(_))))
        );
    }

    #[test]
    fn a_zero_length_prefix_covers_the_whole_family_so_no_forwarded_entry_is_a_client() {
        // Arrange
        let proxies = TrustedProxies::parse(&["0.0.0.0/0".to_owned()]).expect("range parses");

        // Act
        let client = resolve(&proxies, "203.0.113.1", &["198.51.100.7"]);

        // Assert
        assert_eq!(client, "203.0.113.1".parse::<IpAddr>().expect("ip"));
    }
}
