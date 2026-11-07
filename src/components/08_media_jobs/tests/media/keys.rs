//! AC-2: the unique job key, the unique derived key and blob id, the cascade from a revision to its jobs, the
//! generated lane, and the account match of every composite reference.

use sqlx::{Connection, PgConnection};
use uuid::Uuid;

use crate::support::{
    DerivedRow, Fixture, JobRow, account_with_revision, blob, constraint, derived, job, sqlstate,
};

const UNIQUE_VIOLATION: &str = "23505";
const FOREIGN_KEY_REFUSALS: [&str; 2] = ["23503", "23001"];
const GENERATED_COLUMN_WRITE: &str = "428C9";
const HASH: [u8; 32] = [7; 32];

async fn lane_of(conn: &mut PgConnection, id: Uuid) -> String {
    sqlx::query_scalar("SELECT lane FROM jobs WHERE id = $1")
        .bind(id)
        .fetch_one(conn)
        .await
        .expect("read lane")
}

async fn job_count(conn: &mut PgConnection, revision: Uuid) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM jobs WHERE revision_id = $1")
        .bind(revision)
        .fetch_one(conn)
        .await
        .expect("count jobs")
}

#[tokio::test]
async fn a_second_job_with_the_same_account_kind_hash_and_params_version_is_refused() {
    // Arrange
    let fixture = Fixture::new().await;
    let mut conn = fixture.owner_conn().await;
    let account = Uuid::new_v4();
    let revision = account_with_revision(&mut conn, account, &HASH).await;
    let row = JobRow::new(account, revision, &HASH);
    job(&mut conn, &row).await.expect("first job");

    // Act
    let duplicate = job(&mut conn, &row).await;
    let other_params = job(
        &mut conn,
        &JobRow {
            params_version: 2,
            ..JobRow::new(account, revision, &HASH)
        },
    )
    .await;
    let other_kind = job(
        &mut conn,
        &JobRow {
            kind: "poster",
            ..JobRow::new(account, revision, &HASH)
        },
    )
    .await;

    // Assert
    let error = duplicate.expect_err("one job per key");
    assert_eq!(sqlstate(&error), UNIQUE_VIOLATION);
    assert_eq!(constraint(&error), "jobs_key");
    other_params.expect("a new params version is a new job");
    other_kind.expect("another kind is another job");
    conn.close().await.expect("close owner connection");
    fixture.finish().await;
}

#[tokio::test]
async fn the_same_content_in_two_accounts_makes_two_jobs() {
    // Arrange
    let fixture = Fixture::new().await;
    let mut conn = fixture.owner_conn().await;
    let (a, b) = (Uuid::new_v4(), Uuid::new_v4());
    let revision_a = account_with_revision(&mut conn, a, &HASH).await;
    let revision_b = account_with_revision(&mut conn, b, &HASH).await;

    // Act
    let first = job(&mut conn, &JobRow::new(a, revision_a, &HASH)).await;
    let second = job(&mut conn, &JobRow::new(b, revision_b, &HASH)).await;

    // Assert
    first.expect("account a");
    second.expect("account b");
    conn.close().await.expect("close owner connection");
    fixture.finish().await;
}

#[tokio::test]
async fn a_second_derived_row_with_the_same_key_or_the_same_blob_is_refused() {
    // Arrange
    let fixture = Fixture::new().await;
    let mut conn = fixture.owner_conn().await;
    let account = Uuid::new_v4();
    account_with_revision(&mut conn, account, &HASH).await;
    let (first_blob, second_blob, third_blob) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());
    for id in [first_blob, second_blob, third_blob] {
        blob(&mut conn, id, account).await;
    }
    let row = DerivedRow {
        account,
        content_hash: &HASH,
        kind: "thumb",
        params_version: 1,
        blob: first_blob,
    };
    derived(&mut conn, &row).await.expect("first derived row");

    // Act
    let same_key = derived(
        &mut conn,
        &DerivedRow {
            blob: second_blob,
            ..row
        },
    )
    .await;
    let same_blob = derived(
        &mut conn,
        &DerivedRow {
            kind: "preview",
            ..row
        },
    )
    .await;
    let other_kind = derived(
        &mut conn,
        &DerivedRow {
            kind: "preview",
            blob: third_blob,
            ..row
        },
    )
    .await;

    // Assert
    let key_error = same_key.expect_err("one derived row per key");
    assert_eq!(sqlstate(&key_error), UNIQUE_VIOLATION);
    assert_eq!(constraint(&key_error), "derived_pkey");
    let blob_error = same_blob.expect_err("one derived row per blob");
    assert_eq!(sqlstate(&blob_error), UNIQUE_VIOLATION);
    assert_eq!(constraint(&blob_error), "derived_blob_id_key");
    other_kind.expect("another kind over another blob");
    conn.close().await.expect("close owner connection");
    fixture.finish().await;
}

#[tokio::test]
async fn deleting_a_revision_deletes_its_jobs_and_no_other() {
    // Arrange
    let fixture = Fixture::new().await;
    let mut conn = fixture.owner_conn().await;
    let (a, b) = (Uuid::new_v4(), Uuid::new_v4());
    let revision_a = account_with_revision(&mut conn, a, &HASH).await;
    let revision_b = account_with_revision(&mut conn, b, &HASH).await;
    for kind in ["stills", "poster", "transcode"] {
        job(
            &mut conn,
            &JobRow {
                kind,
                ..JobRow::new(a, revision_a, &HASH)
            },
        )
        .await
        .expect("job of a");
    }
    job(&mut conn, &JobRow::new(b, revision_b, &HASH))
        .await
        .expect("job of b");
    let before = (
        job_count(&mut conn, revision_a).await,
        job_count(&mut conn, revision_b).await,
    );

    // Act
    sqlx::query("DELETE FROM revisions WHERE id = $1")
        .bind(revision_a)
        .execute(&mut conn)
        .await
        .expect("delete the revision");
    let after = (
        job_count(&mut conn, revision_a).await,
        job_count(&mut conn, revision_b).await,
    );

    // Assert
    assert_eq!(before, (3, 1));
    assert_eq!(after, (0, 1));
    conn.close().await.expect("close owner connection");
    fixture.finish().await;
}

#[tokio::test]
async fn lane_is_image_for_stills_and_video_for_poster_and_transcode() {
    // Arrange
    let fixture = Fixture::new().await;
    let mut conn = fixture.owner_conn().await;
    let account = Uuid::new_v4();
    let revision = account_with_revision(&mut conn, account, &HASH).await;

    // Act
    let mut lanes = Vec::new();
    for kind in ["stills", "poster", "transcode"] {
        let id = job(
            &mut conn,
            &JobRow {
                kind,
                ..JobRow::new(account, revision, &HASH)
            },
        )
        .await
        .expect("job");
        lanes.push((kind, lane_of(&mut conn, id).await));
    }

    // Assert
    assert_eq!(
        lanes,
        [
            ("stills", "image".to_owned()),
            ("poster", "video".to_owned()),
            ("transcode", "video".to_owned())
        ]
    );
    conn.close().await.expect("close owner connection");
    fixture.finish().await;
}

#[tokio::test]
async fn a_lane_cannot_be_written() {
    // Arrange
    let fixture = Fixture::new().await;
    let mut conn = fixture.owner_conn().await;
    let account = Uuid::new_v4();
    let revision = account_with_revision(&mut conn, account, &HASH).await;
    let id = job(&mut conn, &JobRow::new(account, revision, &HASH))
        .await
        .expect("job");

    // Act
    let update = sqlx::query("UPDATE jobs SET lane = 'video' WHERE id = $1")
        .bind(id)
        .execute(&mut conn)
        .await;

    // Assert
    assert_eq!(
        sqlstate(&update.expect_err("generated column")),
        GENERATED_COLUMN_WRITE
    );
    conn.close().await.expect("close owner connection");
    fixture.finish().await;
}

#[tokio::test]
async fn a_job_or_a_derived_row_cannot_name_another_accounts_revision_or_blob() {
    // Arrange
    let fixture = Fixture::new().await;
    let mut conn = fixture.owner_conn().await;
    let (a, b) = (Uuid::new_v4(), Uuid::new_v4());
    let revision_a = account_with_revision(&mut conn, a, &HASH).await;
    account_with_revision(&mut conn, b, &[8; 32]).await;
    let blob_b = Uuid::new_v4();
    blob(&mut conn, blob_b, b).await;

    // Act
    let foreign_revision = job(&mut conn, &JobRow::new(b, revision_a, &HASH)).await;
    let foreign_blob = derived(
        &mut conn,
        &DerivedRow {
            account: a,
            content_hash: &HASH,
            kind: "thumb",
            params_version: 1,
            blob: blob_b,
        },
    )
    .await;

    // Assert
    for error in [
        foreign_revision.expect_err("foreign revision"),
        foreign_blob.expect_err("foreign blob"),
    ] {
        assert!(
            FOREIGN_KEY_REFUSALS.contains(&sqlstate(&error).as_str()),
            "{error}"
        );
    }
    conn.close().await.expect("close owner connection");
    fixture.finish().await;
}
