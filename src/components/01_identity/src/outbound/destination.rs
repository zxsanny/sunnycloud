//! Where the server may send a provider request: `https` (`http` only with the development flag), and never to
//! a link-local or cloud-metadata destination. Private LAN and loopback addresses are allowed: an operator may
//! run the identity provider on the local network.

use std::net::{IpAddr, Ipv6Addr};

use reqwest::Url;

const METADATA_HOSTS: [&str; 3] = ["metadata.google.internal", "metadata.goog", "instance-data"];
/// The AWS IPv6 instance-metadata address; the IPv4 one is link-local and covered by that rule.
const AWS_METADATA_V6: Ipv6Addr = Ipv6Addr::new(0xfd00, 0x0ec2, 0, 0, 0, 0, 0, 0x0254);

/// Link-local (`169.254.0.0/16`, `fe80::/10`, which holds `169.254.169.254`) and the IPv6 metadata address.
pub(crate) fn address_refused(ip: IpAddr) -> bool {
    match ip.to_canonical() {
        IpAddr::V4(v4) => v4.is_link_local(),
        IpAddr::V6(v6) => v6.is_unicast_link_local() || v6 == AWS_METADATA_V6,
    }
}

/// The checks that need no DNS: scheme, an IP-literal host and the metadata host names. A host name that
/// resolves to a refused address is stopped by the resolver at connect time.
pub(crate) fn url_refused(url: &Url, allow_plain_http: bool) -> bool {
    let scheme_allowed = match url.scheme() {
        "https" => true,
        "http" => allow_plain_http,
        _ => false,
    };
    let Some(host) = url.host_str() else {
        return true;
    };
    let literal = host
        .trim_start_matches('[')
        .trim_end_matches(']')
        .parse::<IpAddr>();
    let host_refused = match literal {
        Ok(ip) => address_refused(ip),
        Err(_) => METADATA_HOSTS
            .iter()
            .any(|name| host.trim_end_matches('.').eq_ignore_ascii_case(name)),
    };
    !scheme_allowed || host_refused
}
