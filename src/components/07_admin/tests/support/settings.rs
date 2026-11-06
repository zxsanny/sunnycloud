use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

use sc_config::{ApiSettings, ConfigError, ConfigSources, Environment, TABLE, load};

/// One secrets folder per test process, filled once with a file for every secret the API section needs.
fn secrets_dir() -> PathBuf {
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    DIR.get_or_init(|| {
        let path = std::env::temp_dir().join(format!("sc_admin_secrets_{}", std::process::id()));
        fs::create_dir_all(&path).expect("create secrets dir");
        let needed = TABLE.iter().filter(|spec| {
            spec.secret
                && (spec.key == "provider_github_client_secret"
                    || (spec.default.is_none() && !spec.optional))
        });
        for spec in needed {
            fs::write(path.join(spec.key), "sample-secret\n").expect("write secret file");
        }
        path
    })
    .clone()
}

/// An API-section load with every required key present, plus `extra` variables.
pub fn load_settings(
    environment: Environment,
    extra: &[(&str, &str)],
) -> Result<Arc<ApiSettings>, ConfigError> {
    let mut sources = ConfigSources {
        secrets_dir: secrets_dir(),
        environment,
        ..ConfigSources::default()
    };
    let required = [
        ("SUNNYCLOUD_PUBLIC_URL", "https://sunnycloud.test"),
        ("SUNNYCLOUD_GITHUB_CLIENT_ID", "gh-client"),
    ];
    for (key, value) in required.iter().chain(extra) {
        sources.env.insert((*key).to_owned(), (*value).to_owned());
    }
    load::<ApiSettings>(&sources)
}
