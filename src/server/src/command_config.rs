//! Per-command configuration binding (E-22/c04).
//!
//! The loader, the key table and the per-key rules are `sc_config`'s, the cross-field rules of each section are
//! its owner's. This module only decides what a command asks of them: which table keys it binds, whether the
//! owners' validation runs, and which secret a command must find before it starts.

use std::ops::Deref;

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

use crate::commands::{Command, MigrateAction};
use crate::context::ServiceContext;
use crate::error::CliError;

const MIGRATOR_URL_KEY: &str = "db_migrator_url";
const API_URL_KEY: &str = "db_api_url";

/// The settings of `serve`: every key the table lists for `sunnycloud`, and the start validation of every
/// owner run inside the load, so the loader's problems and the owners' problems reach the operator in one
/// refusal.
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

/// What a command asks of configuration before its handler runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigCheck {
    /// Nothing is checked here: `version`, `openapi` and `healthcheck` use no section, and for every other
    /// command the documents do not say which keys its builders read, so its task binds them.
    Unrestricted,
    /// The database keys of the migrator step, and the migrator secret must be present (`migrate`, `migrate run`,
    /// `migrate revert`).
    MigratorUrl,
    /// The database keys of either role: the migrator secret when present, else the API URL (`migrate status`).
    AnyDatabaseUrl,
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
        ConfigCheck::Server => {
            services.get::<ServeSettings>().await?;
            Ok(())
        }
    }
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
pub(crate) fn load_serve(
    sources: &ConfigSources,
) -> Result<std::sync::Arc<ServeSettings>, ConfigError> {
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
