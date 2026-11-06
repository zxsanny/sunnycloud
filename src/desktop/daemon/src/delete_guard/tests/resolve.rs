use ipc_schema::HoldDecision;

use super::{Fixture, fixture};
use crate::delete_guard::{DeleteBatch, GuardError, HeldDeletes, Resolution, Verdict};
use crate::local_index::PairState;

/// 200 local deletes (a directory) and 3 remote deletes: the pair is held with both directions in the detail.
fn held_pair(f: &Fixture) -> HeldDeletes {
    f.add_files("album", 200);
    f.add_files("notes", 3);
    let remote = vec![
        "notes/f1".to_owned(),
        "notes/f2".to_owned(),
        "notes/f3".to_owned(),
    ];
    let local = vec!["album".to_owned()];
    let verdict = f
        .guard
        .evaluate(
            f.pair_id,
            &DeleteBatch {
                local: &local,
                remote: &remote,
            },
        )
        .expect("evaluate");
    assert!(matches!(verdict, Verdict::Hold(_)));
    HeldDeletes { local, remote }
}

#[test]
fn confirming_releases_exactly_the_held_deletes_and_returns_the_pair_to_active() {
    // Arrange
    let f = fixture();
    let held = held_pair(&f);

    // Act
    let resolution = f.guard.resolve(f.pair_id, HoldDecision::ConfirmDeletes);

    // Assert
    assert_eq!(resolution.expect("resolve"), Resolution::Release(held));
    assert_eq!(f.state(), PairState::Active);
    let pair = f.index.get_pair(f.pair_id).expect("read").expect("pair");
    assert_eq!(pair.hold_detail, None);
}

#[test]
fn restoring_hands_exactly_the_held_paths_to_the_applier_and_releases_none() {
    // Arrange
    let f = fixture();
    let held = held_pair(&f);

    // Act
    let resolution = f.guard.resolve(f.pair_id, HoldDecision::RestoreFromServer);

    // Assert
    assert_eq!(resolution.expect("resolve"), Resolution::Restore(held));
    assert_eq!(f.state(), PairState::Active);
}

#[test]
fn resolving_a_pair_without_a_hold_is_no_such_hold() {
    // Arrange
    let f = fixture();

    // Act
    let confirm = f.guard.resolve(f.pair_id, HoldDecision::ConfirmDeletes);
    let restore = f.guard.resolve(f.pair_id, HoldDecision::RestoreFromServer);

    // Assert
    assert!(matches!(confirm, Err(GuardError::NoSuchHold)));
    assert!(matches!(restore, Err(GuardError::NoSuchHold)));
}

#[test]
fn a_hold_is_resolved_once() {
    // Arrange
    let f = fixture();
    held_pair(&f);
    f.guard
        .resolve(f.pair_id, HoldDecision::ConfirmDeletes)
        .expect("first resolve");

    // Act
    let second = f.guard.resolve(f.pair_id, HoldDecision::ConfirmDeletes);

    // Assert
    assert!(matches!(second, Err(GuardError::NoSuchHold)));
}

#[test]
fn a_missing_root_hold_has_no_decision() {
    // Arrange
    let f = fixture();
    f.index
        .set_pair_state(f.pair_id, PairState::RootMissing, None)
        .expect("hold");

    // Act
    let resolution = f.guard.resolve(f.pair_id, HoldDecision::ConfirmDeletes);

    // Assert
    assert!(matches!(resolution, Err(GuardError::RootHold)));
    assert_eq!(f.state(), PairState::RootMissing);
}

#[test]
fn after_resolving_the_next_small_batch_passes_and_a_large_one_holds_again() {
    // Arrange
    let f = fixture();
    held_pair(&f);
    f.guard
        .resolve(f.pair_id, HoldDecision::RestoreFromServer)
        .expect("resolve");
    let small = vec!["notes/f1".to_owned()];
    let large = vec!["album".to_owned()];

    // Act
    let first = f.guard.evaluate(
        f.pair_id,
        &DeleteBatch {
            local: &small,
            remote: &[],
        },
    );
    let second = f.guard.evaluate(
        f.pair_id,
        &DeleteBatch {
            local: &large,
            remote: &[],
        },
    );

    // Assert
    assert_eq!(first.expect("small"), Verdict::Release);
    assert!(matches!(second.expect("large"), Verdict::Hold(_)));
}
