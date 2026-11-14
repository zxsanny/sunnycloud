//! Shared pieces of the account erase tests (AZ-3757 AC-1, AC-2): the hook over the rig's service and the real 04
//! GC queue, the account moved to `deleting`, and probes of the queued blobs and of the rows the erase removes.

use std::sync::Arc;

use sc_clock::Clock;
use sunnycloud_blobstore::GcQueue;
use sunnycloud_transfer::PgTransferLifecycle;
use uuid::Uuid;

use crate::rig_config::blob_config;
use crate::start_support::Rig;

impl Rig {
    /// The erase hook over the current service instance and the real GC queue on the rig's database and clock.
    pub fn lifecycle(&self) -> PgTransferLifecycle {
        let key_file = self.volume.with_file_name("master.key");
        let clock: Arc<dyn Clock> = self.clock.clone();
        let gc = GcQueue::new(
            &blob_config(&self.volume, &key_file, 0),
            self.fixture.api.clone(),
            clock,
        )
        .expect("gc queue");
        PgTransferLifecycle::new(Arc::clone(&self.service), Arc::new(gc))
    }

    /// What 01's `begin_deletion` leaves for the guard: the account in `deleting`.
    pub async fn begin_deletion(&self) {
        self.exec(&format!(
            "UPDATE accounts SET state = 'deleting' WHERE id = '{}'",
            self.account
        ))
        .await;
    }

    /// The blobs of the account's open session and staged rows.
    pub async fn open_session_blobs(&self) -> Vec<Uuid> {
        let mut conn = self.owner().await;
        let blobs = sqlx::query_scalar(
            "SELECT blob_id FROM upload_sessions WHERE account_id = $1 \
             AND committed_revision_id IS NULL ORDER BY blob_id",
        )
        .bind(self.account)
        .fetch_all(&mut conn)
        .await
        .expect("open session blobs");
        crate::start_support::close(conn).await;
        blobs
    }

    /// Blobs of the account that are not committed and have neither a session row nor an `account_deleted` queue
    /// row due by now: a deleted row whose blob was not queued with it.
    pub async fn unqueued_blobs(&self) -> i64 {
        self.count(&format!(
            "SELECT count(*) FROM blobs b WHERE b.account_id = '{account}' AND b.state <> 'committed' \
             AND NOT EXISTS (SELECT 1 FROM upload_sessions s WHERE s.blob_id = b.id) \
             AND NOT EXISTS (SELECT 1 FROM blob_gc g WHERE g.blob_id = b.id \
             AND g.reason = 'account_deleted' AND g.due_at <= {due})",
            account = self.account,
            due = self.due_now()
        ))
        .await
    }

    /// Queue rows of the account with the reason `account_deleted`, due by now (`Due::Now`).
    pub async fn queued_now(&self) -> i64 {
        self.count(&format!(
            "SELECT count(*) FROM blob_gc WHERE account_id = '{}' AND reason = 'account_deleted' \
             AND due_at <= {}",
            self.account,
            self.due_now()
        ))
        .await
    }

    /// The queue row and state of one blob.
    pub async fn queued(&self, blob: Uuid) -> (i64, Option<String>) {
        let rows = self
            .count(&format!(
                "SELECT count(*) FROM blob_gc WHERE blob_id = '{blob}'"
            ))
            .await;
        (rows, self.blob_state(blob).await)
    }

    pub async fn account_receipts(&self) -> i64 {
        self.count(&format!(
            "SELECT count(*) FROM commit_receipts WHERE account_id = '{}'",
            self.account
        ))
        .await
    }

    /// `count` receipts of the account, written directly.
    pub async fn fill_receipts(&self, count: i64) {
        self.exec(&format!(
            "INSERT INTO commit_receipts (account_id, commit_id, request_hash, response, created_at) \
             SELECT '{}', gen_random_uuid(), decode(repeat('07', 32), 'hex'), '[]'::jsonb, \
             'epoch'::timestamptz FROM generate_series(1, {count})",
            self.account
        ))
        .await;
    }

    /// Now on the rig's clock as a SQL timestamp.
    fn due_now(&self) -> String {
        format!(
            "'epoch'::timestamptz + {}::bigint * interval '1 microsecond'",
            self.clock.now_utc().as_unix_micros()
        )
    }
}
