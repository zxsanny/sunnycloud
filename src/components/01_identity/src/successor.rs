//! The successor pair of a refresh rotation, sealed so that a repeat of the previous token inside the grace
//! window (a lost response) is answered with the same pair instead of creating or refusing anything.
//!
//! The key is SHA-256 of a domain label and the token that is rotated away. That token is 256 random bits and
//! the database keeps only its hash, so the database alone cannot open the ciphertext; the client that repeats
//! the token supplies the key. A token is rotated away once, so a key encrypts exactly one message and the
//! fixed nonce is never used twice with a key.

use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::error::IdentityError;

const KEY_DOMAIN: &[u8] = b"sunnycloud/session-successor/v1";

/// The tokens a rotation handed out, as text.
pub(crate) struct Pair {
    pub access: String,
    pub refresh: String,
}

fn cipher(previous_token: &str) -> ChaCha20Poly1305 {
    let digest = Sha256::new()
        .chain_update(KEY_DOMAIN)
        .chain_update(previous_token.as_bytes())
        .finalize();
    ChaCha20Poly1305::new(&Key::from(digest))
}

/// Seals `pair` for the rotation of `previous_token` in session `session`.
pub(crate) fn seal(
    previous_token: &str,
    session: Uuid,
    pair: &Pair,
) -> Result<Vec<u8>, IdentityError> {
    let plaintext = format!("{}\n{}", pair.access, pair.refresh);
    cipher(previous_token)
        .encrypt(
            &Nonce::default(),
            Payload {
                msg: plaintext.as_bytes(),
                aad: session.as_bytes(),
            },
        )
        .map_err(|_| IdentityError::SuccessorSeal)
}

/// The pair sealed for `previous_token`, or `None` when `sealed` was made for another token or session.
pub(crate) fn open(previous_token: &str, session: Uuid, sealed: &[u8]) -> Option<Pair> {
    let plaintext = cipher(previous_token)
        .decrypt(
            &Nonce::default(),
            Payload {
                msg: sealed,
                aad: session.as_bytes(),
            },
        )
        .ok()?;
    let text = String::from_utf8(plaintext).ok()?;
    let (access, refresh) = text.split_once('\n')?;
    Some(Pair {
        access: access.to_owned(),
        refresh: refresh.to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pair() -> Pair {
        Pair {
            access: "access-token".to_owned(),
            refresh: "refresh-token".to_owned(),
        }
    }

    #[test]
    fn a_sealed_pair_opens_only_for_the_token_and_session_it_was_sealed_for() {
        // Arrange
        let session = Uuid::from_u128(1);
        let sealed = seal("previous", session, &pair()).expect("seal");

        // Act
        let right = open("previous", session, &sealed);
        let other_token = open("another", session, &sealed);
        let other_session = open("previous", Uuid::from_u128(2), &sealed);

        // Assert
        let right = right.expect("opens");
        assert_eq!(
            (right.access.as_str(), right.refresh.as_str()),
            ("access-token", "refresh-token")
        );
        assert!(other_token.is_none());
        assert!(other_session.is_none());
    }

    #[test]
    fn the_ciphertext_does_not_contain_the_tokens() {
        // Arrange
        let sealed = seal("previous", Uuid::from_u128(1), &pair()).expect("seal");

        // Act
        let leaks = sealed
            .windows(b"refresh-token".len())
            .any(|window| window == b"refresh-token");

        // Assert
        assert!(!leaks);
    }

    #[test]
    fn a_damaged_ciphertext_does_not_open() {
        // Arrange
        let session = Uuid::from_u128(1);
        let mut sealed = seal("previous", session, &pair()).expect("seal");
        sealed[0] ^= 1;

        // Act
        let opened = open("previous", session, &sealed);

        // Assert
        assert!(opened.is_none());
    }
}
