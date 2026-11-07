use std::fs::{self, OpenOptions};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use contracts::async_trait;
use sc_clock::Timestamp;
use sqlx::PgConnection;
use sunnycloud_blobstore::{
    BlobstoreConfig, OrphanHook, OrphanHookPoint, OrphanStats, OrphanSweeper, blob_path,
};
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::rows::{DAY, Rig, account, config};

pub const AGE: Duration = Duration::from_millis(500);

/// A version 4 blob id in the prefix directory `prefix` (`0x0102` is `blobs/01/02`); `n` tells ids apart.
pub fn blob_id(prefix: u16, n: u32) -> Uuid {
    let mut bytes = [0u8; 16];
    bytes[0..2].copy_from_slice(&prefix.to_be_bytes());
    bytes[2..6].copy_from_slice(&n.to_be_bytes());
    bytes[6] = 0x40;
    bytes[8] = 0x80;
    Uuid::from_bytes(bytes)
}

fn system_time(at: Timestamp) -> SystemTime {
    let micros = u64::try_from(at.as_unix_micros()).expect("a timestamp after the epoch");
    UNIX_EPOCH + Duration::from_micros(micros)
}

pub struct Scene {
    pub rig: Rig,
    pub account: Uuid,
}

impl Scene {
    pub async fn new() -> Scene {
        let rig = Rig::new().await;
        let account = account(&rig).await;
        Scene { rig, account }
    }

    pub fn config(&self, mode: &str, age: Duration) -> BlobstoreConfig {
        BlobstoreConfig {
            orphan_mode: mode.to_owned(),
            orphan_sweep_age: age,
            ..config(&self.rig.root, 7 * DAY)
        }
    }

    pub fn sweeper(&self, mode: &str, age: Duration) -> OrphanSweeper {
        OrphanSweeper::new(
            &self.config(mode, age),
            self.rig.api.clone(),
            self.rig.clock.clone(),
        )
        .expect("sweeper")
    }

    pub fn hooked(&self, mode: &str, hook: Arc<dyn OrphanHook>) -> OrphanSweeper {
        self.sweeper(mode, AGE).with_hook(hook)
    }

    pub async fn run(sweeper: &OrphanSweeper, now: Timestamp) -> OrphanStats {
        sweeper.run_cycle(now).await.expect("cycle")
    }

    /// A file at the blob's path whose modification time is `ago` before `now`.
    pub fn file_aged(&self, id: Uuid, now: Timestamp, ago: Duration) {
        self.rig.put_file(id);
        OpenOptions::new()
            .write(true)
            .open(self.path(id))
            .expect("open the blob file")
            .set_modified(system_time(now) - ago)
            .expect("set the modification time");
    }

    pub async fn insert_rows(&self, ids: &[Uuid], state: &str) {
        let mut conn = self.rig.owner().await;
        insert_rows(&mut conn, self.account, ids, state).await;
        sqlx::Connection::close(conn).await.expect("close");
    }

    pub fn path(&self, id: Uuid) -> PathBuf {
        blob_path(&self.rig.root, id)
    }

    pub fn has_file(&self, id: Uuid) -> bool {
        self.path(id).exists()
    }

    pub fn all_exist(&self, ids: &[Uuid]) -> bool {
        ids.iter().all(|id| self.has_file(*id))
    }

    pub fn none_exist(&self, ids: &[Uuid]) -> bool {
        ids.iter().all(|id| !self.has_file(*id))
    }

    pub async fn finish(self) {
        self.rig.finish().await;
    }
}

async fn insert_rows(conn: &mut PgConnection, account: Uuid, ids: &[Uuid], state: &str) {
    sqlx::query(
        "INSERT INTO blobs (id, account_id, state, size, wrapped_key, master_key_id, created_at) \
         SELECT id, $2, $3, CASE WHEN $3 = 'uploading' THEN NULL ELSE 10 END, 'k'::bytea, 3, \
         'epoch'::timestamptz FROM unnest($1::uuid[]) AS t(id)",
    )
    .bind(ids)
    .bind(account)
    .bind(state)
    .execute(conn)
    .await
    .expect("insert blobs");
}

/// What the 130 files of AC-1 and AC-2 look like: 100 with a row, 20 orphans older than `AGE`, 10 younger.
pub struct Volume {
    pub rowed: Vec<Uuid>,
    pub old: Vec<Uuid>,
    pub young: Vec<Uuid>,
}

impl Volume {
    pub async fn build(scene: &Scene, now: Timestamp) -> Volume {
        let ids = |from: u32, count: u32| -> Vec<Uuid> {
            (from..from + count)
                .map(|n| blob_id(u16::try_from(n % 5).expect("prefix") + 1, n))
                .collect()
        };
        let volume = Volume {
            rowed: ids(0, 100),
            old: ids(100, 20),
            young: ids(120, 10),
        };
        scene.insert_rows(&volume.rowed, "committed").await;
        for id in volume.rowed.iter().chain(&volume.old) {
            scene.file_aged(*id, now, 2 * AGE);
        }
        for id in &volume.young {
            scene.file_aged(*id, now, AGE / 5);
        }
        volume
    }

    pub fn everything(&self) -> Vec<Uuid> {
        [&self.rowed, &self.old, &self.young]
            .into_iter()
            .flatten()
            .copied()
            .collect()
    }
}

pub enum Plan {
    /// Insert a committed row for this blob of this account when the re-check of the blob is about to run.
    RowBeforeRecheck(Uuid, Uuid),
    /// Take the table away when the prefix directory has just been listed.
    HideTableAfterListing(u16),
    /// Take the table away before the first re-check.
    HideTableBeforeRecheck,
}

/// Changes the database at a chosen point of a run, on a connection of its own.
pub struct Interference {
    conn: Mutex<PgConnection>,
    plan: Plan,
}

impl Interference {
    pub async fn new(scene: &Scene, plan: Plan) -> Arc<Interference> {
        Arc::new(Interference {
            conn: Mutex::new(scene.rig.db.admin().await),
            plan,
        })
    }

    pub async fn close(self: Arc<Self>) {
        let hook = Arc::into_inner(self).expect("hook still shared");
        sqlx::Connection::close(hook.conn.into_inner())
            .await
            .expect("close the hook connection");
    }

    async fn hide_table(&self) {
        let mut conn = self.conn.lock().await;
        sqlx::query("ALTER TABLE blobs RENAME TO blobs_hidden")
            .execute(&mut *conn)
            .await
            .expect("rename blobs");
    }
}

#[async_trait]
impl OrphanHook for Interference {
    async fn at(&self, point: OrphanHookPoint) {
        match (&self.plan, point) {
            (Plan::RowBeforeRecheck(id, account), OrphanHookPoint::BeforeRecheck(next))
                if *id == next =>
            {
                let mut conn = self.conn.lock().await;
                insert_rows(&mut conn, *account, &[*id], "committed").await;
            }
            (Plan::HideTableAfterListing(prefix), OrphanHookPoint::Listed(listed))
                if *prefix == listed =>
            {
                self.hide_table().await;
            }
            (Plan::HideTableBeforeRecheck, OrphanHookPoint::BeforeRecheck(_)) => {
                self.hide_table().await;
            }
            _ => {}
        }
    }
}

pub fn make_blocker(scene: &Scene, prefix: u16) -> PathBuf {
    let [high, low] = prefix.to_be_bytes();
    let folder = scene.rig.root.join("blobs").join(format!("{high:02x}"));
    fs::create_dir_all(&folder).expect("prefix folder");
    let blocker = folder.join(format!("{low:02x}"));
    fs::write(&blocker, b"not a directory").expect("blocker");
    blocker
}
