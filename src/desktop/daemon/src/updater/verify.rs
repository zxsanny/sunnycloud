//! The manifest checks in the daemon's order (ADR-017 "Where each updater check reads its input"):
//! signature, layout, expiry, replay, version, platform entry, source. Size and digest of the payload are
//! checked by the download.

use thiserror::Error;

use super::manifest::{
    LayoutError, Manifest, PlatformEntry, Version, is_plain_asset_name, parse_expires, parse_sha256,
};
use super::trust::{Slot, verify_signature};

/// Why an update was refused. [`Rejection::check`] is the name the WARN line carries.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum Rejection {
    #[error("the manifest has no signature file")]
    Unsigned,
    #[error("the signature does not verify against either pinned key")]
    BadSignature,
    #[error("this build pins no updater key")]
    KeysNotProvisioned,
    #[error("unknown manifest format")]
    UnknownFormat,
    #[error("malformed manifest")]
    Malformed,
    #[error("the manifest expired")]
    Expired,
    #[error("serial {serial} is not above the accepted serial {accepted}")]
    Replay { serial: u64, accepted: u64 },
    #[error("version {offered} is not above the installed {installed}")]
    NotNewer { offered: String, installed: String },
    #[error("the manifest has no entry for {0}")]
    NoPlatformEntry(String),
    #[error("the release source did not serve the file: {0}")]
    Source(String),
    #[error("the payload size does not match the signed size")]
    Size,
    #[error("the payload SHA-256 does not match the signed digest")]
    Digest,
}

impl Rejection {
    pub fn check(&self) -> &'static str {
        match self {
            Self::Unsigned | Self::BadSignature | Self::KeysNotProvisioned => "signature",
            Self::UnknownFormat | Self::Malformed => "layout",
            Self::Expired => "expiry",
            Self::Replay { .. } => "replay",
            Self::NotNewer { .. } => "version",
            Self::NoPlatformEntry(_) => "platform",
            Self::Source(_) => "source",
            Self::Size => "size",
            Self::Digest => "digest",
        }
    }
}

/// What the checks compare against: all of it local to the daemon.
pub struct VerifyContext<'a> {
    pub now_unix_micros: i64,
    /// The highest serial accepted so far (`meta`), `None` before the first update.
    pub accepted_serial: Option<u64>,
    pub installed: Version,
    pub platform: &'a str,
}

/// A manifest that passed every check before the download.
#[derive(Debug, Clone)]
pub struct Verified {
    pub slot: Slot,
    pub version: Version,
    pub serial: u64,
    pub entry: PlatformEntry,
    pub sha256: [u8; 32],
}

/// Runs the manifest checks in order and stops at the first failure. `signature` is `None` when the
/// release has no `.sig` file.
pub fn verify_manifest(
    manifest: &[u8],
    signature: Option<&[u8]>,
    context: &VerifyContext<'_>,
) -> Result<Verified, Rejection> {
    let signature = signature.ok_or(Rejection::Unsigned)?;
    let slot = verify_signature(manifest, signature)?;
    let parsed = Manifest::parse(manifest).map_err(|error| match error {
        LayoutError::UnknownFormat => Rejection::UnknownFormat,
        LayoutError::Malformed => Rejection::Malformed,
    })?;
    let expires = parse_expires(&parsed.expires).ok_or(Rejection::Malformed)?;
    if context.now_unix_micros >= expires {
        return Err(Rejection::Expired);
    }
    if let Some(accepted) = context.accepted_serial
        && parsed.serial <= accepted
    {
        return Err(Rejection::Replay {
            serial: parsed.serial,
            accepted,
        });
    }
    let version = Version::parse(&parsed.version).ok_or(Rejection::Malformed)?;
    if version <= context.installed {
        return Err(Rejection::NotNewer {
            offered: version.to_string(),
            installed: context.installed.to_string(),
        });
    }
    let entry = parsed
        .platforms
        .get(context.platform)
        .cloned()
        .ok_or_else(|| Rejection::NoPlatformEntry(context.platform.to_owned()))?;
    let sha256 = parse_sha256(&entry.sha256).ok_or(Rejection::Malformed)?;
    if !is_plain_asset_name(&entry.asset) {
        return Err(Rejection::Source(
            "the asset is not a plain file name".to_owned(),
        ));
    }
    Ok(Verified {
        slot,
        version,
        serial: parsed.serial,
        entry,
        sha256,
    })
}
