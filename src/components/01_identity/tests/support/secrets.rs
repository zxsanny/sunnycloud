//! One secrets folder per test process, filled once with a file for every secret the API section needs, so
//! parallel tests never read a half-written file.

use std::fs;
use std::path::PathBuf;
use std::sync::OnceLock;

use sc_config::TABLE;

pub fn shared_dir() -> PathBuf {
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    DIR.get_or_init(|| {
        let path = std::env::temp_dir().join(format!("sc_identity_secrets_{}", std::process::id()));
        fs::create_dir_all(&path).expect("create secrets dir");
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
