//! A tiny GeoLite2-City-shaped MaxMind database, written at test time: an IPv6 tree with IPv4 under `::/96`,
//! English names only. No binary is committed.

use std::net::{IpAddr, Ipv6Addr};
use std::path::{Path, PathBuf};

use maxminddb_writer::Database;
use maxminddb_writer::metadata::IpVersion;
use maxminddb_writer::paths::IpAddrWithMask;
use serde::Serialize;

#[derive(Serialize)]
struct Names {
    en: &'static str,
}

#[derive(Serialize)]
struct Place {
    names: Names,
}

#[derive(Serialize)]
struct Record {
    #[serde(skip_serializing_if = "Option::is_none")]
    city: Option<Place>,
    #[serde(skip_serializing_if = "Option::is_none")]
    country: Option<Place>,
}

fn place(name: &'static str) -> Option<Place> {
    Some(Place {
        names: Names { en: name },
    })
}

/// Network, city, country. The private and loopback networks carry records on purpose: the reader must still
/// answer "unknown" for them.
const NETWORKS: [(&str, Option<&str>, Option<&str>); 8] = [
    ("203.0.113.0/24", Some("Kyiv"), Some("Ukraine")),
    ("198.51.100.0/24", Some("Lviv"), Some("Ukraine")),
    ("192.0.2.0/24", None, Some("Poland")),
    ("203.0.114.0/24", Some("Orphan"), None),
    ("2001:db8::/32", Some("Berlin"), Some("Germany")),
    ("10.0.0.0/8", Some("Intranet"), Some("Nowhere")),
    ("127.0.0.0/8", Some("Loopback"), Some("Nowhere")),
    ("fd00::/8", Some("Intranet"), Some("Nowhere")),
];

fn tree_path(network: &str) -> IpAddrWithMask {
    let parsed: IpAddrWithMask = network.parse().expect("fixture network");
    match parsed.addr {
        IpAddr::V4(v4) => IpAddrWithMask::new(
            IpAddr::V6(Ipv6Addr::from(u128::from(u32::from(v4)))),
            96 + parsed.mask,
        ),
        IpAddr::V6(_) => parsed,
    }
}

/// Writes the fixture database to `dir/GeoLite2-City.mmdb` and returns its path.
pub fn write_fixture(dir: &Path) -> PathBuf {
    let mut db = Database::default();
    db.metadata.ip_version = IpVersion::V6;
    db.metadata.database_type = "GeoLite2-City".to_owned();
    db.metadata.languages = vec!["en".to_owned()];
    db.metadata.binary_format_major_version = 2;
    for (network, city, country) in NETWORKS {
        let data = db
            .insert_value(Record {
                city: city.and_then(place),
                country: country.and_then(place),
            })
            .expect("serialize record");
        db.insert_node(tree_path(network), data);
    }
    let path = dir.join("GeoLite2-City.mmdb");
    let bytes = db.write_to(Vec::new()).expect("serialize fixture database");
    std::fs::write(&path, bytes).expect("write fixture database");
    path
}
