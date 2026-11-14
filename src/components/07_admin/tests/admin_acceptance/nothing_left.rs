//! AZ-3823 AC-3 (AT-07-1 in its port form; AC-27, AC-30, AC-21; FT-P-19, NFT-RES-30, NFT-SEC-27): a user with the
//! 300 small files of DS-23 (as rows over one-byte blob files: the erase never reads content), a photo with a
//! finished thumbnail (08's jobs and derived rows), a share link made through 06, 2 live sessions and a device
//! token requests its own deletion; the old credentials are refused within 5 s; the purger is dropped mid-cycle
//! and a fresh instance on a fresh pool finishes; nothing of the account is left and its link answers as
//! missing. The container kill, the HTTP 202/401 and the ctl probe of AT-07-1 run in AZ-3665 and FT-P-19.

use std::sync::Arc;
use std::time::{Duration, Instant};

use contracts::{
    AccountDeletionService, Authenticator, AuthenticatorError, Credential, Janitor, ShareOpen,
    ShareService,
};
use sc_clock::{Clock, SystemClock};
use sqlx::{Connection, PgConnection};
use sunnycloud_identity::IdentityAuthenticator;
use tokio::sync::Notify;
use uuid::Uuid;

use crate::capture;
use crate::ports::{Ports, VISITOR, live_session, principal};
use crate::purge_support::probes::{deleted_audits, drain, files_left, rows_of, state, total};
use crate::purge_support::recording::{Fault, Log, record_hooks};
use crate::purge_support::seed;
use crate::purge_support::world::PurgeWorld;
use crate::support::seed::device_token;
use crate::support::world::{device_credential, session_credential};

/// The 300 files of DS-23's mass-delete set.
const FILES: usize = 300;
const REVOCATION_BOUND: Duration = Duration::from_secs(5);
/// AC-3 target with the test settings (AC-27 limit 15 min).
const PURGE_TARGET: Duration = Duration::from_secs(60);
const POLL: Duration = Duration::from_millis(100);

async fn first_file(conn: &mut PgConnection, account: Uuid) -> Uuid {
    sqlx::query_scalar(
        "SELECT id FROM nodes WHERE account_id = $1 AND kind = 'file' ORDER BY id LIMIT 1",
    )
    .bind(account)
    .fetch_one(conn)
    .await
    .expect("a file of the account")
}

async fn share_links(conn: &mut PgConnection, account: Uuid) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM share_links WHERE account_id = $1")
        .bind(account)
        .fetch_one(conn)
        .await
        .expect("count share links")
}

/// The two sessions' access tokens and the device token, as presented by the clients.
fn credentials(sessions: [Uuid; 2], device: Uuid) -> [Credential; 3] {
    [
        session_credential(sessions[0]),
        session_credential(sessions[1]),
        device_credential(device),
    ]
}

/// Polls each credential on `authenticator` until it is refused; returns how long that took.
async fn refused_after(
    authenticator: &IdentityAuthenticator,
    sessions: [Uuid; 2],
    device: Uuid,
) -> Duration {
    let started = Instant::now();
    for slot in 0..3 {
        loop {
            let credential = credentials(sessions, device)
                .into_iter()
                .nth(slot)
                .expect("slot");
            match authenticator.authenticate(credential).await {
                Err(AuthenticatorError::Unauthenticated) => break,
                Ok(_) if started.elapsed() <= REVOCATION_BOUND => tokio::time::sleep(POLL).await,
                other => panic!("still accepted after {:?}: {other:?}", started.elapsed()),
            }
        }
    }
    started.elapsed()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn at_07_1_a_deleted_account_leaves_nothing_behind_after_a_purger_restart_mid_cycle() {
    // Arrange
    capture::install();
    let clock: Arc<dyn Clock> = Arc::new(SystemClock::new());
    let world = PurgeWorld::new(clock).await;
    let ports = Ports::new(&world);
    // The keyring check runs before any blob row exists, as at a first start.
    let wiring = world.wiring(world.api.clone()).await;
    let target = seed::account(&world, seed::files(FILES)).await;
    let account = target.account;
    let mut conn = world.fixture().await;
    let sessions = [
        live_session(&mut conn, account).await,
        live_session(&mut conn, account).await,
    ];
    let device = device_token(&mut conn, account).await;
    let file = first_file(&mut conn, account).await;
    let link = ports
        .shares
        .create(account, file, None, None)
        .await
        .expect("share link created");
    // A running API process: its authenticator has the credentials cached.
    let authenticator = ports.authenticator(&world);
    let [first, second, token] = credentials(sessions, device);
    let own = principal(&authenticator, first).await;
    principal(&authenticator, second).await;
    principal(&authenticator, token).await;
    let open_before = ports.shares.open(&link.token, VISITOR).await;
    let entered = Arc::new(Notify::new());
    let stuck = world.purger(
        wiring.accounts.clone(),
        record_hooks(
            wiring.hooks.clone(),
            &Log::default(),
            Some((1, Fault::HangAfterNext(entered.clone()))),
        ),
        world.api.clone(),
    );

    // Act
    let requested_at = world.clock.now_utc();
    let accepted = ports
        .deletion
        .request(&own, &account.to_string())
        .await
        .expect("deletion accepted");
    let answered_at = world.clock.now_utc();
    let purge_started = Instant::now();
    let refused = refused_after(&authenticator, sessions, device).await;
    {
        let cycle = stuck.run_cycle(world.clock.now_utc());
        tokio::pin!(cycle);
        tokio::select! {
            _ = &mut cycle => panic!("the cycle ended before the hang"),
            () = entered.notified() => {}
        }
    }
    drop(stuck);
    let state_after_crash = state(&mut conn, account).await;
    let fresh = world.wiring(world.new_pool().await).await;
    let purger = world.purger(
        fresh.accounts.clone(),
        fresh.hooks.clone(),
        fresh.db.clone(),
    );
    drain(&world, &purger, &world.sweeper(&fresh), account).await;
    let purge_took = purge_started.elapsed();
    let open_after = ports.shares.open(&link.token, VISITOR).await;

    // Assert
    let deadline = world.config.account_delete_deadline;
    let earliest = requested_at.checked_add(deadline).expect("fits");
    let latest = answered_at.checked_add(deadline).expect("fits");
    assert!(
        earliest <= accepted.deadline && accepted.deadline <= latest,
        "{accepted:?}"
    );
    assert!(refused <= REVOCATION_BOUND, "refused after {refused:?}");
    assert!(
        matches!(open_before, Ok(ShareOpen::Available { .. })),
        "{open_before:?}"
    );
    assert_eq!(state_after_crash.as_deref(), Some("deleting"));
    assert!(purge_took <= PURGE_TARGET, "purge took {purge_took:?}");
    let left = rows_of(&mut conn, account).await;
    assert_eq!(total(&left), 0, "{left:?}");
    assert_eq!(files_left(&world, &target.blobs), 0);
    assert_eq!(share_links(&mut conn, account).await, 0);
    assert!(
        matches!(open_after, Ok(ShareOpen::NotFound)),
        "{open_after:?}"
    );
    assert_eq!(deleted_audits(&mut conn, account).await, 1);
    let lines = capture::of_account(account);
    assert_eq!(capture::at_level(&lines, "error").len(), 0, "{lines:#?}");
    conn.close().await.expect("close");
    world.finish().await;
}
