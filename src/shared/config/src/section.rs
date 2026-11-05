use std::ops::Deref;

use crate::error::{KeyProblem, Reason};
use crate::spec::Reader;
use crate::table::Settings;
use crate::value::Environment;

/// A typed view for one binary. `load` binds and validates the keys the reader reads.
pub trait ConfigSection: Sized + Send + Sync + 'static {
    const READER: Reader;

    fn from_settings(settings: Settings) -> Self;

    /// Cross-field checks owned by the section; per-key types and ranges are already checked.
    fn validate(&self, _environment: Environment) -> Vec<KeyProblem> {
        Vec::new()
    }
}

macro_rules! section {
    ($(#[$doc:meta])* $name:ident, $reader:expr, $validate:expr) => {
        $(#[$doc])*
        #[derive(Debug)]
        pub struct $name(Settings);

        impl Deref for $name {
            type Target = Settings;

            fn deref(&self) -> &Settings {
                &self.0
            }
        }

        impl ConfigSection for $name {
            const READER: Reader = $reader;

            fn from_settings(settings: Settings) -> Self {
                Self(settings)
            }

            fn validate(&self, environment: Environment) -> Vec<KeyProblem> {
                $validate(&self.0, environment)
            }
        }
    };
}

fn no_cross_field_checks(_: &Settings, _: Environment) -> Vec<KeyProblem> {
    Vec::new()
}

section!(
    /// Settings of `sunnycloud` (api and CLI).
    ApiSettings, Reader::Api, api_checks
);
section!(
    /// Settings of `sunnycloud-worker`.
    WorkerSettings, Reader::Worker, no_cross_field_checks
);
section!(
    /// Settings of `decoder-runner`.
    DecoderSettings, Reader::Decoder, no_cross_field_checks
);
section!(
    /// Settings of the desktop daemon and the mobile backup crate.
    ClientSettings, Reader::Clients, no_cross_field_checks
);

fn api_checks(settings: &Settings, environment: Environment) -> Vec<KeyProblem> {
    let mut problems = Vec::new();
    check_public_url(&settings.public_url, environment, &mut problems);
    check_sign_in_provider(settings, &mut problems);
    problems
}

/// `https` outside development. An empty value was already reported as missing or malformed.
fn check_public_url(url: &str, environment: Environment, problems: &mut Vec<KeyProblem>) {
    let lower = url.to_ascii_lowercase();
    let reason = if url.is_empty() || lower.starts_with("https://") {
        return;
    } else if lower.starts_with("http://") {
        if environment == Environment::Development {
            return;
        }
        Reason::ForbiddenInRelease
    } else {
        Reason::WrongType
    };
    problems.push(KeyProblem::new("public_url", reason));
}

/// At least one sign-in provider must be fully configured (FT-N-25); a half-configured one is
/// reported by the keys it lacks.
fn check_sign_in_provider(settings: &Settings, problems: &mut Vec<KeyProblem>) {
    let github = [
        ("github_client_id", settings.github_client_id.is_some()),
        (
            "provider_github_client_secret",
            settings.provider_github_client_secret.is_some(),
        ),
    ];
    let oidc = [
        ("oidc_issuer", settings.oidc_issuer.is_some()),
        ("oidc_client_id", settings.oidc_client_id.is_some()),
        (
            "provider_oidc_client_secret",
            settings.provider_oidc_client_secret.is_some(),
        ),
    ];
    let mut any_complete = false;
    let mut any_partial = false;
    for provider in [&github[..], &oidc[..]] {
        let missing: Vec<&str> = provider
            .iter()
            .filter(|(_, present)| !present)
            .map(|(key, _)| *key)
            .collect();
        if missing.is_empty() {
            any_complete = true;
        } else if missing.len() < provider.len() {
            any_partial = true;
            problems.extend(
                missing
                    .into_iter()
                    .map(|key| KeyProblem::new(key, Reason::MissingRequired)),
            );
        }
    }
    if !any_complete && !any_partial {
        problems.push(KeyProblem::new("github_client_id", Reason::MissingRequired));
        problems.push(KeyProblem::new("oidc_issuer", Reason::MissingRequired));
    }
}
