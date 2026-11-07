use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use contracts::{KeyError, Keyring, MasterKey};
use sc_audit::AuditWriter;
use sc_config::Secret;
use sc_db::Db;
use sc_testdb::{MIGRATOR, START, TestDb, clock_at_start};
use sqlx::Connection;
use sunnycloud_blobstore::{BlobstoreConfig, DataKey, MasterKeyring};
use uuid::Uuid;

pub const KEY_ONE: [u8; 32] = [0x11; 32];
pub const KEY_TWO: [u8; 32] = [0x22; 32];
pub const OTHER_KEY: [u8; 32] = [0x99; 32];

/// A volume folder and a folder of key files, removed on drop.
pub struct Scratch(pub PathBuf);

impl Scratch {
    pub fn new() -> Self {
        let dir = std::env::temp_dir().join(format!("sc-keyring-{}", Uuid::new_v4()));
        fs::create_dir_all(&dir).expect("scratch directory is created");
        Scratch(dir)
    }

    pub fn volume(&self) -> PathBuf {
        let volume = self.0.join("volume");
        fs::create_dir_all(&volume).expect("volume folder");
        volume
    }

    pub fn key_file(&self, name: &str, bytes: &[u8]) -> PathBuf {
        let path = self.0.join(name);
        fs::write(&path, bytes).expect("key file");
        path
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_dir_all(&self.0) {
            eprintln!("scratch {} not removed: {error}", self.0.display());
        }
    }
}

pub struct Env {
    pub db: TestDb,
    pub api: Db,
    pub scratch: Scratch,
    pub volume: PathBuf,
}

impl Env {
    pub async fn new() -> Env {
        let db = TestDb::clone_template().await;
        let api = db.api().await;
        let scratch = Scratch::new();
        let volume = scratch.volume();
        Env {
            db,
            api,
            scratch,
            volume,
        }
    }

    pub fn config(
        &self,
        key_file: &Path,
        ring: Option<&str>,
        current: Option<u64>,
    ) -> BlobstoreConfig {
        BlobstoreConfig {
            blob_root: Some(self.volume.to_string_lossy().into_owned()),
            retention: Duration::from_secs(7 * 86_400),
            orphan_sweep_age: Duration::from_secs(86_400),
            orphan_mode: "report".to_owned(),
            orphan_sweep_interval: Duration::from_secs(3600),
            blob_free_reserve: 0,
            gc_interval: Duration::from_secs(60),
            gc_batch: 100,
            gc_claim_lease: Duration::from_secs(60),
            gc_cycle_budget: Duration::from_secs(60),
            scrub_interval: Duration::from_secs(3600),
            scrub_cycle_budget: Duration::from_secs(60),
            scrub_max_rate: 1 << 20,
            scrub_pause_active_streams: 1,
            blob_sync_coalesce_delay: Duration::ZERO,
            blob_preallocate: false,
            blob_read_ahead_chunks: 1,
            blob_io_threads: None,
            master_key_file: Secret::new(key_file.to_string_lossy().into_owned()),
            master_key_ring: ring.map(str::to_owned),
            master_key_current: current,
        }
    }

    pub fn keyring(&self, config: BlobstoreConfig) -> MasterKeyring {
        let audit = AuditWriter::new(self.api.clone(), clock_at_start());
        MasterKeyring::new(config, self.api.clone(), audit)
    }

    /// A keyring on the key file `bytes`, with no ring.
    pub fn keyring_with_key(&self, bytes: &[u8]) -> MasterKeyring {
        let key = self.scratch.key_file("master.key", bytes);
        self.keyring(self.config(&key, None, None))
    }

    pub fn key_check_path(&self) -> PathBuf {
        self.volume.join(".keycheck")
    }

    /// Inserts `count` blob rows of one new account as the schema owner, each wrapped under `keyring`.
    pub async fn blob_rows(&self, keyring: &MasterKeyring, state: &str, count: usize) -> Vec<Uuid> {
        let mut rows = Vec::new();
        for _ in 0..count {
            let blob_id = Uuid::new_v4();
            let key = DataKey::generate().expect("data key");
            let (wrapped, key_id) = keyring.wrap_data_key(blob_id, &key).expect("wrap");
            rows.push((blob_id, wrapped, key_id));
        }
        self.insert_blobs(state, &rows).await;
        rows.into_iter().map(|(id, _, _)| id).collect()
    }

    pub async fn insert_blobs(&self, state: &str, rows: &[(Uuid, Vec<u8>, i16)]) {
        let mut conn = self.db.conn_as(MIGRATOR).await;
        let account = Uuid::new_v4();
        let at = START.as_unix_micros();
        sqlx::query(
            "INSERT INTO accounts (id, state, created_at, display_name, role) \
             VALUES ($1, 'active', 'epoch'::timestamptz + $2::bigint * interval '1 microsecond', 'Fixture', 'user')",
        )
        .bind(account)
        .bind(at)
        .execute(&mut conn)
        .await
        .expect("insert account");
        for (blob_id, wrapped, key_id) in rows {
            sqlx::query(
                "INSERT INTO blobs (id, account_id, state, size, wrapped_key, master_key_id, created_at) \
                 VALUES ($1, $2, $3, $4, $5, $6, 'epoch'::timestamptz + $7::bigint * interval '1 microsecond')",
            )
            .bind(blob_id)
            .bind(account)
            .bind(state)
            .bind((state == "committed").then_some(10_i64))
            .bind(wrapped)
            .bind(key_id)
            .bind(at)
            .execute(&mut conn)
            .await
            .expect("insert blob");
        }
        conn.close().await.expect("close owner connection");
    }

    pub async fn audit_rows(&self, filter: &str) -> i64 {
        self.db
            .admin_count(&format!("SELECT count(*) FROM audit_log WHERE {filter}"))
            .await
    }

    /// A file below `blobs/`, as a blob file would be.
    pub fn blob_file(&self) {
        let dir = self.volume.join("blobs").join("ab").join("cd");
        fs::create_dir_all(&dir).expect("blob folders");
        fs::write(dir.join("abcd0000-0000-4000-8000-000000000000"), b"x").expect("blob file");
    }

    pub async fn finish(self) {
        let Env { db, api, .. } = self;
        drop(api);
        assert_eq!(db.finish().await, 0, "connections left open");
    }
}

pub fn candidate(bytes: &[u8]) -> MasterKey {
    MasterKey(bytes.to_vec())
}

pub fn ids(keyring: &Keyring) -> (i16, Vec<i16>) {
    (keyring.current_key_id, keyring.key_ids.clone())
}

pub fn is_start_refusal(error: &KeyError) -> bool {
    matches!(
        error,
        KeyError::FileMissing
            | KeyError::WrongLength
            | KeyError::KeyCheckMismatch
            | KeyError::KeyCheckAbsentButBlobsExist
    )
}
