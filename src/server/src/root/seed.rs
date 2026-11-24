//! `DevSeed` of 07 for `admin dev-seed` (AZ-3795): the ports beyond 07's own come from the builders every other
//! consumer uses, and the provider step is the HTTP login at the mock identity provider.

use std::sync::Arc;

use contracts::{DevSeed, MasterKeyring};
use sc_clock::Clock;
use sunnycloud_admin::{DevSeedImpl, DevSeedPorts, DevSeedTarget};

use super::domain::{Admin, Identity, Sharing, Transfer, Tree};
use super::failed;
use super::lists::AccountHooks;
use crate::command_config::ServeSettings;
use crate::context::ContextBuilder;
use crate::dev_seed_login::HttpProviderLogin;
use crate::error::CliError;

const SEED_PROVIDER_KIND: &str = "oidc";

pub(super) fn register(builder: ContextBuilder) -> ContextBuilder {
    builder.register::<dyn DevSeed, _, _>(|services| async move {
        let settings = services.get::<ServeSettings>().await?;
        let identity = services.get::<Identity>().await?;
        let tree = services.get::<Tree>().await?;
        let transfer = services.get::<Transfer>().await?;
        let sharing = services.get::<Sharing>().await?;
        let admin = services.get::<Admin>().await?;
        let clock = services.get::<dyn Clock>().await?;
        // 01 refuses a new account until its lifecycle hooks are registered; building the list registers them,
        // as `serve` does before it accepts a request.
        services.get::<AccountHooks>().await?;
        // The demo files are written encrypted: the ring is loaded and checked first, as `serve` does.
        services
            .get::<dyn MasterKeyring>()
            .await?
            .load_and_verify()
            .await
            .map_err(|error| failed("the master key ring", error))?;
        let provider = identity
            .config
            .providers
            .iter()
            .find(|provider| provider.kind == SEED_PROVIDER_KIND)
            .map(|provider| provider.key.clone())
            .ok_or_else(|| {
                CliError::Usage(
                    "admin dev-seed signs in through an `oidc` provider (the mock identity provider); none is configured"
                        .to_owned(),
                )
            })?;
        let target = DevSeedTarget {
            provider,
            host: public_host(&identity.config.public_url),
        };
        let ports = DevSeedPorts {
            operator: identity.invites.clone(),
            invites: identity.invites.clone(),
            sign_in: identity.sign_in.clone(),
            provider_login: Arc::new(HttpProviderLogin),
            authenticator: identity.authenticator.clone(),
            tree: tree.service.clone(),
            tree_reader: tree.reader.clone(),
            upload: transfer.service.clone(),
            commit: transfer.service.clone(),
            shares: sharing.service.clone(),
        };
        let seed = DevSeedImpl::new(settings.environment, target, admin.admin.clone(), ports, clock);
        let seed: Arc<dyn DevSeed> = Arc::new(seed);
        Ok(seed)
    })
}

/// The `host[:port]` of the public URL: what the sign-in compares with the host of a request.
fn public_host(public_url: &str) -> String {
    let rest = public_url
        .split_once("://")
        .map_or(public_url, |(_, rest)| rest);
    rest.split(['/', '?', '#'])
        .next()
        .unwrap_or(rest)
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::public_host;

    #[test]
    fn host_is_the_authority_of_the_public_url() {
        // Arrange
        let urls = [
            "https://drive.example",
            "http://127.0.0.1:8080/",
            "https://drive.example/app?x=1",
        ];

        // Act
        let hosts = urls.map(public_host);

        // Assert
        assert_eq!(hosts, ["drive.example", "127.0.0.1:8080", "drive.example"]);
    }
}
