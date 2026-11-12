//! The updater (14 §5 Updater, ADR-017, AC-49): checks the compiled-in GitHub Releases source for a manifest
//! signed offline, accepts it only after the signature (either pinned Ed25519 key), layout, expiry, replay,
//! version and platform checks, downloads this OS's asset by file name from the same source, checks size then
//! SHA-256, and stages the package for the user to open. [`sign`] is the offline `update-sign` tool.

mod fetch;
pub mod manifest;
mod redirect;
mod service;
pub mod sign;
#[cfg(test)]
mod tests;
mod trust;
mod verify;

pub use redirect::{AllowedHost, MAX_HOPS, RELEASE_REDIRECT_HOSTS, RedirectRule};
pub use service::{Build, STAGING_DIR_NAME, StagedUpdate, Updater, UpdaterConfig, UpdaterError};
pub use trust::{SignatureFile, Slot, pinned_key, verify_signature};
pub use verify::{Rejection, Verified, VerifyContext, verify_manifest};
