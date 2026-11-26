//! Per-command configuration binding (E-22/c04).
//!
//! The loader, the key table and the per-key rules are `sc_config`'s, the cross-field rules of each section are
//! its owner's. This module only decides what a command asks of them: which table keys it binds, whether the
//! owners' validation runs, and which secret a command must find before it starts.

use std::ops::Deref;
use std::sync::Arc;

use media_api::MediaConfig;
use sc_config::{
    ApiSettings, ConfigError, ConfigSection, ConfigSources, Environment, KeyProblem, Reader,
    Reason, Settings, TABLE, load_keys,
};
use sc_db::{DbConfig, DbRole};
use sunnycloud_admin::AdminConfig;
use sunnycloud_blobstore::BlobstoreConfig;
use sunnycloud_identity::IdentityConfig;
use sunnycloud_quota::QuotaConfig;
use sunnycloud_sharing::ShareConfig;
use sunnycloud_sync_feed::FeedConfig;
use sunnycloud_transfer::TransferConfig;
use sunnycloud_tree::TreeConfig;

use crate::commands::{AdminCommand, Command, MigrateAction};
use crate::context::ServiceContext;
use crate::error::CliError;

const MIGRATOR_URL_KEY: &str = "db_migrator_url";
const API_URL_KEY: &str = "db_api_url";
/// The problem key of 01's rule that at least one sign-in provider is configured; it names no table key.
const PROVIDERS_RULE: &str = "providers";

/// The server settings every builder of the composition root reads. For `serve` (and every command without a key
/// list) they bind every key the table lists for `sunnycloud`, with the start validation of every owner run
/// inside the load, so the loader's problems and the owners' problems reach the operator in one refusal. For a
/// [`ConfigCheck::Services`] command they bind only that command's keys; every other key holds its default.
#[derive(Debug)]
pub struct ServeSettings(Settings);

impl Deref for ServeSettings {
    type Target = Settings;

    fn deref(&self) -> &Settings {
        &self.0
    }
}

impl ConfigSection for ServeSettings {
    const READER: Reader = Reader::Api;

    fn from_settings(settings: Settings) -> Self {
        Self(settings)
    }

    fn validate(&self, _environment: Environment) -> Vec<KeyProblem> {
        owner_problems(&self.0)
    }
}

/// Every server section that exposes a start validation. Sections without one (database, HTTP, vault) add
/// only what the loader checks per key.
fn owner_problems(settings: &Settings) -> Vec<KeyProblem> {
    [
        IdentityConfig::from_settings(settings).validate(),
        TreeConfig::from_settings(settings).validate(),
        TransferConfig::from_settings(settings).validate(),
        FeedConfig::from_settings(settings).validate(),
        ShareConfig::from_settings(settings).validate(),
        AdminConfig::from_settings(settings).validate(),
        QuotaConfig::from_settings(settings).validate(),
        BlobstoreConfig::from_settings(settings).validate(),
        MediaConfig::from_settings(settings).validate(),
    ]
    .concat()
}

/// The settings of a [`ConfigCheck::Services`] command: the owners' start validation runs as for `serve`, and the
/// loader keeps only the problems of the bound keys. 01's rule that a sign-in provider is configured binds `serve`
/// alone: no other command signs anyone in.
struct ScopedSettings(ServeSettings);

impl ConfigSection for ScopedSettings {
    const READER: Reader = Reader::Api;

    fn from_settings(settings: Settings) -> Self {
        Self(ServeSettings(settings))
    }

    fn validate(&self, _environment: Environment) -> Vec<KeyProblem> {
        owner_problems(&self.0)
            .into_iter()
            .filter(|problem| problem.key != PROVIDERS_RULE)
            .collect()
    }
}

/// The table keys one owner's configuration reads (its `from_settings`), for the commands that build only that
/// owner's services. `tests/command_config.rs` checks each list against the owner's source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyGroup {
    /// Helper 07's `DbConfig` as the API role reads it: every `db_*` key but the migrator secret.
    Database,
    /// 04's `BlobstoreConfig`: the master key ring, the blob root and the blob janitors.
    Blobstore,
    /// 01's `IdentityConfig`, provider keys included (each is optional).
    Identity,
}

impl KeyGroup {
    pub fn keys(self) -> &'static [&'static str] {
        match self {
            KeyGroup::Database => &[
                "db_api_url",
                "db_pool_max",
                "db_acquire_timeout",
                "db_idle_in_tx_timeout",
                "db_statement_timeout",
                "db_recovery_budget",
            ],
            KeyGroup::Blobstore => &[
                "blob_root",
                "blob_free_reserve",
                "blob_free_reserve_percent",
                "blob_io_threads",
                "blob_preallocate",
                "blob_read_ahead_chunks",
                "blob_sync_coalesce_delay",
                "master_key_file",
                "master_key_ring",
                "master_key_current",
                "retention",
                "gc_batch",
                "gc_claim_lease",
                "gc_cycle_budget",
                "gc_interval",
                "orphan_mode",
                "orphan_sweep_age",
                "orphan_sweep_interval",
                "scrub_cycle_budget",
                "scrub_interval",
                "scrub_max_rate",
                "scrub_pause_active_streams",
            ],
            KeyGroup::Identity => &[
                "public_url",
                "allowlist_emails",
                "allow_plain_http_dev",
                "access_token_ttl",
                "refresh_token_ttl",
                "refresh_reuse_grace",
                "session_cache_ttl",
                "session_last_seen_interval",
                "device_code_ttl",
                "device_poll_interval",
                "device_grant_ip_limit",
                "invalid_token_ip_limit",
                "oauth_state_ttl",
                "login_code_ttl",
                "invite_ttl",
                "setup_token_ttl",
                "provider_timeout",
                "geoip_db_path",
                "github_client_id",
                "provider_github_client_secret",
                "oidc_issuer",
                "oidc_client_id",
                "provider_oidc_client_secret",
                "apple_client_id",
                "apple_team_id",
                "apple_key_id",
                "apple_p8",
            ],
        }
    }
}

/// The groups of `key-check`, `admin keycheck-init` and `admin rewrap`: the API database pool, the audit writer
/// and 04's keyring (`root/storage.rs`).
const KEYRING: &[KeyGroup] = &[KeyGroup::Database, KeyGroup::Blobstore];
/// The groups of `admin setup-token|create|reset` and `admin invite ...`: the API database pool, the audit writer
/// and 01's invites (`root/domain.rs`), without the sign-in service.
const INVITES: &[KeyGroup] = &[KeyGroup::Database, KeyGroup::Identity];

/// What a command asks of configuration before its handler runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigCheck {
    /// Nothing is checked here: `version`, `openapi` and `healthcheck` use no section. A command that builds
    /// services and has no key list yet binds every key, as `serve` does, without the refusal up front.
    Unrestricted,
    /// The database keys of the migrator step, and the migrator secret must be present (`migrate`, `migrate run`,
    /// `migrate revert`).
    MigratorUrl,
    /// The database keys of either role: the migrator secret when present, else the API URL (`migrate status`).
    AnyDatabaseUrl,
    /// The keys of the owners whose services the command builds, plus `environment`; a key of another section
    /// never refuses the command.
    Services(&'static [KeyGroup]),
    /// Every key `sunnycloud` reads, bound and validated (`serve`).
    Server,
}

impl ConfigCheck {
    /// The table keys this check binds; keys outside the list never refuse the command.
    pub fn keys(self) -> Vec<&'static str> {
        match self {
            ConfigCheck::Unrestricted => Vec::new(),
            ConfigCheck::MigratorUrl => TABLE
                .iter()
                .filter(|spec| spec.read_by.contains(Reader::Api))
                .map(|spec| spec.key)
                .filter(|key| {
                    (key.starts_with("db_") && *key != API_URL_KEY) || *key == "environment"
                })
                .collect(),
            ConfigCheck::AnyDatabaseUrl => TABLE
                .iter()
                .filter(|spec| spec.read_by.contains(Reader::Api))
                .map(|spec| spec.key)
                .filter(|key| key.starts_with("db_") || *key == "environment")
                .collect(),
            ConfigCheck::Services(groups) => std::iter::once("environment")
                .chain(groups.iter().flat_map(|group| group.keys().iter().copied()))
                .collect(),
            ConfigCheck::Server => TABLE
                .iter()
                .filter(|spec| spec.read_by.contains(Reader::Api))
                .map(|spec| spec.key)
                .collect(),
        }
    }
}

impl Command {
    pub fn config_check(&self) -> ConfigCheck {
        match self {
            Command::Serve => ConfigCheck::Server,
            Command::Migrate(args) => match args.action {
                None | Some(MigrateAction::Run | MigrateAction::Revert { .. }) => {
                    ConfigCheck::MigratorUrl
                }
                Some(MigrateAction::Status) => ConfigCheck::AnyDatabaseUrl,
            },
            Command::KeyCheck => ConfigCheck::Services(KEYRING),
            Command::Admin { command } => match command {
                AdminCommand::KeycheckInit | AdminCommand::Rewrap(_) => {
                    ConfigCheck::Services(KEYRING)
                }
                AdminCommand::SetupToken
                | AdminCommand::Create
                | AdminCommand::Reset { .. }
                | AdminCommand::Invite { .. } => ConfigCheck::Services(INVITES),
                _ => ConfigCheck::Unrestricted,
            },
            _ => ConfigCheck::Unrestricted,
        }
    }
}

/// Runs the command's check; the first failure is a configuration error (exit 3).
pub(crate) async fn check(command: &Command, services: &ServiceContext) -> Result<(), CliError> {
    let check = command.config_check();
    match check {
        ConfigCheck::Unrestricted => Ok(()),
        ConfigCheck::MigratorUrl | ConfigCheck::AnyDatabaseUrl => {
            database(check, services).await?;
            Ok(())
        }
        ConfigCheck::Services(_) => {
            let sources = services.get::<ConfigSources>().await?;
            services.provide(load_scoped(&sources, &check.keys())?)
        }
        ConfigCheck::Server => {
            services.get::<ServeSettings>().await?;
            Ok(())
        }
    }
}

/// The settings of a [`ConfigCheck::Services`] command, bound with its `keys` only.
fn load_scoped(sources: &ConfigSources, keys: &[&str]) -> Result<Arc<ServeSettings>, ConfigError> {
    let scoped = load_keys::<ScopedSettings>(sources, keys)?;
    let scoped =
        Arc::into_inner(scoped).expect("load_keys hands over the only reference to its section");
    Ok(Arc::new(scoped.0))
}

/// The database a `migrate` command connects to and the role it connects as: the migrator secret when present;
/// without it, the API URL for [`ConfigCheck::AnyDatabaseUrl`] and a refusal naming the secret otherwise.
pub(crate) async fn database(
    check: ConfigCheck,
    services: &ServiceContext,
) -> Result<(DbConfig, DbRole), CliError> {
    let sources = services.get::<ConfigSources>().await?;
    let settings = load_keys::<ApiSettings>(&sources, &ConfigCheck::MigratorUrl.keys())?;
    let migrator = DbConfig::from_settings(&settings);
    if migrator.db_migrator_url.is_some() {
        return Ok((migrator, DbRole::Migrator));
    }
    if check != ConfigCheck::AnyDatabaseUrl {
        return Err(ConfigError::MissingSecret(MIGRATOR_URL_KEY.to_owned()).into());
    }
    let settings = load_keys::<ApiSettings>(&sources, &check.keys())?;
    Ok((DbConfig::from_settings(&settings), DbRole::Api))
}

/// The settings of `serve`, bound with every key of the server table.
pub(crate) fn load_serve(sources: &ConfigSources) -> Result<Arc<ServeSettings>, ConfigError> {
    load_keys::<ServeSettings>(sources, &ConfigCheck::Server.keys())
}

/// The text of a configuration refusal: every problem, and for a secret key whose value was present the
/// value shown as `***`. A problem never carries a value, so nothing else can leak.
pub(crate) fn describe(error: &ConfigError) -> String {
    let ConfigError::Invalid(problems) = error else {
        return error.to_string();
    };
    let listed: Vec<String> = problems
        .iter()
        .map(|problem| {
            if has_secret_value(problem) {
                format!("{problem} (value ***)")
            } else {
                problem.to_string()
            }
        })
        .collect();
    format!(
        "invalid configuration ({} problems): {}",
        problems.len(),
        listed.join("; ")
    )
}

fn has_secret_value(problem: &KeyProblem) -> bool {
    problem.reason != Reason::MissingRequired
        && TABLE
            .iter()
            .any(|spec| spec.secret && spec.key == problem.key)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    static NEXT: AtomicUsize = AtomicUsize::new(0);

    /// Loads `keys` the way `load` does from the environment of a process given `vars`, the secrets written to files
    /// under a fresh folder that is removed once the load has read them.
    fn scoped_load(
        vars: &[(&str, &str)],
        secrets: &[(&str, &str)],
        load: impl FnOnce(&ConfigSources) -> Result<Arc<ServeSettings>, ConfigError>,
    ) -> Result<Arc<ServeSettings>, ConfigError> {
        let dir = std::env::temp_dir().join(format!(
            "sc-scoped-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).expect("temp dir is writable");
        let mut env: BTreeMap<String, String> = vars
            .iter()
            .map(|(key, value)| (format!("SUNNYCLOUD_{key}"), (*value).to_owned()))
            .collect();
        for (key, value) in secrets {
            let path = dir.join(key);
            std::fs::write(&path, value).expect("secret file is writable");
            env.insert(
                format!("SUNNYCLOUD_{}_FILE", key.to_ascii_uppercase()),
                path.display().to_string(),
            );
        }
        let sources = ConfigSources {
            env: env.into_iter().collect(),
            ..ConfigSources::default()
        };
        let loaded = load(&sources);
        std::fs::remove_dir_all(&dir).expect("temp dir is removable");
        loaded
    }

    #[test]
    fn invite_keys_load_with_no_provider_and_a_bad_value_of_another_section() {
        // Arrange: no provider, no worker token, no blob root, an invalid blobstore value.
        let vars = [
            ("PUBLIC_URL", "https://cloud.example.org"),
            ("GC_INTERVAL", "soon"),
        ];
        let secrets = [("db_api_url", "postgres://api@127.0.0.1/sunnycloud")];

        // Act
        let loaded = scoped_load(&vars, &secrets, |sources| {
            load_scoped(sources, &ConfigCheck::Services(INVITES).keys())
        });

        // Assert
        let settings = loaded.expect("the invite commands' keys load");
        assert_eq!(settings.public_url, "https://cloud.example.org");
    }

    #[test]
    fn keyring_keys_still_refuse_a_bad_value_of_their_own_section() {
        // Arrange
        let vars = [("BLOB_ROOT", "/srv/blobs"), ("GC_INTERVAL", "soon")];
        let secrets = [
            ("db_api_url", "postgres://api@127.0.0.1/sunnycloud"),
            ("master_key_file", "/run/keys/master.key"),
        ];

        // Act
        let refused = scoped_load(&vars, &secrets, |sources| {
            load_scoped(sources, &ConfigCheck::Services(KEYRING).keys())
        });

        // Assert
        match refused {
            Err(ConfigError::Invalid(problems)) => {
                let keys: Vec<&str> = problems.iter().map(|p| p.key.as_str()).collect();
                assert_eq!(keys, ["gc_interval"]);
            }
            other => panic!("expected the gc_interval refusal, got {other:?}"),
        }
    }

    #[test]
    fn serve_keeps_the_rule_that_a_provider_is_configured() {
        // Arrange
        let vars = [("PUBLIC_URL", "https://cloud.example.org")];
        let secrets = [("db_api_url", "postgres://api@127.0.0.1/sunnycloud")];

        // Act
        let refused = scoped_load(&vars, &secrets, load_serve);

        // Assert
        match refused {
            Err(ConfigError::Invalid(problems)) => {
                assert!(problems.iter().any(|p| p.key == PROVIDERS_RULE));
            }
            other => panic!("expected a refusal naming the provider rule, got {other:?}"),
        }
    }
}
