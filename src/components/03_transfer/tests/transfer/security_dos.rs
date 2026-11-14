//! ST-03-5 (AZ-3758 AC-5; AC-39): quota and disk DoS at 03's boundary on the real 04 volume.
//!
//! - DS-26 shape: 100 concurrent starts declaring 50 GiB from one account and from 20 accounts (quota raised to
//!   1 TiB so the cap, not the quota, binds first). Which refusal ends a start depends on the volume: 04 refuses a
//!   50 GiB file on a smaller volume (`StorageFull`) after the session transaction and 03 compensates. The test
//!   asserts what holds on any volume: at most 8 sessions per account, only the four typed refusals, reserved
//!   equal to the open sessions and at most the quota.
//! - The same shape at 1 GiB, which every runner holds, where the counts are exact.
//! - The disk filler (the free-space floor) is its own test binary, `tests/volume_floor/`.

use std::collections::HashMap;
use std::sync::Arc;

use contracts::{StartUploadOutcome, UploadService};
use uuid::Uuid;

use crate::start_support::{GIB, Rig, request};

/// Each account's started sessions and refusals by name.
#[derive(Debug, Default)]
struct Tally {
    started: Vec<Uuid>,
    refused: HashMap<String, usize>,
}

/// `per_account` concurrent starts of `size` bytes for each of `accounts`, all at once.
async fn storm(rig: &Rig, accounts: &[(Uuid, Uuid)], per_account: usize, size: i64) -> Vec<Tally> {
    let mut tasks = Vec::new();
    for &(account, root) in accounts {
        for index in 0..per_account {
            let service = Arc::clone(&rig.service);
            let key = format!("dos-{index}");
            tasks.push(tokio::spawn(async move {
                let service: &dyn UploadService = service.as_ref();
                let outcome = service
                    .start_upload(account, request(root, &key, size, &key))
                    .await;
                (account, outcome)
            }));
        }
    }
    let mut tallies: HashMap<Uuid, Tally> = HashMap::new();
    for task in tasks {
        let (account, outcome) = task.await.expect("start task");
        let tally = tallies.entry(account).or_default();
        match outcome {
            Ok(StartUploadOutcome::Started(started)) => tally.started.push(started.upload_id),
            Ok(other) => panic!("unexpected outcome {other:?}"),
            Err(error) => {
                let name = format!("{error:?}");
                *tally.refused.entry(name).or_default() += 1;
            }
        }
    }
    accounts
        .iter()
        .map(|(account, _)| tallies.remove(account).unwrap_or_default())
        .collect()
}

async fn set_limit(rig: &Rig, account: Uuid, limit: i64) {
    rig.exec(&format!(
        "UPDATE quota SET limit_bytes = {limit} WHERE account_id = '{account}'"
    ))
    .await;
}

/// The rig's account and 20 more, each with `limit` bytes of quota.
async fn accounts(rig: &Rig, limit: i64) -> Vec<(Uuid, Uuid)> {
    let mut accounts = vec![(rig.account, rig.root)];
    for _ in 0..20 {
        accounts.push(rig.other_account().await);
    }
    for &(account, _) in &accounts {
        set_limit(rig, account, limit).await;
    }
    accounts
}

async fn reserved_of(rig: &Rig, account: Uuid) -> i64 {
    rig.count(&format!(
        "SELECT reserved_bytes FROM quota WHERE account_id = '{account}'"
    ))
    .await
}

async fn open_sessions_of(rig: &Rig, account: Uuid) -> i64 {
    rig.count(&format!(
        "SELECT count(*) FROM upload_sessions WHERE account_id = '{account}'"
    ))
    .await
}

const REFUSALS: [&str; 4] = [
    "SessionCapReached",
    "QuotaExceeded",
    "FreeSpaceFloor",
    "StorageFull",
];

#[tokio::test]
async fn start_upload_100_concurrent_50_gib_declarations_never_pass_the_cap_or_the_quota() {
    // Arrange
    let rig = Rig::new().await;
    let accounts = accounts(&rig, 1 << 40).await;
    let declared = 50 * GIB;

    // Act
    let mut tallies = storm(&rig, &accounts[..1], 100, declared).await;
    tallies.extend(storm(&rig, &accounts[1..], 5, declared).await);

    // Assert
    for (tally, &(account, _)) in tallies.iter().zip(&accounts) {
        assert!(tally.started.len() <= 8, "{tally:?}");
        for name in tally.refused.keys() {
            assert!(REFUSALS.contains(&name.as_str()), "{name}");
        }
        let sessions = open_sessions_of(&rig, account).await;
        assert_eq!(sessions, tally.started.len() as i64, "{tally:?}");
        let reserved = reserved_of(&rig, account).await;
        assert_eq!(reserved, sessions * declared);
        assert!(reserved <= 1 << 40);
    }
    eprintln!("one account: {:?}", tallies[0]);
    rig.finish().await;
}

#[tokio::test]
async fn start_upload_concurrent_starts_stop_at_the_session_cap_and_the_quota() {
    // Arrange
    let rig = Rig::new().await;
    let accounts = accounts(&rig, 3 * GIB).await;
    set_limit(&rig, rig.account, 10 * GIB).await;

    // Act
    let one = storm(&rig, &accounts[..1], 100, GIB).await;
    let many = storm(&rig, &accounts[1..], 5, GIB).await;

    // Assert
    assert_eq!(one[0].started.len(), 8);
    assert_eq!(
        one[0].refused,
        HashMap::from([("SessionCapReached".to_owned(), 92)])
    );
    for (tally, &(account, _)) in many.iter().zip(&accounts[1..]) {
        assert_eq!(tally.started.len(), 3, "{tally:?}");
        assert_eq!(
            tally.refused,
            HashMap::from([("QuotaExceeded".to_owned(), 2)])
        );
        assert_eq!(reserved_of(&rig, account).await, 3 * GIB);
    }
    assert_eq!(reserved_of(&rig, rig.account).await, 8 * GIB);
    rig.finish().await;
}
