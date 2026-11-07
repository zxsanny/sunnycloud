//! A complete, valid `serve` configuration on disk: secrets as files, everything else as variables.

#![allow(dead_code)]

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};

pub const PLANTED: &str = "planted-db-password-7f3a9c";

static NEXT: AtomicU32 = AtomicU32::new(0);

pub struct Setup {
    pub dir: PathBuf,
    pub env: BTreeMap<String, String>,
}

impl Setup {
    /// No key at all; the caller adds what the command under test reads.
    pub fn empty() -> Self {
        let dir = std::env::temp_dir().join(format!(
            "sc-config-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&dir).expect("temp dir is writable");
        Self {
            dir,
            env: BTreeMap::new(),
        }
    }

    /// Every required key present: one GitHub provider, the API database URL (with a planted password
    /// pointing at `db_port` on the loopback), a 40-byte grant key, a master key file and a worker token.
    pub fn valid(db_port: u16) -> Self {
        let mut setup = Self::empty();
        setup.set("PUBLIC_URL", "https://cloud.example.org");
        setup.set("GITHUB_CLIENT_ID", "client-id");
        setup.set("BLOB_ROOT", &setup.dir.join("blobs").display().to_string());
        setup.secret("provider_github_client_secret", "github-secret");
        setup.secret(
            "db_api_url",
            &format!("postgres://sunnycloud:{PLANTED}@127.0.0.1:{db_port}/sunnycloud"),
        );
        setup.secret("master_key_file", "/run/keys/master.key");
        setup.secret("share_grant_key", &"g".repeat(40));
        setup.secret("worker_api_token", "worker-token");
        setup
    }

    pub fn set(&mut self, key: &str, value: &str) -> &mut Self {
        self.env.insert(
            format!("SUNNYCLOUD_{}", key.to_ascii_uppercase()),
            value.to_owned(),
        );
        self
    }

    pub fn remove(&mut self, key: &str) -> &mut Self {
        self.env
            .remove(&format!("SUNNYCLOUD_{}", key.to_ascii_uppercase()));
        self
    }

    /// Writes the secret to a file and points `SUNNYCLOUD_<KEY>_FILE` at it.
    pub fn secret(&mut self, key: &str, value: &str) -> &mut Self {
        let path = self.dir.join(key);
        fs::write(&path, value).expect("secret file is writable");
        self.set(&format!("{key}_FILE"), &path.display().to_string())
    }

    /// Drops a secret: the variable and the file, so the loader finds neither.
    pub fn without_secret(&mut self, key: &str) -> &mut Self {
        self.remove(&format!("{key}_FILE"));
        let path = self.dir.join(key);
        if path.exists() {
            fs::remove_file(&path).expect("secret file is removable");
        }
        self
    }
}

impl Drop for Setup {
    fn drop(&mut self) {
        drop(fs::remove_dir_all(&self.dir));
    }
}
