//! The account lifecycle hook and the blob owner cleanup (AZ-3717), wired to the real 04 `GcQueue`, `BlobRegistry`
//! and `GcSweeper` on the test database and a scratch blob directory. Probes run as the superuser.

use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use contracts::{AccountLifecycleHook, BlobOwnerCleanup, Due, GcQueue as _, GcReason};
use sc_clock::{CancellationToken, ManualClock};
use sc_config::Secret;
use sc_db::Scope;
use sqlx::PgConnection;
use sunnycloud_blobstore::{BlobRegistry, BlobstoreConfig, GcQueue, GcSweeper, blob_path};
use sunnycloud_tree::PgTreeLifecycle;
use uuid::Uuid;

use crate::mutation_support::{Tree, close};
use crate::support::START;

const DAY: Duration = Duration::from_secs(86_400);

pub fn config(root: &std::path::Path) -> BlobstoreConfig {
    BlobstoreConfig {
        blob_root: Some(root.to_string_lossy().into_owned()),
        retention: 7 * DAY,
        orphan_sweep_age: DAY,
        orphan_mode: "report".to_owned(),
        orphan_sweep_interval: Duration::from_secs(3600),
        blob_free_reserve: 0,
        blob_free_reserve_percent: 100,
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
        master_key_file: Secret::new("unused".to_owned()),
        master_key_ring: None,
        master_key_current: None,
    }
}

pub struct Erase {
    pub tree: Tree,
    pub dir: PathBuf,
    pub config: BlobstoreConfig,
    pub clock: Arc<ManualClock>,
    pub queue: Arc<GcQueue>,
    pub hook: Arc<PgTreeLifecycle>,
}

impl Erase {
    pub async fn new() -> Erase {
        let tree = Tree::new().await;
        let dir = std::env::temp_dir().join(format!("sc-tree-erase-{}", Uuid::new_v4()));
        fs::create_dir_all(&dir).expect("blob root");
        let config = config(&dir);
        let clock = sc_testdb::clock_at_start();
        let api = tree.fixture.api.clone();
        let queue = Arc::new(GcQueue::new(&config, api.clone(), clock.clone()).expect("queue"));
        let hook = Arc::new(PgTreeLifecycle::new(api, queue.clone(), clock.clone()));
        Erase {
            tree,
            dir,
            config,
            clock,
            queue,
            hook,
        }
    }

    /// A new process: a fresh pool, queue and hook; nothing of the earlier ones is kept.
    pub async fn restart(&mut self) {
        let api = self.tree.fixture.db.api().await;
        let queue =
            Arc::new(GcQueue::new(&self.config, api.clone(), self.clock.clone()).expect("queue"));
        self.hook = Arc::new(PgTreeLifecycle::new(api, queue.clone(), self.clock.clone()));
        self.queue = queue;
    }

    pub async fn mark_deleting(&self) {
        self.tree
            .exec(&format!(
                "UPDATE accounts SET state = 'deleting' WHERE id = '{}'",
                self.tree.account
            ))
            .await;
    }

    pub async fn erase(&self, limit: u32) -> u64 {
        self.hook
            .erase_batch(self.tree.account, limit)
            .await
            .expect("erase batch")
            .remaining
    }

    /// The real sweeper with the tree as its only owner cleanup.
    pub fn sweeper(&self) -> GcSweeper {
        let registry =
            Arc::new(BlobRegistry::new(&self.config, self.clock.clone()).expect("registry"));
        let cleanup: Arc<dyn BlobOwnerCleanup> = self.hook.clone();
        GcSweeper::new(
            &self.config,
            self.tree.fixture.api.clone(),
            self.clock.clone(),
            registry,
            vec![cleanup],
            CancellationToken::new(),
        )
        .expect("sweeper")
    }

    pub async fn sweep(&self) -> u64 {
        self.sweeper()
            .run_cycle(START)
            .await
            .expect("gc cycle")
            .removed
    }

    /// `count` committed blob rows of the account, each with a file in the blob directory.
    pub async fn blob_files(&self, count: usize) -> Vec<Uuid> {
        let ids = self.tree.blobs(count).await;
        for &id in &ids {
            self.put_file(id);
        }
        ids
    }

    pub fn put_file(&self, id: Uuid) {
        let path = blob_path(&self.dir, id);
        fs::create_dir_all(path.parent().expect("blob folder")).expect("folders");
        fs::write(path, b"x").expect("blob file");
    }

    pub fn has_file(&self, id: Uuid) -> bool {
        blob_path(&self.dir, id).exists()
    }

    /// Queues `blobs` as a file delete would, due now.
    pub async fn queue_now(&self, blobs: &[Uuid]) {
        self.tree
            .fixture
            .api
            .run(Scope::Account(self.tree.account), async |tx| {
                for &blob in blobs {
                    self.queue
                        .schedule_in_tx(tx, blob, GcReason::Deleted, Due::Now)
                        .await?;
                }
                Ok::<_, contracts::GcQueueError>(())
            })
            .await
            .expect("queue blobs");
    }

    pub async fn of_account(&self, table: &str) -> i64 {
        self.tree
            .count(&format!(
                "SELECT count(*) FROM {table} WHERE account_id = '{}'",
                self.tree.account
            ))
            .await
    }

    pub async fn finish(self) {
        let Erase {
            tree,
            dir,
            queue,
            hook,
            ..
        } = self;
        drop((queue, hook));
        if let Err(error) = fs::remove_dir_all(&dir) {
            eprintln!("scratch {} not removed: {error}", dir.display());
        }
        tree.finish().await;
    }
}

/// The tree of the erase tests: 1,000 nodes (root, 9 folders, 90 subfolders, 900 files) and 600 revisions over
/// 600 blobs with files: 500 current, 50 replaced and 50 last revisions of tombstoned files; files 500 to 899
/// are tombstoned, those from 550 on with no revision left.
pub struct Seeded {
    pub blobs: Vec<Uuid>,
}

pub const NODES: i64 = 1000;
pub const REVISIONS: i64 = 600;

fn new_ids(count: usize) -> Vec<Uuid> {
    (0..count).map(|_| Uuid::new_v4()).collect()
}

impl Erase {
    pub async fn seed(&self) -> Seeded {
        let account = self.tree.account;
        let folders = new_ids(9);
        let subfolders = new_ids(90);
        let files = new_ids(900);
        let blobs = self.blob_files(600).await;
        let revisions = new_ids(600);
        let mut admin = self.tree.fixture.db.admin().await;
        let parents = vec![self.tree.root; 9];
        insert_nodes(&mut admin, account, "folder", &folders, &parents).await;
        let parents: Vec<Uuid> = (0..90).map(|i| folders[i / 10]).collect();
        insert_nodes(&mut admin, account, "folder", &subfolders, &parents).await;
        let parents: Vec<Uuid> = (0..900).map(|i| subfolders[i / 10]).collect();
        insert_nodes(&mut admin, account, "file", &files, &parents).await;
        // Revision i: files 0..500 current, then files 0..50 replaced, then files 500..550 last of a tombstone.
        let owners: Vec<Uuid> = (0..600)
            .map(|i| match i {
                0..500 => files[i],
                500..550 => files[i - 500],
                _ => files[i - 50],
            })
            .collect();
        sqlx::query(
            "INSERT INTO revisions (id, account_id, node_id, blob_id, size, content_hash, blocklist, created_at) \
             SELECT r, $4, n, b, 1, decode(repeat('07', 32), 'hex'), ''::bytea, 'epoch'::timestamptz \
             FROM unnest($1::uuid[], $2::uuid[], $3::uuid[]) AS t(r, n, b)",
        )
        .bind(&revisions)
        .bind(&owners)
        .bind(&blobs)
        .bind(account)
        .execute(&mut admin)
        .await
        .expect("insert revisions");
        let current: Vec<Uuid> = revisions[..500]
            .iter()
            .chain(&revisions[550..])
            .copied()
            .collect();
        let pointed: Vec<Uuid> = files[..550].to_vec();
        sqlx::query(
            "UPDATE nodes n SET current_revision_id = t.r FROM unnest($1::uuid[], $2::uuid[]) AS t(n, r) \
             WHERE n.id = t.n",
        )
        .bind(&pointed)
        .bind(&current)
        .execute(&mut admin)
        .await
        .expect("point files at their revisions");
        sqlx::query("UPDATE nodes SET deleted_at = 'epoch'::timestamptz WHERE id = ANY($1)")
            .bind(&files[500..])
            .execute(&mut admin)
            .await
            .expect("tombstone files");
        close(admin).await;
        Seeded { blobs }
    }
}

async fn insert_nodes(
    conn: &mut PgConnection,
    account: Uuid,
    kind: &str,
    ids: &[Uuid],
    parents: &[Uuid],
) {
    sqlx::query(
        "INSERT INTO nodes (id, account_id, parent_id, kind, name, name_key, name_key_version, created_at, updated_at) \
         SELECT id, $3, parent, $4, 'n' || ord, 'n' || ord, 1, 'epoch'::timestamptz, 'epoch'::timestamptz \
         FROM unnest($1::uuid[], $2::uuid[]) WITH ORDINALITY AS t(id, parent, ord)",
    )
    .bind(ids)
    .bind(parents)
    .bind(account)
    .bind(kind)
    .execute(conn)
    .await
    .expect("insert nodes");
}

/// The hook as the purger holds it.
pub fn as_hook(hook: &Arc<PgTreeLifecycle>) -> Arc<dyn AccountLifecycleHook> {
    hook.clone()
}
