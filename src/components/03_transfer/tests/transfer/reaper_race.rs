//! Reaper versus commit (AZ-3756 AC-2; IT-03-9 case b): `SessionReaper::run_cycle` and `commit_upload` race on one
//! session 500 times. The reaper is handed the session's expiry as its `now` while the service clock still sees the
//! session live, so both really compete for the row. A fixed witness reservation stays on the account: a double
//! release would show as reserved below it (the ledger clamps at zero, so without it a second release could hide).
//! The probes share one owner connection, so a round costs the operations under test and one query.

use std::sync::Arc;

use contracts::{CommitServiceError, Janitor};
use sqlx::PgConnection;
use uuid::Uuid;

use crate::commit_support::commit_request;
use crate::put_support::{Upload, content};
use crate::start_support::{Rig, close, request, started};

const ROUNDS: usize = 500;
const SIZE: usize = 1024;
const WITNESS: i64 = 1_000_000;

/// `(used, reserved, state of blob)` of the rig's account in one statement.
async fn probe(conn: &mut PgConnection, account: Uuid, blob: Uuid) -> (i64, i64, Option<String>) {
    sqlx::query_as(
        "SELECT used_bytes, reserved_bytes, (SELECT state FROM blobs WHERE id = $2) \
         FROM quota WHERE account_id = $1",
    )
    .bind(account)
    .bind(blob)
    .fetch_one(conn)
    .await
    .expect("probe")
}

async fn blob_of(conn: &mut PgConnection, upload_id: Uuid) -> Uuid {
    sqlx::query_scalar("SELECT blob_id FROM upload_sessions WHERE id = $1")
        .bind(upload_id)
        .fetch_one(conn)
        .await
        .expect("session blob")
}

#[tokio::test]
async fn session_reaper_racing_commit_upload_has_exactly_one_winner_each_round() {
    // Arrange
    let rig = Rig::new().await;
    let reaper = Arc::new(sunnycloud_transfer::SessionReaper::new(Arc::clone(
        &rig.service,
    )));
    let mut conn = rig.owner().await;
    sqlx::query("UPDATE quota SET reserved_bytes = reserved_bytes + $2 WHERE account_id = $1")
        .bind(rig.account)
        .bind(WITNESS)
        .execute(&mut conn)
        .await
        .expect("witness reservation");
    let mut used = 0i64;
    let mut wins = (0usize, 0usize);

    for round in 0..ROUNDS {
        let name = format!("race-{round}.bin");
        let size = SIZE as i64;
        let start = started(rig.start(request(rig.root, &name, size, &name)).await);
        let upload = Upload {
            id: start.upload_id,
            blob: blob_of(&mut conn, start.upload_id).await,
            data: content(round as u64, SIZE),
        };
        rig.put(&upload, 0).await.expect("block");
        let request = commit_request(&upload, rig.root, &name, None);
        let (service, actor) = (Arc::clone(&rig.service), rig.actor());
        let expiry = start.expires_at;

        // Act
        let commit = tokio::spawn(async move { service.commit_upload(actor, request).await });
        let reap = {
            let reaper = Arc::clone(&reaper);
            tokio::spawn(async move { reaper.run_cycle(expiry).await })
        };
        let committed = commit.await.expect("commit task");
        reap.await.expect("reap task").expect("cycle");
        let (now_used, reserved, state) = probe(&mut conn, rig.account, upload.blob).await;

        // Assert
        match committed {
            Ok(_) => {
                wins.0 += 1;
                used += size;
                assert_eq!(state.as_deref(), Some("committed"), "round {round}");
                assert!(rig.file_exists(upload.blob), "round {round}");
            }
            Err(CommitServiceError::UploadNotFound) => {
                wins.1 += 1;
                assert_eq!(state, None, "round {round}");
                assert!(!rig.file_exists(upload.blob), "round {round}");
            }
            other => panic!("round {round}: {other:?}"),
        }
        assert_eq!((now_used, reserved), (used, WITNESS), "round {round}");
    }
    assert_eq!(rig.revisions().await, wins.0 as i64);
    eprintln!("commit won {} rounds, reaper won {}", wins.0, wins.1);
    close(conn).await;
    drop(reaper);
    rig.finish().await;
}
