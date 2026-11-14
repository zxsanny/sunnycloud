//! AZ-3818 AC-2, AC-3, AC-4 (IT-07-4, IT-07-5, ST-07-3): concurrent removals of two admins on two connections
//! never both pass, at least one active admin always remains, and no lock conflict reaches the caller. Real
//! 01 and 07a on a real PostgreSQL; a barrier releases both calls of a round together.

mod support;

use std::sync::Arc;
use std::time::{Duration, Instant};

use contracts::{
    AccountDeletionService, AccountState, AdminError, AdminService, DeletionError, PrincipalKind,
    Role,
};
use sqlx::PgConnection;
use support::delayed_accounts::DelayedAccounts;
use support::harness::{admin_actor, principal};
use support::seed::{admin, role_and_state};
use support::world::World;
use tokio::sync::Barrier;
use uuid::Uuid;

const DEMOTION_ROUNDS: usize = 100;
const MIXED_ROUNDS: usize = 200;
const LOCK_HOLD: Duration = Duration::from_millis(200);

/// How one call of a round ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Outcome {
    Passed,
    LastAdmin,
    /// 01 refuses `set_state` on an account that is already `deleting`; not a last-admin refusal.
    Refused,
}

fn admin_outcome(result: Result<(), AdminError>) -> Outcome {
    match result {
        Ok(()) => Outcome::Passed,
        Err(AdminError::LastAdmin) => Outcome::LastAdmin,
        Err(AdminError::Unexpected(_)) => Outcome::Refused,
        Err(error) => panic!("unexpected admin failure: {error:?}"),
    }
}

fn deletion_outcome<T>(result: Result<T, DeletionError>) -> Outcome {
    match result {
        Ok(_) => Outcome::Passed,
        Err(DeletionError::LastAdmin) => Outcome::LastAdmin,
        Err(error) => panic!("unexpected deletion failure: {error:?}"),
    }
}

/// The removals of IT-07-5.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Removal {
    DeleteA,
    DeleteB,
    DisableB,
    DemoteA,
    DemoteB,
}

const REMOVALS: [Removal; 5] = [
    Removal::DeleteA,
    Removal::DeleteB,
    Removal::DisableB,
    Removal::DemoteA,
    Removal::DemoteB,
];

struct Pair {
    a: Uuid,
    b: Uuid,
}

impl Pair {
    async fn seed(conn: &mut PgConnection) -> Pair {
        let pair = Pair {
            a: Uuid::new_v4(),
            b: Uuid::new_v4(),
        };
        admin(conn, pair.a).await;
        admin(conn, pair.b).await;
        pair
    }

    /// Both accounts back to active admins, as at the start of every round.
    async fn reset(&self, conn: &mut PgConnection) {
        sqlx::query(
            "UPDATE accounts SET role = 'admin', state = 'active', deletion_requested_at = NULL \
             WHERE id = ANY($1)",
        )
        .bind(vec![self.a, self.b])
        .execute(conn)
        .await
        .expect("reset the pair");
    }

    async fn active_admins(&self, conn: &mut PgConnection) -> usize {
        let mut count = 0;
        for id in [self.a, self.b] {
            if role_and_state(conn, id).await == ("admin".into(), "active".into()) {
                count += 1;
            }
        }
        count
    }

    /// One removal; A deletes itself through its session, B is deleted by the operator.
    async fn run(&self, world: &World, removal: Removal) -> Outcome {
        let (by_a, by_b) = (admin_actor(self.a), admin_actor(self.b));
        match removal {
            Removal::DeleteA => {
                let own = principal(self.a, PrincipalKind::Session, Role::Admin);
                deletion_outcome(world.deletion.request(&own, &self.a.to_string()).await)
            }
            Removal::DeleteB => deletion_outcome(world.deletion.request_as_operator(self.b).await),
            Removal::DisableB => admin_outcome(
                world
                    .admin
                    .update_user(&by_a, self.b, None, Some(AccountState::Disabled))
                    .await,
            ),
            Removal::DemoteA => admin_outcome(
                world
                    .admin
                    .update_user(&by_b, self.a, Some(Role::User), None)
                    .await,
            ),
            Removal::DemoteB => admin_outcome(
                world
                    .admin
                    .update_user(&by_a, self.b, Some(Role::User), None)
                    .await,
            ),
        }
    }
}

/// A demotes B and B demotes A, released together; each call's outcome and duration.
async fn demote_each_other(world: &World, pair: &Pair) -> [(Outcome, Duration); 2] {
    let barrier = Arc::new(Barrier::new(2));
    let call = async |caller: Uuid, target: Uuid| {
        barrier.wait().await;
        let started = Instant::now();
        let result = world
            .admin
            .update_user(&admin_actor(caller), target, Some(Role::User), None)
            .await;
        (admin_outcome(result), started.elapsed())
    };
    let (first, second) = tokio::join!(call(pair.a, pair.b), call(pair.b, pair.a));
    [first, second]
}

fn one_passed_one_refused(outcomes: [(Outcome, Duration); 2]) -> bool {
    let mut kinds = outcomes.map(|(outcome, _)| outcome);
    kinds.sort_by_key(|outcome| *outcome as u8);
    kinds == [Outcome::Passed, Outcome::LastAdmin]
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn of_two_admins_demoting_each_other_at_once_exactly_one_passes_in_every_round() {
    // Arrange
    let world = World::new().await;
    let mut conn = world.fixture().await;
    let pair = Pair::seed(&mut conn).await;

    // Act
    let mut rounds = Vec::with_capacity(DEMOTION_ROUNDS);
    for _ in 0..DEMOTION_ROUNDS {
        pair.reset(&mut conn).await;
        let outcomes = demote_each_other(&world, &pair).await;
        rounds.push((outcomes, pair.active_admins(&mut conn).await));
    }

    // Assert
    for (round, (outcomes, active)) in rounds.iter().enumerate() {
        assert!(
            one_passed_one_refused(*outcomes),
            "round {round}: {outcomes:?}"
        );
        assert_eq!(*active, 1, "round {round}");
    }
    drop(conn);
    world.finish().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mixed_concurrent_removals_always_leave_an_active_admin() {
    // Arrange
    let world = World::new().await;
    let mut conn = world.fixture().await;
    let pair = Pair::seed(&mut conn).await;
    let pairs: Vec<(Removal, Removal)> = REMOVALS
        .iter()
        .enumerate()
        .flat_map(|(i, first)| REMOVALS[i + 1..].iter().map(|second| (*first, *second)))
        .collect();

    // Act
    let mut rounds = Vec::with_capacity(MIXED_ROUNDS);
    for round in 0..MIXED_ROUNDS {
        let (first, second) = pairs[round % pairs.len()];
        pair.reset(&mut conn).await;
        let barrier = Barrier::new(2);
        let call = async |removal: Removal| {
            barrier.wait().await;
            pair.run(&world, removal).await
        };
        let outcomes = tokio::join!(call(first), call(second));
        let b_state = role_and_state(&mut conn, pair.b).await.1;
        rounds.push((
            [(first, outcomes.0), (second, outcomes.1)],
            pair.active_admins(&mut conn).await,
            b_state,
        ));
    }

    // Assert
    for (round, (calls, active, b_state)) in rounds.iter().enumerate() {
        assert!(
            *active >= 1,
            "round {round}: no active admin after {calls:?}"
        );
        for (removal, outcome) in calls {
            if *outcome == Outcome::Refused {
                let b_deleted = calls.contains(&(Removal::DeleteB, Outcome::Passed));
                assert!(
                    *removal == Removal::DisableB && b_deleted && b_state == "deleting",
                    "round {round}: {calls:?}"
                );
            }
        }
    }
    drop(conn);
    world.finish().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn with_a_delay_after_the_lock_the_second_demotion_waits_and_exactly_one_passes() {
    // Arrange
    let world = World::with_accounts(|identity| DelayedAccounts::wrap(identity, LOCK_HOLD)).await;
    let mut conn = world.fixture().await;
    let pair = Pair::seed(&mut conn).await;

    // Act
    let mut rounds = Vec::with_capacity(DEMOTION_ROUNDS);
    for _ in 0..DEMOTION_ROUNDS {
        pair.reset(&mut conn).await;
        rounds.push(demote_each_other(&world, &pair).await);
    }

    // Assert
    for (round, outcomes) in rounds.iter().enumerate() {
        assert!(
            one_passed_one_refused(*outcomes),
            "round {round}: {outcomes:?}"
        );
        let slower = outcomes[0].1.max(outcomes[1].1);
        assert!(
            slower >= LOCK_HOLD * 2,
            "round {round}: the second call did not wait for the first ({outcomes:?})"
        );
    }
    drop(conn);
    world.finish().await;
}
