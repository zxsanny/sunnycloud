//! The Sign in with Apple client secret: an ES256 JWT signed with the operator's `.p8` key. Apple accepts a
//! lifetime of up to 15,777,000 s; the server builds a new one for every code exchange with a short lifetime, so
//! it never holds a secret that could expire and the operator has no renewal step.

use base64::Engine;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use ring::rand::SystemRandom;
use ring::signature::{ECDSA_P256_SHA256_FIXED_SIGNING, EcdsaKeyPair};
use serde_json::json;
use thiserror::Error;

/// How long each generated secret is valid.
pub(super) const SECRET_LIFETIME_SECS: i64 = 300;
/// The `aud` claim Apple expects of a client secret.
pub(super) const AUDIENCE: &str = "https://appleid.apple.com";

/// Why a `.p8` file is unusable. Carries no part of the file.
#[derive(Debug, Error, PartialEq, Eq)]
pub(crate) enum KeyError {
    #[error("the key file is not PKCS#8 text in PEM or base64 form")]
    Encoding,
    #[error("the key is not an ECDSA P-256 PKCS#8 private key")]
    NotAnEcKey,
}

#[derive(Debug, Error)]
#[error("the client secret could not be signed")]
pub(super) struct SignError;

pub(super) struct ClientSecretSigner {
    key: EcdsaKeyPair,
    rng: SystemRandom,
    team_id: String,
    key_id: String,
    client_id: String,
}

impl ClientSecretSigner {
    /// `p8` is the text of Apple's key file; the PEM armour lines are optional.
    pub(super) fn new(
        p8: &str,
        team_id: &str,
        key_id: &str,
        client_id: &str,
    ) -> Result<Self, KeyError> {
        let body: String = p8
            .lines()
            .map(str::trim)
            .filter(|line| !line.starts_with("-----"))
            .collect();
        let der = STANDARD.decode(body).map_err(|_| KeyError::Encoding)?;
        let rng = SystemRandom::new();
        let key = EcdsaKeyPair::from_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, &der, &rng)
            .map_err(|_| KeyError::NotAnEcKey)?;
        Ok(ClientSecretSigner {
            key,
            rng,
            team_id: team_id.to_owned(),
            key_id: key_id.to_owned(),
            client_id: client_id.to_owned(),
        })
    }

    /// A secret issued at `issued_at_secs` (seconds since the epoch) that expires [`SECRET_LIFETIME_SECS`] later.
    pub(super) fn sign(&self, issued_at_secs: i64) -> Result<String, SignError> {
        let header = json!({"alg": "ES256", "kid": self.key_id});
        let claims = json!({
            "iss": self.team_id,
            "iat": issued_at_secs,
            "exp": issued_at_secs.saturating_add(SECRET_LIFETIME_SECS),
            "aud": AUDIENCE,
            "sub": self.client_id,
        });
        let signing_input = format!(
            "{}.{}",
            URL_SAFE_NO_PAD.encode(header.to_string()),
            URL_SAFE_NO_PAD.encode(claims.to_string())
        );
        let signature = self
            .key
            .sign(&self.rng, signing_input.as_bytes())
            .map_err(|_| SignError)?;
        Ok(format!(
            "{signing_input}.{}",
            URL_SAFE_NO_PAD.encode(signature.as_ref())
        ))
    }
}

#[cfg(test)]
mod tests {
    use ring::signature::{ECDSA_P256_SHA256_FIXED, KeyPair, UnparsedPublicKey};
    use serde_json::Value;

    use super::*;

    const TEAM: &str = "TEAM123456";
    const KEY_ID: &str = "KEYID12345";
    const CLIENT: &str = "test.services.id";
    /// The most Apple accepts between `iat` and `exp`.
    const APPLE_MAX_LIFETIME_SECS: i64 = 15_777_000;

    /// A throw-away key in the form of an Apple `.p8` file, and the public key that verifies its signatures.
    fn generated_p8() -> (String, Vec<u8>) {
        let rng = SystemRandom::new();
        let pkcs8 = EcdsaKeyPair::generate_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, &rng)
            .expect("generate a key");
        let pair = EcdsaKeyPair::from_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, pkcs8.as_ref(), &rng)
            .expect("load the key");
        let armour = ["-----BEGIN", "PRIVATE KEY-----"].join(" ");
        let end = ["-----END", "PRIVATE KEY-----"].join(" ");
        let p8 = format!("{armour}\n{}\n{end}\n", STANDARD.encode(pkcs8.as_ref()));
        (p8, pair.public_key().as_ref().to_vec())
    }

    fn parts(jwt: &str) -> (Value, Value, Vec<u8>, String) {
        let [header, claims, signature]: [&str; 3] = jwt
            .split('.')
            .collect::<Vec<_>>()
            .try_into()
            .expect("three segments");
        let decode = |segment: &str| URL_SAFE_NO_PAD.decode(segment).expect("base64url segment");
        (
            serde_json::from_slice(&decode(header)).expect("header json"),
            serde_json::from_slice(&decode(claims)).expect("claims json"),
            decode(signature),
            format!("{header}.{claims}"),
        )
    }

    #[test]
    fn the_secret_is_an_es256_jwt_with_the_documented_claims_and_a_signature_the_team_key_verifies()
    {
        // Arrange
        let (p8, public) = generated_p8();
        let signer = ClientSecretSigner::new(&p8, TEAM, KEY_ID, CLIENT).expect("signer");

        // Act
        let jwt = signer.sign(1_000).expect("sign");

        // Assert
        let (header, claims, signature, signing_input) = parts(&jwt);
        assert_eq!(header, json!({"alg": "ES256", "kid": KEY_ID}));
        assert_eq!(
            claims,
            json!({"iss": TEAM, "iat": 1_000, "exp": 1_300, "aud": AUDIENCE, "sub": CLIENT})
        );
        assert_eq!(signature.len(), 64, "fixed-size r || s");
        UnparsedPublicKey::new(&ECDSA_P256_SHA256_FIXED, &public)
            .verify(signing_input.as_bytes(), &signature)
            .expect("the team key verifies the signature");
    }

    #[test]
    fn the_lifetime_is_positive_and_within_what_apple_accepts() {
        // Arrange
        let lifetime = SECRET_LIFETIME_SECS;

        // Act
        let within = lifetime > 0 && lifetime <= APPLE_MAX_LIFETIME_SECS;

        // Assert
        assert!(within, "{lifetime}");
    }

    #[test]
    fn a_secret_issued_after_the_renewal_point_is_new_and_expires_later() {
        // Arrange
        let (p8, _) = generated_p8();
        let signer = ClientSecretSigner::new(&p8, TEAM, KEY_ID, CLIENT).expect("signer");

        // Act
        let first = signer.sign(1_000).expect("sign");
        let second = signer.sign(1_000 + SECRET_LIFETIME_SECS).expect("sign");

        // Assert
        let (_, first_claims, _, _) = parts(&first);
        let (_, second_claims, _, _) = parts(&second);
        assert_ne!(first, second);
        assert!(second_claims["iat"].as_i64() >= first_claims["exp"].as_i64());
    }

    #[test]
    fn a_key_file_that_is_not_an_ec_pkcs8_key_is_refused_without_echoing_it() {
        // Arrange
        let cases = [
            ("not base64 !!", KeyError::Encoding),
            ("aGVsbG8gd29ybGQ=", KeyError::NotAnEcKey),
            ("", KeyError::NotAnEcKey),
        ];

        for (text, expected) in cases {
            // Act
            let result = ClientSecretSigner::new(text, TEAM, KEY_ID, CLIENT);

            // Assert
            let error = result.err().expect("refused");
            assert_eq!(error, expected);
            assert!(!error.to_string().contains(text) || text.is_empty());
        }
    }
}
