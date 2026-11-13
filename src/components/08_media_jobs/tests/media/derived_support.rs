//! Fixtures of the derived access (AZ-3844). Rows go in as the schema owner with their foreign keys, or, for the
//! bulk seeds, as the superuser with foreign-key triggers off (as `plans.rs`: the probes join nothing, so only
//! the indexed columns matter).

use contracts::{DerivedBlob, DerivedError};
use sc_ids::FileHash;
use sqlx::PgConnection;
use uuid::Uuid;

/// Jobs of `kind` and `status` for `hashes` of `account`, at params version 1, on any revision (triggers off).
pub async fn bulk_jobs(
    admin: &mut PgConnection,
    account: Uuid,
    hashes: &[FileHash],
    kind: &str,
    status: &str,
) {
    let outcome = (status == "succeeded").then_some("derived");
    sqlx::query(
        "INSERT INTO jobs (id, account_id, revision_id, kind, content_hash, params_version, priority, status, \
                           available_at, created_at, finished_at, outcome) \
         SELECT gen_random_uuid(), $1, gen_random_uuid(), $2, h, 1, 0, $3, 'epoch'::timestamptz, \
                'epoch'::timestamptz, CASE WHEN $3 IN ('succeeded', 'dead') THEN 'epoch'::timestamptz END, $4 \
         FROM unnest($5::bytea[]) AS h",
    )
    .bind(account)
    .bind(kind)
    .bind(status)
    .bind(outcome)
    .bind(bytes(hashes))
    .execute(admin)
    .await
    .expect("insert jobs");
}

/// `kind` rows at `params_version` for `hashes` of `account`, each with a blob id of its own (triggers off).
pub async fn bulk_derived(
    admin: &mut PgConnection,
    account: Uuid,
    hashes: &[FileHash],
    kind: &str,
    params_version: i16,
) {
    sqlx::query(
        "INSERT INTO derived (account_id, content_hash, kind, params_version, blob_id, media_type, size, created_at) \
         SELECT $1, h, $2, $3, gen_random_uuid(), 'image/webp', 10, 'epoch'::timestamptz \
         FROM unnest($4::bytea[]) AS h",
    )
    .bind(account)
    .bind(kind)
    .bind(params_version)
    .bind(bytes(hashes))
    .execute(admin)
    .await
    .expect("insert derived rows");
}

pub async fn replica(admin: &mut PgConnection) {
    sqlx::raw_sql("SET session_replication_role = replica")
        .execute(admin)
        .await
        .expect("disable foreign-key triggers");
}

fn bytes(hashes: &[FileHash]) -> Vec<Vec<u8>> {
    hashes.iter().map(|hash| hash.as_bytes().to_vec()).collect()
}

/// What a lookup answered, comparable across the two entry points.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Blob(DerivedBlob),
    NotReady,
    NotFound,
    Unexpected,
}

pub fn outcome(result: Result<DerivedBlob, DerivedError>) -> Outcome {
    match result {
        Ok(blob) => Outcome::Blob(blob),
        Err(DerivedError::NotReady) => Outcome::NotReady,
        Err(DerivedError::NotFound) => Outcome::NotFound,
        Err(DerivedError::Unexpected(_)) => Outcome::Unexpected,
    }
}
