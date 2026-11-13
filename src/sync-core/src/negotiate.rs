//! Step 3 to 5 of `connect` (`sync_api.md`): what the client accepts from `ServerInfo` and the limits it derives.
//! Every number from the server is checked here, before any operation can size a buffer or a loop with it.

use std::time::Duration;

use sync_core_proto::{BLOCK_SIZE, MAX_CLIENT_LANES, ServerInfo};

use crate::client::ConnectError;
use crate::config::SyncConfig;

const PROTOCOL_BLOCK: u64 = BLOCK_SIZE as u64;
/// Accepted ranges. The server's own defaults are 500 entries, 1000 staged files and a 30 s long-poll (`server_info.md`).
const BATCH_ENTRIES: std::ops::RangeInclusive<u32> = 1..=10_000;
const STAGED_PER_ACCOUNT: std::ops::RangeInclusive<u32> = 1..=1_000_000;
/// Read as seconds (`server_info.md` open point 1); the server caps its own wait at 55 s.
const LONGPOLL_SECONDS: std::ops::RangeInclusive<u64> = 1..=300;
const FAMILIES: [&str; 3] = ["desktop", "android", "ios"];

/// What the server allows, after the clamps. Server limits win over the local settings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct Limits {
    /// `min(4, max_lanes)`.
    pub lanes: u32,
    pub batch_max_entries: u32,
    pub max_staged_per_account: u32,
    /// `min(sync.longpoll_wait, ServerInfo.longpoll_max_wait)`.
    pub longpoll_wait: Duration,
}

/// `MAJOR.MINOR.PATCH`, digits only.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Version(u64, u64, u64);

impl Version {
    pub(crate) fn parse(text: &str) -> Option<Self> {
        let mut parts = text.split('.').map(|part| {
            (!part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()))
                .then(|| part.parse::<u64>().ok())
                .flatten()
        });
        let version = Self(parts.next()??, parts.next()??, parts.next()??);
        parts.next().is_none().then_some(version)
    }
}

/// The client's own identity: the checked `X-Sunnycloud-Client` value and version.
pub(crate) fn identity(config: &SyncConfig) -> Result<(String, Version), ConnectError> {
    if !FAMILIES.contains(&config.client_family.as_str()) {
        return Err(ConnectError::InvalidConfig {
            field: "client_family",
        });
    }
    let version = Version::parse(&config.client_version).ok_or(ConnectError::InvalidConfig {
        field: "client_version",
    })?;
    Ok((
        format!("{}/{}", config.client_family, config.client_version),
        version,
    ))
}

/// The minimum for `family`: one semver for all, or `family=semver,...`. A family missing from the list has no
/// minimum (`server_info.md` open point 4).
fn minimum_for(spec: &str, family: &str) -> Result<Option<Version>, ConnectError> {
    let malformed = ConnectError::ProtocolMismatch {
        what: "min_client_version",
    };
    if !spec.contains('=') {
        return Version::parse(spec).map(Some).ok_or(malformed);
    }
    let mut found = None;
    for entry in spec.split(',') {
        let (name, version) = entry.split_once('=').ok_or_else(|| malformed.clone())?;
        let version = Version::parse(version.trim()).ok_or_else(|| malformed.clone())?;
        if name.trim() == family {
            found = Some(version);
        }
    }
    Ok(found)
}

fn upgrade(message: String) -> ConnectError {
    ConnectError::UpgradeRequired {
        message,
        update_url: None,
    }
}

pub(crate) fn negotiate(
    info: &ServerInfo,
    config: &SyncConfig,
    own: Version,
) -> Result<Limits, ConnectError> {
    let mismatch = |what| ConnectError::ProtocolMismatch { what };
    if info.block_size != PROTOCOL_BLOCK {
        return Err(mismatch("block_size"));
    }
    if info.small_file_max != PROTOCOL_BLOCK {
        return Err(mismatch("small_file_max"));
    }
    if !info.api_versions.iter().any(|version| version == "v1") {
        return Err(upgrade(
            "this server no longer serves the API version of this client; please upgrade"
                .to_owned(),
        ));
    }
    if let Some(minimum) = minimum_for(&info.min_client_version, &config.client_family)?
        && own < minimum
    {
        return Err(upgrade(format!(
            "this client ({}) is older than the minimum supported version ({}); please upgrade",
            config.client_version,
            info.min_client_version.trim()
        )));
    }
    if info.max_lanes == 0 {
        return Err(mismatch("max_lanes"));
    }
    if !BATCH_ENTRIES.contains(&info.batch_max_entries) {
        return Err(mismatch("batch_max_entries"));
    }
    if !STAGED_PER_ACCOUNT.contains(&info.max_staged_per_account) {
        return Err(mismatch("max_staged_per_account"));
    }
    if !LONGPOLL_SECONDS.contains(&info.longpoll_max_wait) {
        return Err(mismatch("longpoll_max_wait"));
    }
    Ok(Limits {
        lanes: MAX_CLIENT_LANES.min(info.max_lanes),
        batch_max_entries: info.batch_max_entries,
        max_staged_per_account: info.max_staged_per_account,
        longpoll_wait: config
            .longpoll_wait
            .min(Duration::from_secs(info.longpoll_max_wait)),
    })
}
