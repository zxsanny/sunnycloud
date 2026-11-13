//! The read statements of `ThumbnailStateSource` and `DerivedOpener`, run inside one account-scoped transaction
//! of the caller. Each is a lookup on a unique index: `derived_pkey` (account_id, content_hash, kind,
//! params_version) or `jobs_key` (account_id, kind, content_hash, params_version); none joins another table.

use media_shared::types::{DerivedKind, JobKind};
use sc_db::{Db, DbError, DbTx, RunError, Scope};
use sc_ids::FileHash;
use sqlx::AssertSqlSafe;
use uuid::Uuid;

use crate::job_repo::kind_name;

/// The asked hashes that have a `thumb` row at the params version.
const THUMB_READY: &str = "SELECT content_hash FROM derived WHERE account_id = $1 \
     AND content_hash = ANY($2::bytea[]) AND kind = 'thumb' AND params_version = $3::bigint";

/// The unfinished or dead jobs of the asked hashes whose kind produces a `thumb` (`$4`: the job kind names).
const THUMB_JOBS: &str = "SELECT content_hash, status FROM jobs WHERE account_id = $1 \
     AND kind = ANY($4::text[]) AND content_hash = ANY($2::bytea[]) AND params_version = $3::bigint \
     AND status IN ('queued', 'running', 'dead')";

const DERIVED_ROW: &str = "SELECT blob_id, media_type FROM derived WHERE account_id = $1 \
     AND content_hash = $2 AND kind = $3 AND params_version = $4::bigint";

/// Whether a queued or running job of one of the kinds `$2` will produce the content's derived file.
const PENDING_JOB: &str = "SELECT EXISTS (SELECT 1 FROM jobs WHERE account_id = $1 AND kind = ANY($2::text[]) \
     AND content_hash = $3 AND params_version = $4::bigint AND status IN ('queued', 'running'))";

/// The state of a job that would produce a `thumb`, as `states` needs it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ThumbJob {
    /// Queued or running.
    Pending,
    Dead,
}

/// The derived row of one content and kind.
pub(crate) struct DerivedRow {
    pub blob_id: Uuid,
    pub media_type: String,
}

pub(crate) fn derived_kind_name(kind: DerivedKind) -> &'static str {
    match kind {
        DerivedKind::Thumb => "thumb",
        DerivedKind::Preview => "preview",
        DerivedKind::Transcode => "transcode",
    }
}

/// The names of the job kinds whose outputs include `kind` (`JobKind::outputs`).
pub(crate) fn producer_names(kind: DerivedKind) -> Vec<&'static str> {
    [JobKind::Stills, JobKind::Poster, JobKind::Transcode]
        .into_iter()
        .filter(|job| job.outputs().contains(&kind))
        .map(kind_name)
        .collect()
}

fn hash_params(hashes: &[FileHash]) -> Vec<&[u8]> {
    hashes
        .iter()
        .map(|hash| hash.as_bytes().as_slice())
        .collect()
}

fn file_hash(bytes: Vec<u8>) -> Result<FileHash, DbError> {
    <[u8; 32]>::try_from(bytes)
        .map(FileHash::from_bytes)
        .map_err(|_| DbError::Other("content hash is not 32 bytes".to_owned()))
}

pub(crate) async fn thumb_ready(
    tx: &mut DbTx,
    account: Uuid,
    hashes: &[FileHash],
    params_version: u32,
) -> Result<Vec<FileHash>, DbError> {
    let rows: Vec<Vec<u8>> = sqlx::query_scalar(THUMB_READY)
        .bind(account)
        .bind(hash_params(hashes))
        .bind(i64::from(params_version))
        .fetch_all(tx.conn())
        .await?;
    rows.into_iter().map(file_hash).collect()
}

pub(crate) async fn thumb_jobs(
    tx: &mut DbTx,
    account: Uuid,
    hashes: &[FileHash],
    params_version: u32,
) -> Result<Vec<(FileHash, ThumbJob)>, DbError> {
    let rows: Vec<(Vec<u8>, String)> = sqlx::query_as(THUMB_JOBS)
        .bind(account)
        .bind(hash_params(hashes))
        .bind(i64::from(params_version))
        .bind(producer_names(DerivedKind::Thumb))
        .fetch_all(tx.conn())
        .await?;
    rows.into_iter()
        .map(|(hash, status)| {
            let state = match status.as_str() {
                "queued" | "running" => ThumbJob::Pending,
                "dead" => ThumbJob::Dead,
                other => return Err(DbError::Other(format!("unexpected job status {other:?}"))),
            };
            Ok((file_hash(hash)?, state))
        })
        .collect()
}

pub(crate) async fn derived_row(
    tx: &mut DbTx,
    account: Uuid,
    content_hash: &FileHash,
    kind: DerivedKind,
    params_version: u32,
) -> Result<Option<DerivedRow>, DbError> {
    let row: Option<(Uuid, String)> = sqlx::query_as(DERIVED_ROW)
        .bind(account)
        .bind(content_hash.as_bytes().as_slice())
        .bind(derived_kind_name(kind))
        .bind(i64::from(params_version))
        .fetch_optional(tx.conn())
        .await?;
    Ok(row.map(|(blob_id, media_type)| DerivedRow {
        blob_id,
        media_type,
    }))
}

pub(crate) async fn pending_job(
    tx: &mut DbTx,
    account: Uuid,
    content_hash: &FileHash,
    kind: DerivedKind,
    params_version: u32,
) -> Result<bool, DbError> {
    let pending: bool = sqlx::query_scalar(PENDING_JOB)
        .bind(account)
        .bind(producer_names(kind))
        .bind(content_hash.as_bytes().as_slice())
        .bind(i64::from(params_version))
        .fetch_one(tx.conn())
        .await?;
    Ok(pending)
}

/// `EXPLAIN` of the two probes of `states` as the API role runs them in the account's scope, for the plan test.
#[doc(hidden)]
pub async fn explain_thumb_states(
    db: &Db,
    account: Uuid,
    hashes: &[FileHash],
    params_version: u32,
) -> Result<(String, String), RunError<DbError>> {
    db.run(Scope::Account(account), async |tx| {
        let derived: Vec<String> =
            sqlx::query_scalar(AssertSqlSafe(format!("EXPLAIN {THUMB_READY}")))
                .bind(account)
                .bind(hash_params(hashes))
                .bind(i64::from(params_version))
                .fetch_all(tx.conn())
                .await?;
        let jobs: Vec<String> = sqlx::query_scalar(AssertSqlSafe(format!("EXPLAIN {THUMB_JOBS}")))
            .bind(account)
            .bind(hash_params(hashes))
            .bind(i64::from(params_version))
            .bind(producer_names(DerivedKind::Thumb))
            .fetch_all(tx.conn())
            .await?;
        Ok::<_, DbError>((derived.join("\n"), jobs.join("\n")))
    })
    .await
}
