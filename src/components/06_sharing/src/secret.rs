//! The link secrets: the token handed to the owner once, and the optional password, stored as an Argon2id PHC
//! string with a fresh salt per link and checked against that string's own parameters. Neither value is logged or
//! put into an error.

use argon2::password_hash::Error as PhcError;
use argon2::{Algorithm, Argon2, Params, PasswordHasher, PasswordVerifier, Version};
use sc_ids::{RawToken, TokenKind, new_token};

use crate::error::SharingError;

/// The marker in front of every link token (description §5): the share kind's prefix of the shared token helper.
pub const SHARE_TOKEN_PREFIX: &str = TokenKind::Share.prefix();

const SALT_BYTES: usize = argon2::RECOMMENDED_SALT_LEN;

/// A new link token: the prefix and 256 random bits as URL-safe base64 (the shared token helper adds both).
pub(crate) fn new_share_token() -> Result<RawToken, SharingError> {
    new_token(TokenKind::Share).map_err(|_| SharingError::RandomUnavailable)
}

/// The Argon2id PHC string of `password` with the configured cost and a salt drawn for this link only.
pub(crate) fn hash_password(params: &Params, password: &str) -> Result<String, SharingError> {
    let mut salt = [0u8; SALT_BYTES];
    getrandom::fill(&mut salt).map_err(|_| SharingError::RandomUnavailable)?;
    let hasher = Argon2::new(Algorithm::Argon2id, Version::V0x13, params.clone());
    hasher
        .hash_password_with_salt(password.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(SharingError::PasswordHash)
}

/// Whether `password` matches the stored PHC string `phc`. The algorithm, version, cost and salt are the ones the
/// string carries (share create's, owner decision 2026-11-14), so a later change of `share_argon2_params` does not
/// lock existing links. A string that does not parse is `PasswordHash`, never a wrong password.
pub(crate) fn verify_password(phc: &str, password: &str) -> Result<bool, SharingError> {
    match Argon2::default().verify_password(password.as_bytes(), phc) {
        Ok(()) => Ok(true),
        Err(PhcError::PasswordInvalid) => Ok(false),
        Err(other) => Err(SharingError::PasswordHash(other)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_share_token_is_the_prefix_and_32_random_bytes() {
        // Arrange
        let encoded_len = 43;

        // Act
        let first = new_share_token().expect("token");
        let second = new_share_token().expect("token");

        // Assert
        let body = first
            .expose()
            .strip_prefix(SHARE_TOKEN_PREFIX)
            .expect("prefix");
        assert_eq!(body.len(), encoded_len);
        assert!(
            body.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        );
        assert_eq!(first.kind(), TokenKind::Share);
        assert_ne!(first.expose(), second.expose());
        assert!(!format!("{first:?}").contains(body));
    }

    #[test]
    fn a_password_hash_is_an_argon2id_phc_string_with_its_own_salt() {
        // Arrange
        let params = Params::new(8, 1, 1, None).expect("small test cost");

        // Act
        let first = hash_password(&params, "correct horse").expect("hash");
        let second = hash_password(&params, "correct horse").expect("hash");

        // Assert
        assert!(first.starts_with("$argon2id$v=19$m=8,t=1,p=1$"), "{first}");
        assert!(!first.contains("correct horse"));
        assert_ne!(first, second);
    }

    #[test]
    fn verification_uses_the_cost_stored_in_the_phc_string() {
        // Arrange
        let stored =
            hash_password(&Params::new(16, 2, 1, None).expect("cost"), "пароль 🔑").expect("hash");

        // Act
        let right = verify_password(&stored, "пароль 🔑").expect("verify");
        let wrong = verify_password(&stored, "пароль").expect("verify");
        let empty = verify_password(&stored, "").expect("verify");

        // Assert
        assert!(right);
        assert!(!wrong && !empty);
    }

    #[test]
    fn a_stored_string_that_does_not_parse_is_an_error_not_a_wrong_password() {
        // Arrange
        let broken = "$argon2id$v=19$m=8,t=1,p=1$not-base64!";

        // Act
        let outcome = verify_password(broken, "anything");

        // Assert
        assert!(
            matches!(outcome, Err(SharingError::PasswordHash(_))),
            "{outcome:?}"
        );
    }
}
