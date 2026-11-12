//! ST-04-5 and ST-04-6 (AC-8): account B never reaches account A's blobs. The catalog answers `None` for a
//! foreign id, a reference forged from A's id and B's key material opens nothing, the pending count is per
//! account, B's scope reads only B's rows, the sweepers (system scope) reach every account, and the runtime role
//! has no DDL on `blobs` or `blob_gc`.

use contracts::{
    BlobCatalog as _, BlobError, BlobRef, Due, GcQueue as _, GcReason, ScrubMode, Scrubber,
};
use sc_clock::Clock;
use sc_db::Scope;
use sc_testdb::API;
use sqlx::Connection;
use uuid::Uuid;

use crate::kit::{read_all, second_account, store};
use crate::support::{Rig, run_to_done};

const PERMISSION_DENIED: &str = "42501";

struct Accounts {
    rig: Rig,
    b: Uuid,
    a_kept: BlobRef,
    a_removed: Vec<Uuid>,
    b_kept: BlobRef,
    b_removed: Uuid,
}

impl Accounts {
    fn a(&self) -> Uuid {
        self.rig.account
    }
}

async fn schedule(rig: &Rig, account: Uuid, blob_id: Uuid) {
    rig.api
        .run(Scope::Account(account), async |tx| {
            rig.queue
                .schedule_in_tx(tx, blob_id, GcReason::Deleted, Due::Now)
                .await
        })
        .await
        .expect("schedule removal");
}

/// A keeps one blob and removes two; B keeps one and removes one.
async fn accounts() -> Accounts {
    let rig = Rig::new().await;
    let a = rig.account;
    let b = second_account(&rig).await;
    let a_kept = store(&rig, a, &vec![1; 70_000]).await;
    let mut a_removed = Vec::new();
    for seed in 2..4 {
        let blob = store(&rig, a, &vec![seed; 1000]).await;
        schedule(&rig, a, blob.blob_id).await;
        a_removed.push(blob.blob_id);
    }
    let b_kept = store(&rig, b, &vec![4; 70_000]).await;
    let b_removed = store(&rig, b, &vec![5; 1000]).await.blob_id;
    schedule(&rig, b, b_removed).await;
    Accounts {
        rig,
        b,
        a_kept,
        a_removed,
        b_kept,
        b_removed,
    }
}

async fn scoped_ids(rig: &Rig, account: Uuid, sql: &'static str) -> Vec<Uuid> {
    let mut ids: Vec<Uuid> = rig
        .api
        .run(Scope::Account(account), async |tx| {
            sqlx::query_scalar(sql).fetch_all(tx.conn()).await
        })
        .await
        .expect("scoped read");
    ids.sort();
    ids
}

fn sorted(mut ids: Vec<Uuid>) -> Vec<Uuid> {
    ids.sort();
    ids
}

#[tokio::test(flavor = "multi_thread")]
async fn b_gets_no_reference_to_a_blob_and_counts_only_its_own_pending_removals() {
    // Arrange
    let accounts = accounts().await;
    let rig = &accounts.rig;
    let forged = BlobRef {
        blob_id: accounts.a_kept.blob_id,
        ..accounts.b_kept.clone()
    };

    // Act
    let foreign = rig
        .catalog
        .get_ref(accounts.b, accounts.a_kept.blob_id)
        .await
        .expect("lookup");
    let own = rig
        .catalog
        .get_ref(accounts.a(), accounts.a_kept.blob_id)
        .await
        .expect("lookup");
    let read = read_all(&rig.reader, forged.blob_id, &forged).await;
    let pending_b = rig
        .queue
        .pending_for_account(accounts.b)
        .await
        .expect("pending b");
    let pending_a = rig
        .queue
        .pending_for_account(accounts.a())
        .await
        .expect("pending a");

    // Assert
    assert_eq!(foreign, None);
    assert_eq!(
        own.as_ref().map(|blob| blob.blob_id),
        Some(accounts.a_kept.blob_id)
    );
    assert!(
        matches!(read.open, Some(BlobError::KeyUnavailable)) && read.bytes.is_empty(),
        "{read:?}"
    );
    assert_eq!((pending_a, pending_b), (2, 1));
    accounts.rig.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn b_scope_reads_only_b_rows_and_the_sweepers_reach_every_account() {
    // Arrange
    let accounts = accounts().await;
    let rig = &accounts.rig;
    let scrub = rig.scrub(10);

    // Act
    let blobs = scoped_ids(rig, accounts.b, "SELECT id FROM blobs").await;
    let queued = scoped_ids(rig, accounts.b, "SELECT blob_id FROM blob_gc").await;
    let run = scrub
        .scrubber
        .start_run(ScrubMode::Full, None)
        .await
        .expect("scrub run");
    let (scrubbed, _) = run_to_done(scrub.scrubber.as_ref(), &run, 10).await;
    let swept = rig
        .sweeper(None)
        .run_cycle(rig.clock.now_utc())
        .await
        .expect("gc cycle");
    let left = rig.count("SELECT count(*) FROM blobs").await;

    // Assert
    assert_eq!(
        blobs,
        sorted(vec![accounts.b_kept.blob_id, accounts.b_removed])
    );
    assert_eq!(queued, vec![accounts.b_removed]);
    assert_eq!(scrubbed.checked, 2, "the committed blobs of A and B");
    assert_eq!(swept.removed, 3, "the due blobs of A and B");
    assert_eq!(swept.remaining_due, 0);
    assert_eq!(left, 2);
    assert_eq!(accounts.a_removed.len(), 2);
    drop(scrub);
    accounts.rig.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn the_runtime_role_cannot_alter_drop_truncate_or_index_the_blob_tables() {
    // Arrange
    let rig = Rig::new().await;
    let mut conn = rig.db.conn_as(API).await;
    let statements = [
        "ALTER TABLE blobs ADD COLUMN extra bigint",
        "ALTER TABLE blob_gc ADD COLUMN extra bigint",
        "ALTER TABLE blobs DISABLE ROW LEVEL SECURITY",
        "CREATE INDEX blobs_extra_idx ON blobs (created_at)",
        "TRUNCATE blob_gc",
        "DROP TABLE blobs",
    ];

    // Act
    let mut states = Vec::new();
    for statement in statements {
        let result = sqlx::raw_sql(statement).execute(&mut conn).await;
        states.push(match result {
            Ok(_) => "executed".to_owned(),
            Err(error) => error
                .as_database_error()
                .and_then(|error| error.code())
                .map_or_else(|| error.to_string(), |code| code.into_owned()),
        });
    }

    // Assert
    assert_eq!(states, [PERMISSION_DENIED; 6]);
    conn.close().await.expect("close api connection");
    rig.finish().await;
}
