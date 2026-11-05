use std::fmt;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

use crate::error::RngUnavailable;
use crate::types::TokenHash;

const TOKEN_BYTES: usize = 32;
/// RFC 8628 section 5.1: 20 consonants without vowels, so a code cannot spell a word.
const USER_CODE_ALPHABET: &[u8; 20] = b"BCDFGHJKLMNPQRSTVWXZ";
const USER_CODE_LEN: usize = 8;
/// Bytes below this value map to a base-20 digit without bias (240 = 12 * 20).
const USER_CODE_UNBIASED_LIMIT: u8 = 240;

/// The token families. The kind travels with the token for the caller; it does not change the value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TokenKind {
    Session,
    Refresh,
    DeviceToken,
    DeviceCode,
    Invite,
    Setup,
    Share,
    MobileLoginCode,
}

/// A bearer secret as handed to its owner once. `Debug` and `Display` print a placeholder.
///
/// Store only [`sha256_token`] of it; read the value with [`RawToken::expose`] at the single place that
/// sends it to its owner.
pub struct RawToken {
    kind: TokenKind,
    value: String,
}

impl RawToken {
    /// Wraps a token received from outside (a request header, a form field) so it can be hashed.
    pub fn from_presented(kind: TokenKind, value: String) -> Self {
        Self { kind, value }
    }

    pub fn kind(&self) -> TokenKind {
        self.kind
    }

    /// The secret value, URL-safe base64 without padding.
    pub fn expose(&self) -> &str {
        &self.value
    }
}

impl AsRef<str> for RawToken {
    fn as_ref(&self) -> &str {
        &self.value
    }
}

impl fmt::Debug for RawToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RawToken")
            .field("kind", &self.kind)
            .field("value", &"<redacted>")
            .finish()
    }
}

impl fmt::Display for RawToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("<redacted>")
    }
}

/// The short code of the device grant: 8 characters of base-20, shown to a person, stored plain.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct UserCode(String);

impl UserCode {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for UserCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl AsRef<str> for UserCode {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

fn fill_random(buf: &mut [u8]) -> Result<(), RngUnavailable> {
    getrandom::fill(buf).map_err(|_| RngUnavailable)
}

/// A new token: 256 CSPRNG bits as URL-safe base64 without padding (43 characters).
pub fn new_token(kind: TokenKind) -> Result<RawToken, RngUnavailable> {
    let mut bytes = [0u8; TOKEN_BYTES];
    fill_random(&mut bytes)?;
    Ok(RawToken {
        kind,
        value: URL_SAFE_NO_PAD.encode(bytes),
    })
}

/// A new device-grant user code: 8 characters drawn uniformly from the RFC 8628 base-20 alphabet.
pub fn new_user_code() -> Result<UserCode, RngUnavailable> {
    let mut code = String::with_capacity(USER_CODE_LEN);
    let mut pool = [0u8; 2 * USER_CODE_LEN];
    while code.len() < USER_CODE_LEN {
        fill_random(&mut pool)?;
        for byte in pool
            .iter()
            .copied()
            .filter(|b| *b < USER_CODE_UNBIASED_LIMIT)
        {
            if code.len() == USER_CODE_LEN {
                break;
            }
            code.push(char::from(USER_CODE_ALPHABET[usize::from(byte % 20)]));
        }
    }
    Ok(UserCode(code))
}

/// SHA-256 of the token's text, the only form that is stored.
pub fn sha256_token(token: impl AsRef<str>) -> TokenHash {
    TokenHash::from_bytes(Sha256::digest(token.as_ref().as_bytes()).into())
}

/// Constant-time equality (`subtle`); slices of different length are unequal.
pub fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    a.ct_eq(b).into()
}
