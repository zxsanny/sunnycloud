//! One invite, concurrent redemptions, one account (AZ-3685 AC-75, IT-01-8, NFT-SEC-48): in each of 200
//! iterations a fresh invite is redeemed by 2, then 16, distinct provider identities whose `complete` calls are
//! released together by a barrier and run on separate pool connections, with the real hooks of 02, 05 and 07a.

mod support;

use std::sync::Arc;

use contracts::{SignInError, SignInOutcome};
use sunnycloud_identity::IdentitySignIn;
use support::registration::{Rig, Rows, authorized};
use support::sign_in::{GITHUB, HOST};
use tokio::sync::Barrier;
use tokio::task::JoinSet;

const ITERATIONS: u64 = 200;
const MOST_RACERS: u32 = 16;

struct Tally {
    sessions: usize,
    invitation_required: usize,
    other: Vec<String>,
}

/// One iteration: a fresh invite, `racers` identities through `begin` and the provider, then every `complete`
/// at once.
async fn race(rig: &Rig, service: &Arc<IdentitySignIn>, iteration: u64, racers: u64) -> Tally {
    let token = format!("race-{racers}-{iteration}");
    let mut callbacks = Vec::new();
    for racer in 0..racers {
        let subject = (racers * 1_000_000 + iteration * 100 + racer).to_string();
        callbacks.push(authorized(&rig.world, GITHUB, &subject, Some(&token), |_| {}).await);
    }
    let barrier = Arc::new(Barrier::new(callbacks.len()));
    let mut calls = JoinSet::new();
    for (callback, binding) in callbacks {
        let (service, barrier) = (Arc::clone(service), Arc::clone(&barrier));
        calls.spawn(async move {
            barrier.wait().await;
            service.complete(GITHUB, callback, &binding, HOST).await
        });
    }
    let mut tally = Tally {
        sessions: 0,
        invitation_required: 0,
        other: Vec::new(),
    };
    for result in calls.join_all().await {
        match result {
            Ok(SignInOutcome::Session { .. }) => tally.sessions += 1,
            Err(SignInError::InvitationRequired) => tally.invitation_required += 1,
            other => tally.other.push(format!("{other:?}")),
        }
    }
    tally
}

fn plus_one(rows: Rows) -> Rows {
    Rows {
        accounts: rows.accounts + 1,
        identities: rows.identities + 1,
        root_nodes: rows.root_nodes + 1,
        feeds: rows.feeds + 1,
        quotas: rows.quotas + 1,
        redeemed_invites: rows.redeemed_invites + 1,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn concurrent_redemptions_of_one_invite_create_exactly_one_account_and_the_losers_leave_no_row()
 {
    // Arrange
    let mut rig = Rig::unregistered(MOST_RACERS + 4, |_| {}).await;
    let service = Arc::new(rig.new_service());
    service
        .register_hooks(rig.real_hooks())
        .expect("register the hooks");

    // Act
    let mut failures = Vec::new();
    for racers in [2, u64::from(MOST_RACERS)] {
        for iteration in 0..ITERATIONS {
            rig.member_invite(&format!("race-{racers}-{iteration}"))
                .await;
            let before = rig.rows().await;
            let tally = race(&rig, &service, iteration, racers).await;
            let after = rig.rows().await;
            let expected = (
                1,
                racers as usize - 1,
                Vec::<String>::new(),
                plus_one(before),
            );
            let seen = (
                tally.sessions,
                tally.invitation_required,
                tally.other,
                after,
            );
            if seen != expected {
                failures.push(format!("{racers} racers, iteration {iteration}: {seen:?}"));
            }
        }
    }

    // Assert
    assert!(
        failures.is_empty(),
        "{} failed iterations: {failures:#?}",
        failures.len()
    );
    drop(service);
    rig.finish().await;
}
