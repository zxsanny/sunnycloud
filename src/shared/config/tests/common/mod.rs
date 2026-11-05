#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicUsize, Ordering};

use sc_config::{
    ApiSettings, ClientSettings, ConfigError, ConfigSources, DecoderSettings, Environment,
    KeyProblem, KeySpec, Reader, Reason, TABLE, ValueKind, WorkerSettings, load,
};

pub const PUBLIC_URL: &str = "https://cloud.example";

/// A scratch folder removed on drop.
pub struct TempDir(PathBuf);

impl TempDir {
    pub fn new() -> Self {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "sc_config_test_{}_{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).expect("create temp dir");
        Self(path)
    }

    pub fn path(&self) -> &Path {
        &self.0
    }

    pub fn write(&self, name: &str, content: &str) -> PathBuf {
        let path = self.0.join(name);
        fs::write(&path, content).expect("write temp file");
        path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_dir_all(&self.0) {
            eprintln!("cannot remove {}: {error}", self.0.display());
        }
    }
}

pub fn set(sources: &mut ConfigSources, name: &str, value: &str) {
    sources.env.insert(name.to_owned(), value.to_owned());
}

/// Sets `SUNNYCLOUD_<KEY>`.
pub fn set_key(sources: &mut ConfigSources, key: &str, value: &str) {
    set(
        sources,
        &format!("SUNNYCLOUD_{}", key.to_ascii_uppercase()),
        value,
    );
}

/// One folder per process, filled once with a file for every required secret and the GitHub client secret, so parallel tests
/// never read a half-written file.
fn shared_secret_dir() -> PathBuf {
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    DIR.get_or_init(|| {
        let path = std::env::temp_dir().join(format!("sc_config_secrets_{}", std::process::id()));
        fs::create_dir_all(&path).expect("create secret dir");
        for spec in TABLE.iter().filter(|spec| {
            spec.secret
                && (spec.key == "provider_github_client_secret"
                    || (spec.default.is_none() && !spec.optional))
        }) {
            fs::write(path.join(spec.key), "sample-secret\n").expect("write secret file");
        }
        path
    })
    .clone()
}

/// A private secret folder with only the required secrets (no sign-in provider secret),
/// already set as the secret folder of `sources`.
pub fn without_github_provider(sources: &mut ConfigSources) -> TempDir {
    sources.env.retain(|name, _| !name.contains("GITHUB"));
    let dir = TempDir::new();
    for spec in TABLE
        .iter()
        .filter(|spec| spec.secret && spec.default.is_none() && !spec.optional)
    {
        dir.write(spec.key, "sample-secret\n");
    }
    sources.secrets_dir = dir.path().to_path_buf();
    dir
}

fn sample_value(spec: &KeySpec) -> &'static str {
    match spec.kind {
        ValueKind::Text if spec.secret => "sample-secret",
        _ => PUBLIC_URL,
    }
}

/// Sources in which `reader` loads cleanly: every required key has a value, and the API has
/// one sign-in provider. Secrets come from files in one per-process folder.
pub fn valid_sources(reader: Reader) -> ConfigSources {
    let mut sources = ConfigSources::default();
    for spec in TABLE
        .iter()
        .filter(|s| s.read_by.contains(reader) && s.default.is_none() && !s.optional)
    {
        if spec.secret {
            sources.secrets_dir = shared_secret_dir();
        } else {
            set_key(&mut sources, spec.key, sample_value(spec));
        }
    }
    if reader == Reader::Api {
        set_key(&mut sources, "github_client_id", "gh-client");
        sources.secrets_dir = shared_secret_dir();
    }
    sources
}

pub fn load_reader(reader: Reader, sources: &ConfigSources) -> Result<(), ConfigError> {
    match reader {
        Reader::Api => load::<ApiSettings>(sources).map(drop),
        Reader::Worker => load::<WorkerSettings>(sources).map(drop),
        Reader::Decoder => load::<DecoderSettings>(sources).map(drop),
        Reader::Clients => load::<ClientSettings>(sources).map(drop),
    }
}

pub const READERS: [Reader; 4] = [
    Reader::Api,
    Reader::Worker,
    Reader::Decoder,
    Reader::Clients,
];

pub fn problems_of(result: Result<(), ConfigError>) -> Vec<KeyProblem> {
    match result {
        Err(ConfigError::Invalid(problems)) => problems,
        other => panic!("expected Invalid, got {other:?}"),
    }
}

pub fn production() -> Environment {
    Environment::Production
}

pub fn problem(key: &str, reason: Reason) -> KeyProblem {
    KeyProblem::new(key, reason)
}
