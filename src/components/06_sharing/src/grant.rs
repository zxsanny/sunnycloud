//! The access grant of a password link (decision 15): `HMAC-SHA-256(share_grant_key, token_hash ‖ expires_at)`,
//! the expiry as big-endian unix microseconds, the tag as URL-safe base64 without padding (cookie-safe, 11 sends
//! it back with its `expires_at`). Nothing is stored: a grant is checked by recomputing the tag for the link it
//! is presented to, so it opens that link only, until its expiry, and a revoke ends it because every content
//! request re-reads the link. The key and the values never reach a log or an error.

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use contracts::AccessGrant;
use hmac::{Hmac, KeyInit, Mac};
use sc_clock::Timestamp;
use sc_ids::{TokenHash, ct_eq};
use sha2::Sha256;

/// The length of every grant value: 32 tag bytes as unpadded base64.
pub(crate) const GRANT_VALUE_LEN: usize = 43;

/// The keyed HMAC, cloned per grant.
#[derive(Clone)]
pub(crate) struct GrantKey(Hmac<Sha256>);

impl GrantKey {
    /// HMAC takes a key of any length; the length rule of `share_grant_key` is `ShareConfig`'s.
    pub(crate) fn new(key: &[u8]) -> Result<Self, hmac::digest::InvalidLength> {
        Hmac::new_from_slice(key).map(GrantKey)
    }

    /// The grant for the link whose token hashes to `token_hash`, valid while `now < expires_at`.
    pub(crate) fn issue(&self, token_hash: &TokenHash, expires_at: Timestamp) -> AccessGrant {
        AccessGrant {
            value: self.tag(token_hash, expires_at),
            expires_at,
        }
    }

    /// Whether `grant` was issued for this link and is not expired at `now`. The expiry and the value's length
    /// are public, so they are checked first; the tag itself is compared in constant time.
    pub(crate) fn accepts(
        &self,
        token_hash: &TokenHash,
        grant: &AccessGrant,
        now: Timestamp,
    ) -> bool {
        if grant.expires_at <= now || grant.value.len() != GRANT_VALUE_LEN {
            return false;
        }
        let expected = self.tag(token_hash, grant.expires_at);
        ct_eq(expected.as_bytes(), grant.value.as_bytes())
    }

    fn tag(&self, token_hash: &TokenHash, expires_at: Timestamp) -> String {
        let mut mac = self.0.clone();
        mac.update(token_hash.as_bytes());
        mac.update(&expires_at.as_unix_micros().to_be_bytes());
        URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes())
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    const KEY: &[u8] = b"0123456789abcdef0123456789abcdef";
    const NOW: Timestamp = Timestamp::from_unix_micros(1_794_052_800_000_000);
    const TTL: Duration = Duration::from_millis(500);

    fn key() -> GrantKey {
        GrantKey::new(KEY).expect("any key length")
    }

    fn link(seed: u8) -> TokenHash {
        TokenHash::from_bytes([seed; 32])
    }

    fn expiry() -> Timestamp {
        NOW.checked_add(TTL).expect("in range")
    }

    #[test]
    fn a_grant_is_valid_only_for_its_own_link_and_before_its_expiry() {
        // Arrange
        let grant = key().issue(&link(1), expiry());
        let just_before = expiry()
            .checked_sub(Duration::from_micros(1))
            .expect("in range");

        // Act
        let own_link = key().accepts(&link(1), &grant, NOW);
        let last_moment = key().accepts(&link(1), &grant, just_before);
        let at_expiry = key().accepts(&link(1), &grant, expiry());
        let other_link = key().accepts(&link(2), &grant, NOW);

        // Assert
        assert_eq!(grant.value.len(), GRANT_VALUE_LEN);
        assert!(
            grant
                .value
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        );
        assert!(own_link && last_moment);
        assert!(!at_expiry && !other_link);
    }

    #[test]
    fn the_value_is_hmac_sha256_of_the_token_hash_and_the_big_endian_expiry() {
        // Arrange
        let mut message = link(1).as_bytes().to_vec();
        message.extend_from_slice(&expiry().as_unix_micros().to_be_bytes());
        let mut reference = Hmac::<Sha256>::new_from_slice(KEY).expect("key");
        reference.update(&message);
        let expected = URL_SAFE_NO_PAD.encode(reference.finalize().into_bytes());

        // Act
        let grant = key().issue(&link(1), expiry());

        // Assert
        assert_eq!(grant.value, expected);
        assert_eq!(grant.expires_at, expiry());
    }

    #[test]
    fn a_changed_expiry_another_key_or_any_flipped_bit_is_refused() {
        // Arrange
        let grant = key().issue(&link(1), expiry());
        let extended = AccessGrant {
            expires_at: expiry().checked_add(TTL).expect("in range"),
            ..grant.clone()
        };
        let other_key = GrantKey::new(b"another key of at least 32 bytes!!").expect("key");
        let flipped: Vec<AccessGrant> = (0..grant.value.len())
            .flat_map(|offset| (0..8).map(move |bit| (offset, bit)))
            .map(|(offset, bit)| {
                let mut bytes = grant.value.clone().into_bytes();
                bytes[offset] ^= 1 << bit;
                AccessGrant {
                    value: String::from_utf8_lossy(&bytes).into_owned(),
                    expires_at: grant.expires_at,
                }
            })
            .collect();

        // Act
        let extended_ok = key().accepts(&link(1), &extended, NOW);
        let other_key_ok = other_key.accepts(&link(1), &grant, NOW);
        let flipped_ok = flipped
            .iter()
            .filter(|mutant| key().accepts(&link(1), mutant, NOW))
            .count();

        // Assert
        assert!(!extended_ok && !other_key_ok);
        assert_eq!(flipped.len(), GRANT_VALUE_LEN * 8);
        assert_eq!(flipped_ok, 0);
    }

    #[test]
    fn empty_oversized_and_truncated_values_are_refused() {
        // Arrange
        let grant = key().issue(&link(1), expiry());
        let values = [
            String::new(),
            format!("{}A", grant.value),
            grant.value[..GRANT_VALUE_LEN - 1].to_owned(),
            "A".repeat(64 * 1024),
        ];

        // Act
        let accepted = values
            .into_iter()
            .map(|value| AccessGrant {
                value,
                expires_at: grant.expires_at,
            })
            .filter(|mutant| key().accepts(&link(1), mutant, NOW))
            .count();

        // Assert
        assert_eq!(accepted, 0);
    }

    #[test]
    fn the_tag_comparison_is_the_constant_time_helper() {
        // Arrange
        // `accepts` compares through `sc_ids::ct_eq` (subtle's `ConstantTimeEq`); this pins its behaviour on
        // equal-length values that differ only in the last byte.
        let grant = key().issue(&link(1), expiry());
        let mut last_changed = grant.value.clone().into_bytes();
        last_changed[GRANT_VALUE_LEN - 1] ^= 1;

        // Act
        let same = ct_eq(grant.value.as_bytes(), grant.value.as_bytes());
        let different = ct_eq(grant.value.as_bytes(), &last_changed);

        // Assert
        assert!(same);
        assert!(!different);
    }
}
