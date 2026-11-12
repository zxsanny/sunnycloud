//! Account lifecycle hook (AZ-3925) on a real PostgreSQL 18: AC-1 (initialize writes nothing), AC-2 (erase
//! leaves A no vault and B's vault unchanged), AC-3 (repeated and interrupted erase), and the vault half of AC-4.
//! The hook runs as `sunnycloud`; fixtures and probes run as the schema owner.

use contracts::{AccountLifecycleHook, EraseProgress};
use sc_db::Scope;
use sqlx::{Connection, PgConnection};
use sunnycloud_vault_service::PgVaultLifecycle;
use uuid::Uuid;

use crate::harness::{Harness, account, account_with_folder, vault};

const LIMIT: u32 = 5000;

type VaultRow = (Uuid, Uuid, Vec<u8>);

struct Accounts {
    harness: Harness,
    hook: PgVaultLifecycle,
    a: Uuid,
    b: Uuid,
    root_of_a: Uuid,
}

async fn two_accounts_with_vaults() -> Accounts {
    let harness = Harness::new().await;
    let mut conn = harness.fixture_conn().await;
    let (a, b) = (Uuid::new_v4(), Uuid::new_v4());
    let root_of_a = account_with_folder(&mut conn, a).await;
    let root_of_b = account_with_folder(&mut conn, b).await;
    vault(&mut conn, a, root_of_a).await.expect("vault of a");
    vault(&mut conn, b, root_of_b).await.expect("vault of b");
    conn.close().await.expect("close fixture connection");
    let hook = PgVaultLifecycle::new(harness.api.clone());
    Accounts {
        harness,
        hook,
        a,
        b,
        root_of_a,
    }
}

impl Accounts {
    /// The hook holds a clone of the pool, so it goes first or its connections count as a leak.
    async fn finish(self) {
        let Accounts { harness, hook, .. } = self;
        drop(hook);
        harness.finish().await;
    }
}

async fn vault_rows(conn: &mut PgConnection, account: Uuid) -> Vec<VaultRow> {
    sqlx::query_as("SELECT id, root_node_id, header FROM vaults WHERE account_id = $1")
        .bind(account)
        .fetch_all(conn)
        .await
        .expect("read vault rows")
}

async fn erase_until_done(hook: &PgVaultLifecycle, account: Uuid) -> EraseProgress {
    loop {
        let progress = hook.erase_batch(account, LIMIT).await.expect("erase batch");
        if progress.remaining == 0 {
            return progress;
        }
    }
}

#[tokio::test]
async fn initialize_issues_no_write_and_returns_ok() {
    // Arrange
    let harness = Harness::new().await;
    let mut conn = harness.fixture_conn().await;
    let created = Uuid::new_v4();
    account(&mut conn, created).await;
    let hook = PgVaultLifecycle::new(harness.api.clone());

    // Act
    let write_id: Option<String> = harness
        .api
        .run(Scope::Account(created), async |tx| {
            hook.initialize(tx, created)
                .await
                .map_err(|error| error.to_string())?;
            // A transaction gets a transaction id only when it writes.
            sqlx::query_scalar("SELECT txid_current_if_assigned()::text")
                .fetch_one(tx.conn())
                .await
                .map_err(|error| error.to_string())
        })
        .await
        .expect("initialize in the caller's transaction");

    // Assert
    assert_eq!(write_id, None);
    assert!(vault_rows(&mut conn, created).await.is_empty());
    conn.close().await.expect("close fixture connection");
    drop(hook);
    harness.finish().await;
}

#[tokio::test]
async fn erase_of_a_leaves_a_no_vault_and_b_unchanged() {
    // Arrange
    let accounts = two_accounts_with_vaults().await;
    let mut conn = accounts.harness.fixture_conn().await;
    let b_before = vault_rows(&mut conn, accounts.b).await;

    // Act
    let progress = erase_until_done(&accounts.hook, accounts.a).await;

    // Assert
    assert_eq!(progress, EraseProgress { remaining: 0 });
    assert!(vault_rows(&mut conn, accounts.a).await.is_empty());
    assert_eq!(vault_rows(&mut conn, accounts.b).await, b_before);
    assert_eq!(b_before.len(), 1);
    conn.close().await.expect("close fixture connection");
    accounts.finish().await;
}

#[tokio::test]
async fn erase_repeated_after_completion_reports_nothing_remaining() {
    // Arrange
    let accounts = two_accounts_with_vaults().await;
    erase_until_done(&accounts.hook, accounts.a).await;

    // Act
    let again = accounts.hook.erase_batch(accounts.a, LIMIT).await;

    // Assert
    assert_eq!(again.expect("repeat erase"), EraseProgress { remaining: 0 });
    accounts.finish().await;
}

#[tokio::test]
async fn erase_of_an_account_without_a_vault_reports_nothing_remaining() {
    // Arrange
    let accounts = two_accounts_with_vaults().await;
    let never = Uuid::new_v4();

    // Act
    let progress = accounts.hook.erase_batch(never, LIMIT).await;

    // Assert
    assert_eq!(progress.expect("erase"), EraseProgress { remaining: 0 });
    accounts.finish().await;
}

#[tokio::test]
async fn erase_resumed_after_an_interrupted_run_finishes() {
    // Arrange: a run stopped after a batch that deleted nothing, leaving the row in place.
    let accounts = two_accounts_with_vaults().await;
    let stopped = accounts
        .hook
        .erase_batch(accounts.a, 0)
        .await
        .expect("empty batch");

    // Act
    let resumed = accounts.hook.erase_batch(accounts.a, LIMIT).await;

    // Assert
    assert_eq!(stopped, EraseProgress { remaining: 1 });
    assert_eq!(
        resumed.expect("resumed erase"),
        EraseProgress { remaining: 0 }
    );
    accounts.finish().await;
}

/// The vault half of AC-4 only: once this hook reports `remaining = 0`, no `vaults` row references the
/// account's root folder, so the foreign key cannot block 02's purge. The full hook-list run with 02's real
/// hook waits for AZ-3717.
#[tokio::test]
async fn erase_done_leaves_no_vault_referencing_the_root_folder() {
    // Arrange
    let accounts = two_accounts_with_vaults().await;
    let mut conn = accounts.harness.fixture_conn().await;

    // Act
    erase_until_done(&accounts.hook, accounts.a).await;

    // Assert
    let referencing: i64 =
        sqlx::query_scalar("SELECT count(*) FROM vaults WHERE root_node_id = $1")
            .bind(accounts.root_of_a)
            .fetch_one(&mut conn)
            .await
            .expect("count references");
    assert_eq!(referencing, 0);
    conn.close().await.expect("close fixture connection");
    accounts.finish().await;
}
