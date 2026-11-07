//! Loads the API settings the way the server does. Secret files live in a folder of their own per load, removed
//! before the settings are returned, so a value can only have been read once, at load.

use std::fs;
use std::path::Path;
use std::sync::Arc;

use sc_config::{ApiSettings, ConfigError, ConfigSources, Environment, TABLE, load};
use uuid::Uuid;

pub const GRANT_KEY_FILE: &str = "share_grant_key";
pub const TEST_GRANT_KEY: &str = "test-grant-key-of-exactly-32-byt";
const _: () = assert!(TEST_GRANT_KEY.len() == 32);

/// What the grant key file holds in a load.
pub enum KeyFile<'a> {
    Absent,
    Bytes(&'a [u8]),
    /// A folder where the file should be: reading it fails.
    Directory,
    /// The key file named by `SUNNYCLOUD_SHARE_GRANT_KEY_FILE` is a folder; the default location is empty.
    DirectoryByVariable,
}

pub fn load_with_key(
    key: KeyFile<'_>,
    environment: Environment,
    extra: &[(&str, &str)],
) -> (ConfigSources, Result<Arc<ApiSettings>, ConfigError>) {
    let dir = std::env::temp_dir().join(format!("sc_sharing_secrets_{}", Uuid::new_v4().simple()));
    fs::create_dir_all(&dir).expect("create secrets dir");
    write_required_secrets(&dir);
    let mut sources = ConfigSources {
        secrets_dir: dir.clone(),
        environment,
        ..ConfigSources::default()
    };
    match key {
        KeyFile::Absent => {}
        KeyFile::Bytes(bytes) => fs::write(dir.join(GRANT_KEY_FILE), bytes).expect("write key"),
        KeyFile::Directory => fs::create_dir(dir.join(GRANT_KEY_FILE)).expect("create key dir"),
        KeyFile::DirectoryByVariable => {
            let folder = dir.join("key_as_folder");
            fs::create_dir(&folder).expect("create key dir");
            sources.env.insert(
                "SUNNYCLOUD_SHARE_GRANT_KEY_FILE".to_owned(),
                path_text(&folder),
            );
        }
    }
    let required = [
        ("SUNNYCLOUD_PUBLIC_URL", "https://sunnycloud.test"),
        ("SUNNYCLOUD_GITHUB_CLIENT_ID", "gh-client"),
        ("SUNNYCLOUD_BLOB_ROOT", "/var/lib/sunnycloud/blobs"),
    ];
    for (name, value) in required.iter().chain(extra) {
        sources.env.insert((*name).to_owned(), (*value).to_owned());
    }
    let loaded = load::<ApiSettings>(&sources);
    fs::remove_dir_all(&dir).expect("remove the secrets dir");
    (sources, loaded)
}

/// The production-environment load with only the grant key set, the way the defaults test needs it.
pub fn load_settings(key: &[u8], extra: &[(&str, &str)]) -> Arc<ApiSettings> {
    let (_, loaded) = load_with_key(KeyFile::Bytes(key), Environment::Production, extra);
    loaded.unwrap_or_else(|error| panic!("the test settings do not load: {error}"))
}

fn write_required_secrets(dir: &Path) {
    let needed = TABLE.iter().filter(|spec| {
        spec.secret
            && spec.key != GRANT_KEY_FILE
            && (spec.key == "provider_github_client_secret"
                || (spec.default.is_none() && !spec.optional))
    });
    for spec in needed {
        fs::write(dir.join(spec.key), "sample-secret\n").expect("write secret file");
    }
}

fn path_text(path: &Path) -> String {
    path.to_str().expect("utf-8 temp path").to_owned()
}
