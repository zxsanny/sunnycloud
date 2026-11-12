//! `update-sign` (ADR-017 "Offline signing tool"): signs the exact bytes of `update-manifest.json` with the
//! seed in an owner-only key file and writes `<manifest>.sig`. The key is read only from `--key <file>`:
//! there is no code path for key material from the environment, an argument value or standard input.

use std::ffi::{OsStr, OsString};
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use ed25519_dalek::{Signer, SigningKey};
use thiserror::Error;
use uuid::Uuid;
use zeroize::Zeroizing;

use super::manifest::Manifest;
use super::trust::{SignatureFile, Slot, pinned_key};

/// Variables whose presence means the run is on CI. A guard, not a security boundary: CI holds no key.
const CI_VARIABLES: [&str; 2] = ["CI", "GITHUB_ACTIONS"];
const SEED_LEN: usize = 32;

#[derive(Debug, PartialEq, Eq)]
pub struct SignArgs {
    pub manifest: PathBuf,
    pub key: PathBuf,
    pub slot: Slot,
}

#[derive(Debug, Error)]
pub enum SignError {
    #[error("usage: update-sign --manifest <file> --key <seed file> --slot active|recovery ({0})")]
    Usage(String),
    #[error("refusing to sign on CI ({0} is set)")]
    OnCi(&'static str),
    #[error("the key file must be a regular file readable by its owner only")]
    KeyFilePermissions,
    #[error("the key file must hold exactly 32 bytes (an Ed25519 seed)")]
    KeyFileLength,
    #[error("the manifest is not a valid format-1 update manifest")]
    Manifest,
    #[error("this build pins no {0} key")]
    SlotNotPinned(&'static str),
    #[error("the key file does not match the pinned {0} key")]
    WrongKey(&'static str),
    #[error("{0}: {1}")]
    Io(String, io::Error),
}

impl SignError {
    /// 2 for a refused invocation, 1 for a failed signing.
    pub fn exit_code(&self) -> u8 {
        match self {
            Self::Usage(_) | Self::OnCi(_) => 2,
            _ => 1,
        }
    }
}

/// Parses `--manifest`, `--key` and `--slot`, each exactly once; anything else is refused.
pub fn parse_args(args: impl IntoIterator<Item = OsString>) -> Result<SignArgs, SignError> {
    let (mut manifest, mut key, mut slot) = (None, None, None);
    let mut args = args.into_iter();
    while let Some(flag) = args.next() {
        let flag = flag.to_str().map(str::to_owned).unwrap_or_default();
        let target = match flag.as_str() {
            "--manifest" => &mut manifest,
            "--key" => &mut key,
            "--slot" => &mut slot,
            other => return Err(SignError::Usage(format!("unknown argument {other:?}"))),
        };
        let value = args
            .next()
            .ok_or_else(|| SignError::Usage(format!("{flag} needs a value")))?;
        if target.replace(value).is_some() {
            return Err(SignError::Usage(format!("{flag} given twice")));
        }
    }
    let missing = |name: &str| SignError::Usage(format!("{name} is required"));
    let slot = slot.ok_or_else(|| missing("--slot"))?;
    let slot = slot
        .to_str()
        .and_then(Slot::parse)
        .ok_or_else(|| SignError::Usage("--slot is active or recovery".to_owned()))?;
    Ok(SignArgs {
        manifest: manifest.ok_or_else(|| missing("--manifest"))?.into(),
        key: key.ok_or_else(|| missing("--key"))?.into(),
        slot,
    })
}

/// Refuses when any CI variable is present, whatever its value.
pub fn refuse_on_ci(var: impl Fn(&str) -> Option<OsString>) -> Result<(), SignError> {
    match CI_VARIABLES.into_iter().find(|name| var(name).is_some()) {
        Some(name) => Err(SignError::OnCi(name)),
        None => Ok(()),
    }
}

/// Signs, verifies the signature against the pinned key of the slot, then writes `<manifest>.sig`.
pub fn sign(args: &SignArgs) -> Result<PathBuf, SignError> {
    let slot_name = args.slot.as_str();
    let pinned = pinned_key(args.slot).ok_or(SignError::SlotNotPinned(slot_name))?;
    let manifest = fs::read(&args.manifest).map_err(io_at(&args.manifest))?;
    Manifest::parse(&manifest).map_err(|_| SignError::Manifest)?;
    let key = SigningKey::from_bytes(&*read_seed(&args.key)?);
    let signature = key
        .try_sign(&manifest)
        .map_err(|_| SignError::WrongKey(slot_name))?;
    pinned
        .verify_strict(&manifest, &signature)
        .map_err(|_| SignError::WrongKey(slot_name))?;
    let body = serde_json::to_vec(&SignatureFile::new(args.slot, &signature))
        .map_err(|error| SignError::Io("signature".to_owned(), io::Error::other(error)))?;
    let target = signature_path(&args.manifest);
    write_atomically(&target, &body).map_err(io_at(&target))?;
    Ok(target)
}

pub fn signature_path(manifest: &Path) -> PathBuf {
    let mut name = manifest.as_os_str().to_owned();
    name.push(OsStr::new(".sig"));
    PathBuf::from(name)
}

fn read_seed(path: &Path) -> Result<Zeroizing<[u8; SEED_LEN]>, SignError> {
    let metadata = fs::symlink_metadata(path).map_err(io_at(path))?;
    if !metadata.is_file() || !owner_only(&metadata) {
        return Err(SignError::KeyFilePermissions);
    }
    let mut file = fs::File::open(path).map_err(io_at(path))?;
    let mut bytes = Zeroizing::new(Vec::with_capacity(SEED_LEN + 1));
    Read::by_ref(&mut file)
        .take(SEED_LEN as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(io_at(path))?;
    let seed: [u8; SEED_LEN] = bytes
        .as_slice()
        .try_into()
        .map_err(|_| SignError::KeyFileLength)?;
    Ok(Zeroizing::new(seed))
}

#[cfg(unix)]
fn owner_only(metadata: &fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;
    metadata.permissions().mode() & 0o077 == 0
}

/// Windows ACLs are not inspected: the tool refuses every key file there, so signing runs on a Unix machine.
#[cfg(not(unix))]
fn owner_only(_metadata: &fs::Metadata) -> bool {
    false
}

fn write_atomically(target: &Path, body: &[u8]) -> io::Result<()> {
    let mut partial = target.as_os_str().to_owned();
    partial.push(format!(".partial-{}", Uuid::new_v4()));
    let partial = PathBuf::from(partial);
    let written = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&partial)
        .and_then(|mut file| {
            file.write_all(body)?;
            file.sync_all()
        })
        .and_then(|()| fs::rename(&partial, target));
    match written {
        Err(error) => match fs::remove_file(&partial) {
            Err(cleanup) if cleanup.kind() != io::ErrorKind::NotFound => {
                Err(io::Error::other(format!(
                    "{error}; the partial file {} remains: {cleanup}",
                    partial.display()
                )))
            }
            _ => Err(error),
        },
        Ok(()) => Ok(()),
    }
}

fn io_at(path: &Path) -> impl Fn(io::Error) -> SignError + '_ {
    move |error| SignError::Io(path.display().to_string(), error)
}
