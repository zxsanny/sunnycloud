use std::fs;
use std::sync::Arc;

use ipc_schema::{HoldKind, HoldView, PairId};

use super::{Fixture, fixture, guard_of, strings};
use crate::delete_guard::{
    DeleteBatch, GuardError, HeldDeletes, RootCheck, Verdict, hold_view, uploads_allowed,
};
use crate::local_index::{LocalIndexStore, PairState};

fn hold_200_local(f: &Fixture) -> Vec<String> {
    let paths = f.add_files("docs", 200);
    let verdict = f
        .guard
        .evaluate(
            f.pair_id,
            &DeleteBatch {
                local: &paths,
                remote: &[],
            },
        )
        .expect("evaluate");
    assert!(matches!(verdict, Verdict::Hold(_)));
    paths
}

#[test]
fn the_hold_and_its_detail_survive_a_restart() {
    // Arrange
    let f = fixture();
    let paths = hold_200_local(&f);
    let before = f.index.get_pair(f.pair_id).expect("read").expect("pair");
    let Fixture {
        data, guard, index, ..
    } = f;
    drop(guard);
    drop(index);

    // Act
    let reopened = Arc::new(LocalIndexStore::open(data.path()).expect("reopen index"));
    let after = reopened.get_pair(1).expect("read").expect("pair");
    let view = hold_view(&after).expect("hold view");

    // Assert
    assert_eq!(after.state, PairState::HoldMassDelete);
    assert_eq!(after.hold_detail, before.hold_detail);
    assert_eq!(
        view,
        Some(HoldView {
            pair_id: PairId(1),
            kind: HoldKind::MassDelete,
            local_deletes: 200,
            remote_deletes: 0,
        })
    );
    let resolved = guard_of(&reopened)
        .resolve(1, ipc_schema::HoldDecision::ConfirmDeletes)
        .expect("resolve after restart");
    assert_eq!(
        resolved,
        crate::delete_guard::Resolution::Release(HeldDeletes {
            local: sorted(paths),
            remote: vec![],
        })
    );
}

#[test]
fn a_restart_in_the_middle_of_a_hold_keeps_holding_new_deletes() {
    // Arrange
    let f = fixture();
    hold_200_local(&f);
    let extra = f.add_files("later", 3);
    let Fixture {
        data, guard, index, ..
    } = f;
    drop(guard);
    drop(index);
    let reopened = Arc::new(LocalIndexStore::open(data.path()).expect("reopen index"));

    // Act
    let verdict = guard_of(&reopened).evaluate(
        1,
        &DeleteBatch {
            local: &[],
            remote: &extra,
        },
    );

    // Assert
    assert_eq!(
        verdict.expect("evaluate"),
        Verdict::Hold(HoldView {
            pair_id: PairId(1),
            kind: HoldKind::MassDelete,
            local_deletes: 200,
            remote_deletes: 3,
        })
    );
}

#[test]
fn a_held_pair_releases_nothing_even_for_a_single_delete() {
    // Arrange
    let f = fixture();
    hold_200_local(&f);
    let one = f.add_files("one", 1);

    // Act
    let verdict = f.guard.evaluate(
        f.pair_id,
        &DeleteBatch {
            local: &one,
            remote: &[],
        },
    );

    // Assert
    assert!(matches!(verdict.expect("evaluate"), Verdict::Hold(v) if v.local_deletes == 201));
}

#[test]
fn a_new_file_during_the_hold_neither_ends_nor_changes_it() {
    // Arrange
    let f = fixture();
    hold_200_local(&f);
    let before = f.index.get_pair(f.pair_id).expect("read").expect("pair");
    fs::write(f.root.path().join("brand-new.txt"), b"new").expect("new file");

    // Act
    let check = f.guard.check_root(&f.fs, f.pair_id).expect("check root");

    // Assert
    assert_eq!(check, RootCheck::Healthy);
    assert!(uploads_allowed(f.state()));
    let after = f.index.get_pair(f.pair_id).expect("read").expect("pair");
    assert_eq!(after, before);
}

#[test]
fn uploads_continue_in_a_mass_delete_hold_but_not_when_the_root_is_missing_or_paused() {
    // Arrange
    let states = [
        (PairState::Active, true),
        (PairState::HoldMassDelete, true),
        (PairState::RootMissing, false),
        (PairState::Paused, false),
    ];

    for (state, expected) in states {
        // Act
        let allowed = uploads_allowed(state);

        // Assert
        assert_eq!(allowed, expected, "{state:?}");
    }
}

#[test]
fn a_hold_detail_that_cannot_be_read_keeps_everything_held() {
    // Arrange
    let f = fixture();
    f.index
        .set_pair_state(f.pair_id, PairState::HoldMassDelete, Some("not json"))
        .expect("corrupt the detail");

    // Act
    let evaluated = f.guard.evaluate(
        f.pair_id,
        &DeleteBatch {
            local: &strings(&["a"]),
            remote: &[],
        },
    );
    let resolved = f
        .guard
        .resolve(f.pair_id, ipc_schema::HoldDecision::ConfirmDeletes);

    // Assert
    assert!(matches!(evaluated, Err(GuardError::HoldDetailUnreadable)));
    assert!(matches!(resolved, Err(GuardError::HoldDetailUnreadable)));
    assert_eq!(f.state(), PairState::HoldMassDelete);
}

#[test]
fn a_paused_pair_is_not_evaluated() {
    // Arrange
    let f = fixture();
    f.index
        .set_pair_state(f.pair_id, PairState::Paused, None)
        .expect("pause");

    // Act
    let verdict = f.guard.evaluate(
        f.pair_id,
        &DeleteBatch {
            local: &strings(&["a"]),
            remote: &[],
        },
    );

    // Assert
    assert!(matches!(verdict, Err(GuardError::Paused)));
    assert_eq!(f.state(), PairState::Paused);
}

#[test]
fn an_unknown_pair_is_an_error_not_a_release() {
    // Arrange
    let f = fixture();

    // Act
    let verdict = f.guard.evaluate(
        999,
        &DeleteBatch {
            local: &[],
            remote: &[],
        },
    );

    // Assert
    assert!(matches!(verdict, Err(GuardError::UnknownPair)));
}

fn sorted(mut paths: Vec<String>) -> Vec<String> {
    paths.sort();
    paths
}
