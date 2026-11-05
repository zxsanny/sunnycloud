use std::collections::BTreeMap;
use std::fmt;
use std::path::PathBuf;

use crate::value::Environment;

/// Where a load reads from. The crate reads nothing else, and keeps no global state.
#[derive(Clone)]
pub struct ConfigSources {
    /// Process variables (`std::env::vars().collect()` in a binary).
    pub env: BTreeMap<String, String>,
    /// Config files (JSON objects keyed by table key); later files win.
    pub files: Vec<PathBuf>,
    /// Default folder of secret files.
    pub secrets_dir: PathBuf,
    /// Starting value of the `environment` key; the file and `SUNNYCLOUD_ENVIRONMENT` override it.
    pub environment: Environment,
    /// `Some` in a release build: the values compiled into the binary for the release-fixed
    /// keys (updater source and keys, minimum API family, HTTPS-required flag). Env and file
    /// entries for those keys are ignored. `None` in debug builds and tests.
    pub release_fixed: Option<BTreeMap<String, String>>,
}

impl Default for ConfigSources {
    fn default() -> Self {
        Self {
            env: BTreeMap::new(),
            files: Vec::new(),
            secrets_dir: PathBuf::from("/run/secrets"),
            environment: Environment::Production,
            release_fixed: None,
        }
    }
}

/// Prints names only: a variable can hold a secret value.
impl fmt::Debug for ConfigSources {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ConfigSources")
            .field("env_names", &self.env.keys().collect::<Vec<_>>())
            .field("files", &self.files)
            .field("secrets_dir", &self.secrets_dir)
            .field("environment", &self.environment)
            .field("release_build", &self.release_fixed.is_some())
            .finish()
    }
}
