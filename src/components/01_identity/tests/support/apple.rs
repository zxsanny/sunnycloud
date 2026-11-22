//! Sign in with Apple on the sign-in world: a throw-away team key generated at run time (never a real key, and
//! no key material in the repository), the provider entry that carries it, and the check the in-process
//! provider runs on the client-secret JWT of every Apple token request.

use std::sync::Arc;

use base64::Engine;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use ring::rand::SystemRandom;
use ring::signature::{
    ECDSA_P256_SHA256_FIXED, ECDSA_P256_SHA256_FIXED_SIGNING, EcdsaKeyPair, KeyPair,
    UnparsedPublicKey,
};
use sc_clock::{Clock, ManualClock};
use sc_config::Secret;
use serde_json::{Value, json};
use sunnycloud_identity::{IdentityConfig, ProviderConfig};

use super::provider::ClientSecretCheck;
use super::sign_in::{World, world_with};

pub const APPLE: &str = "apple";
pub const TEAM_ID: &str = "TEAM123456";
pub const KEY_ID: &str = "KEYID12345";
pub const SERVICES_ID: &str = "test.sunnycloud.services";
const AUDIENCE: &str = "https://appleid.apple.com";
/// The most Apple accepts between `iat` and `exp` of a client secret.
const MAX_LIFETIME_SECS: i64 = 15_777_000;

pub struct TeamKey {
    pub p8: String,
    public: Vec<u8>,
}

impl TeamKey {
    pub fn generate() -> TeamKey {
        let rng = SystemRandom::new();
        let pkcs8 = EcdsaKeyPair::generate_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, &rng)
            .expect("generate the team key");
        let pair = EcdsaKeyPair::from_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, pkcs8.as_ref(), &rng)
            .expect("load the team key");
        let (begin, end) = (
            ["-----BEGIN", "PRIVATE KEY-----"].join(" "),
            ["-----END", "PRIVATE KEY-----"].join(" "),
        );
        TeamKey {
            p8: format!("{begin}\n{}\n{end}\n", STANDARD.encode(pkcs8.as_ref())),
            public: pair.public_key().as_ref().to_vec(),
        }
    }

    /// The configured `apple` provider; its issuer is filled in by the world.
    pub fn provider(&self) -> ProviderConfig {
        ProviderConfig {
            key: APPLE.to_owned(),
            kind: APPLE.to_owned(),
            issuer: None,
            client_id: SERVICES_ID.to_owned(),
            client_secret: Secret::new(self.p8.clone()),
            team_id: Some(TEAM_ID.to_owned()),
            key_id: Some(KEY_ID.to_owned()),
        }
    }

    /// What Apple does with a client secret: ES256 signature by the team key, `kid`, `iss` = Team ID, `sub` =
    /// the Services ID, `aud`, a lifetime within 15,777,000 s and a validity window that holds at `clock`'s now.
    pub fn check(&self, clock: Arc<ManualClock>) -> ClientSecretCheck {
        let public = self.public.clone();
        Arc::new(move |secret: &str| {
            let [header, claims, signature]: [&str; 3] = secret
                .split('.')
                .collect::<Vec<_>>()
                .try_into()
                .map_err(|_| "not three segments".to_owned())?;
            let decode = |segment: &str| {
                URL_SAFE_NO_PAD
                    .decode(segment)
                    .map_err(|_| "not base64url".to_owned())
            };
            let (head, body): (Value, Value) = (
                serde_json::from_slice(&decode(header)?).map_err(|_| "header")?,
                serde_json::from_slice(&decode(claims)?).map_err(|_| "claims")?,
            );
            UnparsedPublicKey::new(&ECDSA_P256_SHA256_FIXED, &public)
                .verify(format!("{header}.{claims}").as_bytes(), &decode(signature)?)
                .map_err(|_| "signature".to_owned())?;
            let (iat, exp) = (body["iat"].as_i64(), body["exp"].as_i64());
            let now = clock.now_utc().as_unix_micros() / 1_000_000;
            let (Some(iat), Some(exp)) = (iat, exp) else {
                return Err("iat or exp missing".to_owned());
            };
            let valid = head == json!({"alg": "ES256", "kid": KEY_ID})
                && body["iss"] == TEAM_ID
                && body["sub"] == SERVICES_ID
                && body["aud"] == AUDIENCE
                && exp > iat
                && exp - iat <= MAX_LIFETIME_SECS
                && iat <= now
                && now < exp;
            valid.then_some(()).ok_or_else(|| "claims".to_owned())
        })
    }
}

/// A world whose providers are `apple` and a GitHub entry keyed `gh`, with the team key's check installed.
pub async fn apple_world(team: &TeamKey, change: impl FnOnce(&mut IdentityConfig)) -> World {
    let (kept, github) = (team.provider(), github_entry());
    let world = world_with(move |config| {
        config.providers = vec![kept, github];
        change(config);
    })
    .await;
    world
        .idp
        .check_client_secrets(team.check(world.harness.clock.clone()));
    world
}

fn github_entry() -> ProviderConfig {
    ProviderConfig {
        key: "gh".to_owned(),
        kind: "github".to_owned(),
        issuer: None,
        client_id: "gh-client".to_owned(),
        client_secret: Secret::new("provider-secret".to_owned()),
        team_id: None,
        key_id: None,
    }
}
