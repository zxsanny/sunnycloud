//! Shared pieces of the janitor tests (AZ-3756): the short-TTL settings of IT-03-10, receipts of a chosen age and
//! probes of the blob file and the touched rows.

use std::time::Duration;

use sc_clock::Timestamp;
use sunnycloud_transfer::TransferConfig;
use uuid::Uuid;

use crate::start_support::{Rig, close, transfer_config};

pub const SHORT_TTL: Duration = Duration::from_millis(400);
pub const CHECKPOINT_INTERVAL: Duration = Duration::from_millis(100);

/// IT-03-10: a 400 ms TTL, a checkpoint at least every 100 ms, and room for 32 open sessions.
pub fn short_config() -> TransferConfig {
    TransferConfig {
        upload_session_ttl: SHORT_TTL,
        checkpoint_interval: CHECKPOINT_INTERVAL,
        max_open_sessions: 32,
        ..transfer_config()
    }
}

impl Rig {
    /// A receipt of the rig's account created at `created_at`; returns its commit id.
    pub async fn aged_receipt(&self, created_at: Timestamp) -> Uuid {
        let commit_id = Uuid::new_v4();
        let mut conn = self.owner().await;
        sqlx::query(
            "INSERT INTO commit_receipts (account_id, commit_id, request_hash, response, created_at) \
             VALUES ($1, $2, $3, '[]'::jsonb, 'epoch'::timestamptz + $4::bigint * interval '1 microsecond')",
        )
        .bind(self.account)
        .bind(commit_id)
        .bind(vec![7u8; 32])
        .bind(created_at.as_unix_micros())
        .execute(&mut conn)
        .await
        .expect("insert receipt");
        close(conn).await;
        commit_id
    }

    pub fn file_exists(&self, blob: Uuid) -> bool {
        sunnycloud_blobstore::blob_path(&self.volume, blob).exists()
    }

    /// Whether the session or staged row `id` is still there.
    pub async fn has_session(&self, id: Uuid) -> bool {
        self.count(&format!(
            "SELECT count(*) FROM upload_sessions WHERE id = '{id}'"
        ))
        .await
            == 1
    }
}
