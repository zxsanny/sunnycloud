//! The trust anchor of the updater: the two pinned Ed25519 public keys and the release source. Both are
//! compile-time constants; nothing at run time (environment, settings file, manifest, server) can change them.

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use ed25519_dalek::{Signature, VerifyingKey};
use serde::{Deserialize, Serialize};

use super::verify::Rejection;

#[cfg(all(feature = "test-update-trust", not(debug_assertions)))]
compile_error!("the `test-update-trust` feature is for debug builds only");

/// The GitHub Releases source of the project (ADR-017): manifests and assets are resolved below it only.
#[cfg(not(any(test, feature = "test-update-trust")))]
pub(crate) const RELEASE_SOURCE: &str = "https://github.com/zxsanny/sunnycloud/releases";

/// The release keys are generated offline by the owner and are not provisioned yet: until both are pinned
/// here, every manifest is refused at the signature check and `update-sign` refuses to sign.
#[cfg(not(any(test, feature = "test-update-trust")))]
const PINNED: PinnedKeys = PinnedKeys {
    active: None,
    recovery: None,
};

/// Test build: the public halves of test keys A (active) and R (recovery) of DS-31. Their seeds are
/// derived in the tests from public labels; no real updater key is involved.
#[cfg(any(test, feature = "test-update-trust"))]
const PINNED: PinnedKeys = PinnedKeys {
    active: Some(hex32(
        "7cfc0cee6428cd8f691fb6842edb780bbdb891b7296ce353c4218633172abd74",
    )),
    recovery: Some(hex32(
        "b118a65e6661a983eb533e24b35c3c1b8b3e6ebfe45077dc2c0d3ad4597d4371",
    )),
};

/// Which pinned key signed a manifest; also the `key` hint of the signature file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Slot {
    Active,
    Recovery,
}

impl Slot {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Recovery => "recovery",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "active" => Some(Self::Active),
            "recovery" => Some(Self::Recovery),
            _ => None,
        }
    }

    fn other(self) -> Self {
        match self {
            Self::Active => Self::Recovery,
            Self::Recovery => Self::Active,
        }
    }
}

struct PinnedKeys {
    active: Option<[u8; 32]>,
    recovery: Option<[u8; 32]>,
}

/// The pinned public key of `slot`, or `None` while the build has no key for it.
pub fn pinned_key(slot: Slot) -> Option<VerifyingKey> {
    let bytes = match slot {
        Slot::Active => PINNED.active,
        Slot::Recovery => PINNED.recovery,
    }?;
    VerifyingKey::from_bytes(&bytes).ok()
}

/// `update-manifest.json.sig`: `key` is only a hint of which pinned key to try first.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SignatureFile {
    pub key: String,
    pub sig: String,
}

impl SignatureFile {
    pub fn new(slot: Slot, signature: &Signature) -> Self {
        Self {
            key: slot.as_str().to_owned(),
            sig: STANDARD.encode(signature.to_bytes()),
        }
    }
}

/// Verifies the detached signature over the exact manifest bytes against either pinned key, the hinted one
/// first. Nothing in the signature file or the manifest is ever used as key material.
pub fn verify_signature(manifest: &[u8], signature_file: &[u8]) -> Result<Slot, Rejection> {
    let file: SignatureFile =
        serde_json::from_slice(signature_file).map_err(|_| Rejection::BadSignature)?;
    let raw = STANDARD
        .decode(file.sig.as_bytes())
        .map_err(|_| Rejection::BadSignature)?;
    let signature = Signature::from_slice(&raw).map_err(|_| Rejection::BadSignature)?;
    let first = Slot::parse(&file.key).unwrap_or(Slot::Active);
    let mut provisioned = false;
    for slot in [first, first.other()] {
        if let Some(key) = pinned_key(slot) {
            provisioned = true;
            if key.verify_strict(manifest, &signature).is_ok() {
                return Ok(slot);
            }
        }
    }
    if provisioned {
        Err(Rejection::BadSignature)
    } else {
        Err(Rejection::KeysNotProvisioned)
    }
}

#[cfg(any(test, feature = "test-update-trust"))]
const fn hex32(text: &str) -> [u8; 32] {
    let bytes = text.as_bytes();
    let mut out = [0u8; 32];
    let mut i = 0;
    while i < 32 {
        out[i] = (nibble(bytes[2 * i]) << 4) | nibble(bytes[2 * i + 1]);
        i += 1;
    }
    out
}

#[cfg(any(test, feature = "test-update-trust"))]
const fn nibble(c: u8) -> u8 {
    match c {
        b'0'..=b'9' => c - b'0',
        b'a'..=b'f' => c - b'a' + 10,
        _ => panic!("not a lowercase hex digit"),
    }
}
