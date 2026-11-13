//! AZ-3844 AC-1 (IT-08-14) and the states half of AC-3 (ST-08-4): `ThumbnailStates::states` answers one state
//! per asked hash from two batched probes, and nothing of another account; PT: both probes stay index lookups
//! with 100,000 rows in each table.

use std::collections::HashMap;

use contracts::{ThumbState, ThumbnailStateSource};
use media_api::{JobPlanner, ThumbnailStates, explain_thumb_states};
use media_shared::policy::CURRENT_PARAMS_VERSION;
use sc_ids::FileHash;
use sqlx::Connection;
use uuid::Uuid;

use crate::derived_support::{bulk_derived, bulk_jobs, replica};
use crate::queue_support::hash;
use crate::statements::StatementLog;
use crate::support::{Fixture, account, account_with_revision};

fn source(fixture: &Fixture) -> ThumbnailStates {
    ThumbnailStates::new(
        fixture.api.clone(),
        JobPlanner::new(CURRENT_PARAMS_VERSION, 200),
    )
}

fn range(from: u32, to: u32) -> Vec<FileHash> {
    (from..to).map(hash).collect()
}

/// The AC-1 account: hashes 0..400 with a `thumb` row (and the finished job), 400..550 with queued or running
/// `stills` jobs, 550..700 with queued `poster` jobs, 700..800 dead, 800..1000 with no job (SVG, unsupported);
/// plus a `thumb` row at another params version and a `transcode` job (neither makes a thumbnail state).
async fn seed_states(fixture: &Fixture, account_id: Uuid) {
    let mut admin = fixture.db.admin().await;
    replica(&mut admin).await;
    bulk_derived(&mut admin, account_id, &range(0, 400), "thumb", 1).await;
    bulk_jobs(
        &mut admin,
        account_id,
        &range(0, 400),
        "stills",
        "succeeded",
    )
    .await;
    bulk_jobs(&mut admin, account_id, &range(400, 475), "stills", "queued").await;
    bulk_jobs(
        &mut admin,
        account_id,
        &range(475, 550),
        "stills",
        "running",
    )
    .await;
    bulk_jobs(&mut admin, account_id, &range(550, 700), "poster", "queued").await;
    bulk_jobs(&mut admin, account_id, &range(700, 800), "stills", "dead").await;
    bulk_derived(&mut admin, account_id, &range(800, 850), "thumb", 2).await;
    bulk_jobs(
        &mut admin,
        account_id,
        &range(850, 900),
        "transcode",
        "queued",
    )
    .await;
    admin.close().await.expect("close admin connection");
}

fn expected(hashes: &[FileHash]) -> HashMap<FileHash, ThumbState> {
    hashes
        .iter()
        .enumerate()
        .map(|(n, hash)| {
            let state = match n {
                0..400 => ThumbState::Ready,
                400..700 => ThumbState::Pending,
                700..800 => ThumbState::Failed,
                _ => ThumbState::None,
            };
            (*hash, state)
        })
        .collect()
}

fn is_probe(sql: &str) -> bool {
    !matches!(sql.trim(), "BEGIN" | "COMMIT") && !sql.contains("set_config")
}

#[tokio::test]
async fn states_of_1000_hashes_match_the_seeded_states_in_two_statements() {
    // Arrange
    let fixture = Fixture::new().await;
    let a = Uuid::new_v4();
    let vault = hash(5_000);
    let unknown = hash(5_001);
    let mut conn = fixture.owner_conn().await;
    account_with_revision(&mut conn, a, vault.as_bytes()).await;
    conn.close().await.expect("close owner connection");
    seed_states(&fixture, a).await;
    let mut hashes = range(0, 1_000);
    hashes.extend([vault, unknown]);
    let states = source(&fixture);
    let log = StatementLog::install();

    // Act
    let answer = states.states(a, &hashes).await.expect("states");

    // Assert
    let probes: Vec<String> = log.take().into_iter().filter(|sql| is_probe(sql)).collect();
    let mut want = expected(&hashes[..1_000]);
    want.insert(vault, ThumbState::None);
    want.insert(unknown, ThumbState::None);
    assert_eq!(answer, want);
    assert_eq!(probes.len(), 2, "{probes:#?}");
    assert!(probes[0].contains("FROM derived"), "{probes:#?}");
    assert!(probes[1].contains("FROM jobs"), "{probes:#?}");
    drop(states);
    fixture.finish().await;
}

#[tokio::test]
async fn another_account_sees_no_state_of_the_hashes_of_a() {
    // Arrange
    let fixture = Fixture::new().await;
    let (a, b) = (Uuid::new_v4(), Uuid::new_v4());
    let mut conn = fixture.owner_conn().await;
    account(&mut conn, a).await;
    account(&mut conn, b).await;
    conn.close().await.expect("close owner connection");
    seed_states(&fixture, a).await;
    let hashes = range(0, 1_000);
    let states = source(&fixture);

    // Act
    let answer = states.states(b, &hashes).await.expect("states");

    // Assert
    assert_eq!(answer.len(), 1_000);
    assert!(answer.values().all(|state| *state == ThumbState::None));
    drop(states);
    fixture.finish().await;
}

#[tokio::test]
async fn no_hash_answers_an_empty_map_without_a_statement() {
    // Arrange
    let fixture = Fixture::new().await;
    let states = source(&fixture);
    let log = StatementLog::install();

    // Act
    let answer = states.states(Uuid::new_v4(), &[]).await.expect("states");

    // Assert
    assert!(answer.is_empty());
    assert!(log.take().is_empty());
    drop(states);
    fixture.finish().await;
}

/// 100,000 `thumb` rows and 100,000 jobs over ten accounts, statistics current.
async fn seed_volume(fixture: &Fixture) {
    let mut admin = fixture.db.admin().await;
    sqlx::query(
        "INSERT INTO accounts (id, state, created_at, display_name, role) \
         SELECT md5('account' || n)::uuid, 'active', 'epoch'::timestamptz, 'Fixture', 'user' \
         FROM generate_series(0, 9) AS n",
    )
    .execute(&mut admin)
    .await
    .expect("insert accounts");
    replica(&mut admin).await;
    sqlx::raw_sql(
        "INSERT INTO derived (account_id, content_hash, kind, params_version, blob_id, media_type, size, created_at) \
         SELECT md5('account' || (n % 10))::uuid, sha256(convert_to('d' || n, 'UTF8')), 'thumb', 1, \
                gen_random_uuid(), 'image/webp', 10, 'epoch'::timestamptz \
         FROM generate_series(0, 99999) AS n; \
         INSERT INTO jobs (id, account_id, revision_id, kind, content_hash, params_version, priority, status, \
                           available_at, created_at) \
         SELECT gen_random_uuid(), md5('account' || (n % 10))::uuid, gen_random_uuid(), \
                (ARRAY['stills', 'poster', 'transcode'])[1 + n % 3], sha256(convert_to('j' || n, 'UTF8')), 1, 0, \
                'queued', 'epoch'::timestamptz, 'epoch'::timestamptz \
         FROM generate_series(0, 99999) AS n",
    )
    .execute(&mut admin)
    .await
    .expect("seed");
    sqlx::raw_sql("VACUUM (ANALYZE) derived, jobs")
        .execute(&mut admin)
        .await
        .expect("vacuum and analyze");
    admin.close().await.expect("close admin connection");
}

#[tokio::test]
async fn both_probes_are_index_lookups_with_100000_rows_per_table() {
    // Arrange
    let fixture = Fixture::new().await;
    seed_volume(&fixture).await;
    let mut conn = fixture.owner_conn().await;
    let account: Uuid = sqlx::query_scalar("SELECT md5('account3')::uuid")
        .fetch_one(&mut conn)
        .await
        .expect("account");
    let hashes: Vec<Vec<u8>> = sqlx::query_scalar(
        "(SELECT content_hash FROM derived WHERE account_id = $1 LIMIT 500) \
         UNION ALL (SELECT content_hash FROM jobs WHERE account_id = $1 LIMIT 500)",
    )
    .bind(account)
    .fetch_all(&mut conn)
    .await
    .expect("hashes");
    conn.close().await.expect("close owner connection");
    let hashes: Vec<FileHash> = hashes
        .into_iter()
        .map(|bytes| FileHash::from_bytes(bytes.try_into().expect("32 bytes")))
        .collect();

    // Act
    let (derived, jobs) =
        explain_thumb_states(&fixture.api, account, &hashes, CURRENT_PARAMS_VERSION)
            .await
            .expect("plans");

    // Assert
    assert_eq!(hashes.len(), 1_000);
    assert!(derived.contains("derived_pkey"), "{derived}");
    assert!(!derived.contains("Seq Scan"), "{derived}");
    assert!(jobs.contains("jobs_key"), "{jobs}");
    assert!(!jobs.contains("Seq Scan"), "{jobs}");
    fixture.finish().await;
}
