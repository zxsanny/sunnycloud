use std::net::IpAddr;

use maxminddb::{MaxMindDbError, Reader, geoip2};

/// Resolves an address to a coarse place ("City, Country", or the country alone) from a local GeoIP database.
/// `None` means the place is unknown, whether the address is not in the database or the database is missing.
pub trait GeoLookup: Send + Sync {
    fn locate(&self, ip: IpAddr) -> Option<String>;
}

/// The production [`GeoLookup`] over a MaxMind database file (GeoLite2-City or compatible), English names.
///
/// A missing path or an unreadable or invalid file is not fatal: the service starts and every location is
/// unknown.
pub struct MaxmindGeoLookup {
    reader: Option<Reader<Vec<u8>>>,
}

impl MaxmindGeoLookup {
    pub fn open(path: Option<&str>) -> Self {
        let Some(path) = path else {
            return MaxmindGeoLookup { reader: None };
        };
        match Reader::open_readfile(path) {
            Ok(reader) => MaxmindGeoLookup {
                reader: Some(reader),
            },
            Err(error) => {
                tracing::warn!(
                    class = error_class(&error),
                    "GeoIP database unavailable, locations are unknown"
                );
                MaxmindGeoLookup { reader: None }
            }
        }
    }

    fn lookup(reader: &Reader<Vec<u8>>, ip: IpAddr) -> Result<Option<String>, MaxMindDbError> {
        let Some(record) = reader.lookup(ip)?.decode::<geoip2::City>()? else {
            return Ok(None);
        };
        Ok(
            match (record.city.names.english, record.country.names.english) {
                (Some(city), Some(country)) => Some(format!("{city}, {country}")),
                (None, Some(country)) => Some(country.to_owned()),
                _ => None,
            },
        )
    }
}

impl GeoLookup for MaxmindGeoLookup {
    fn locate(&self, ip: IpAddr) -> Option<String> {
        let reader = self.reader.as_ref()?;
        let ip = ip.to_canonical();
        if is_local(ip) {
            return None;
        }
        match Self::lookup(reader, ip) {
            Ok(place) => place,
            Err(error) => {
                tracing::warn!(class = error_class(&error), "GeoIP lookup failed");
                None
            }
        }
    }
}

fn is_local(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            v4.is_loopback() || v4.is_private() || v4.is_link_local() || v4.is_unspecified()
        }
        IpAddr::V6(v6) => {
            v6.is_loopback()
                || v6.is_unique_local()
                || v6.is_unicast_link_local()
                || v6.is_unspecified()
        }
    }
}

/// The error kind only: messages of the reader may carry offsets and file details.
fn error_class(error: &MaxMindDbError) -> &'static str {
    match error {
        MaxMindDbError::Io(_) => "io",
        MaxMindDbError::InvalidDatabase { .. } => "invalid database",
        MaxMindDbError::Decoding { .. } => "decoding",
        MaxMindDbError::ResourceLimit { .. } => "resource limit",
        _ => "other",
    }
}
