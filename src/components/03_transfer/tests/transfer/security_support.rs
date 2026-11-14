//! Shared pieces of the transfer security suite (AZ-3758): the whole stored state of one account, compared
//! before and after another account's attempts, and calls made as another account through the ports.

use contracts::{BlockAck, CommitActor, Staged, UploadError, UploadService};
use sc_ids::{Hash32, hash_block};
use uuid::Uuid;

use crate::commit_support::DEVICE;
use crate::put_support::body;
use crate::start_support::{Rig, close};

/// Every row of one account (`$1`) in each table the suite compares, as text in a fixed order.
const STATE: [&str; 6] = [
    "SELECT string_agg(t::text, '|' ORDER BY t::text) FROM upload_sessions t WHERE account_id = $1",
    "SELECT string_agg(t::text, '|' ORDER BY t::text) FROM commit_receipts t WHERE account_id = $1",
    "SELECT string_agg(t::text, '|' ORDER BY t::text) FROM nodes t WHERE account_id = $1",
    "SELECT string_agg(t::text, '|' ORDER BY t::text) FROM revisions t WHERE account_id = $1",
    "SELECT string_agg(t::text, '|' ORDER BY t::text) FROM blobs t WHERE account_id = $1",
    "SELECT string_agg(t::text, '|' ORDER BY t::text) FROM quota t WHERE account_id = $1",
];

pub fn actor(account: Uuid) -> CommitActor {
    CommitActor {
        account_id: account,
        device_label: DEVICE.to_owned(),
    }
}

impl Rig {
    /// Every row of `account` in the 02, 03, 04 and 07a tables, as text in a fixed order.
    pub async fn account_state(&self, account: Uuid) -> Vec<String> {
        let mut conn = self.owner().await;
        let mut state = Vec::new();
        for statement in STATE {
            let rows: Option<String> = sqlx::query_scalar(statement)
                .bind(account)
                .fetch_one(&mut conn)
                .await
                .expect("account rows");
            state.push(rows.unwrap_or_default());
        }
        close(conn).await;
        state
    }

    /// `put_block` as `account` with `bytes` declared as their own length.
    pub async fn put_as(
        &self,
        account: Uuid,
        upload_id: Uuid,
        index: u32,
        hash: Hash32,
        bytes: &[u8],
    ) -> Result<BlockAck, UploadError> {
        let (stream, _) = body(bytes, 64 * 1024);
        let service: &dyn UploadService = self.service.as_ref();
        service
            .put_block(account, upload_id, index, hash, stream, bytes.len() as u64)
            .await
    }

    /// `stage_small` of `data` under `key` as `account`, with the client's hash.
    pub async fn stage_as(
        &self,
        account: Uuid,
        key: Uuid,
        data: &[u8],
    ) -> Result<Staged, UploadError> {
        let (stream, _) = body(data, 64 * 1024);
        let hash = (!data.is_empty()).then(|| hash_block(data));
        let service: &dyn UploadService = self.service.as_ref();
        service
            .stage_small(account, key, hash, stream, data.len() as u64)
            .await
    }
}
