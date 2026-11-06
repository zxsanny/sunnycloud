use std::fs;

use ipc_schema::{HoldKind, HoldView, PairId};
use uuid::Uuid;

use super::{Fixture, fixture};
use crate::delete_guard::{
    DeleteBatch, RootCheck, RootFault, SENTINEL_NAME, Verdict, create_sentinel,
};
use crate::local_index::PairState;
use crate::platform_fs::PlatformFs;

fn check(f: &Fixture) -> RootCheck {
    f.guard.check_root(&f.fs, f.pair_id).expect("check root")
}

fn assert_root_missing_releases_nothing(f: &Fixture, fault: RootFault) {
    assert_eq!(check(f), RootCheck::Faulted(fault));
    assert_eq!(f.state(), PairState::RootMissing);
    let everything = f.index.entries_under(f.pair_id, "docs").expect("entries");
    let paths: Vec<String> = everything.into_iter().map(|e| e.rel_path).collect();
    let local = f.guard.evaluate(
        f.pair_id,
        &DeleteBatch {
            local: &paths,
            remote: &[],
        },
    );
    let remote = f.guard.evaluate(
        f.pair_id,
        &DeleteBatch {
            local: &[],
            remote: &paths,
        },
    );
    let expected = Verdict::Hold(HoldView {
        pair_id: PairId(f.pair_id),
        kind: HoldKind::RootMissing,
        local_deletes: 0,
        remote_deletes: 0,
    });
    assert_eq!(local.expect("local batch"), expected);
    assert_eq!(remote.expect("remote batch"), expected);
}

#[test]
fn a_root_with_its_sentinel_and_files_is_healthy() {
    // Arrange
    let f = fixture();
    f.add_files("docs", 3);

    // Act
    let result = check(&f);

    // Assert
    assert_eq!(result, RootCheck::Healthy);
    assert_eq!(f.state(), PairState::Active);
}

#[test]
fn a_new_pair_with_an_empty_root_and_an_empty_index_is_healthy() {
    // Arrange
    let f = fixture();

    // Act
    let result = check(&f);

    // Assert
    assert_eq!(result, RootCheck::Healthy);
}

#[test]
fn a_root_renamed_away_is_root_missing_and_releases_no_delete_in_either_direction() {
    // Arrange
    let f = fixture();
    f.add_files("docs", 300);
    let moved = f.root.path().with_extension("moved");
    fs::rename(f.root.path(), &moved).expect("rename the root away");

    // Act
    let result = check(&f);

    // Assert
    assert_eq!(result, RootCheck::Faulted(RootFault::Unreachable));
    assert_root_missing_releases_nothing(&f, RootFault::Unreachable);
    fs::rename(&moved, f.root.path()).expect("restore the root for cleanup");
}

#[test]
fn an_unmounted_root_left_as_an_empty_directory_is_root_missing() {
    // Arrange
    let f = fixture();
    f.add_files("docs", 300);
    f.empty_root();
    fs::remove_file(f.sentinel_path()).expect("sentinel is on the unmounted volume");

    // Act
    let result = check(&f);

    // Assert
    assert_eq!(result, RootCheck::Faulted(RootFault::SentinelMissing));
    assert_root_missing_releases_nothing(&f, RootFault::SentinelMissing);
}

#[test]
fn a_swapped_volume_with_another_sentinel_id_is_root_missing() {
    // Arrange
    let f = fixture();
    f.add_files("docs", 300);
    fs::write(f.sentinel_path(), Uuid::new_v4().to_string()).expect("other volume's sentinel");

    // Act
    let result = check(&f);

    // Assert
    assert_eq!(result, RootCheck::Faulted(RootFault::SentinelMismatch));
    assert_root_missing_releases_nothing(&f, RootFault::SentinelMismatch);
}

#[test]
fn a_sentinel_that_is_too_long_to_be_an_id_never_matches() {
    // Arrange
    let f = fixture();
    let padded = format!("{}{}", f.sentinel, "0".repeat(100));
    fs::write(f.sentinel_path(), padded).expect("long sentinel");

    // Act
    let result = check(&f);

    // Assert
    assert_eq!(result, RootCheck::Faulted(RootFault::SentinelMismatch));
}

#[test]
fn a_sentinel_replaced_by_a_directory_is_root_missing() {
    // Arrange
    let f = fixture();
    fs::remove_file(f.sentinel_path()).expect("remove sentinel");
    fs::create_dir(f.sentinel_path()).expect("directory in its place");

    // Act
    let result = check(&f);

    // Assert
    assert_eq!(result, RootCheck::Faulted(RootFault::Unreachable));
    assert_eq!(f.state(), PairState::RootMissing);
}

#[test]
fn an_emptied_root_while_the_index_lists_files_is_root_missing() {
    // Arrange
    let f = fixture();
    f.add_files("docs", 300);
    f.empty_root();

    // Act
    let result = check(&f);

    // Assert
    assert_eq!(result, RootCheck::Faulted(RootFault::EmptyWithIndex));
    assert_root_missing_releases_nothing(&f, RootFault::EmptyWithIndex);
}

#[test]
fn a_root_holding_only_a_file_with_a_non_utf8_name_is_not_empty() {
    // Arrange
    use std::os::unix::ffi::OsStrExt;
    let f = fixture();
    f.add_files("docs", 3);
    f.empty_root();
    let odd = std::ffi::OsStr::from_bytes(b"caf\xe9.txt");
    fs::write(f.root.path().join(odd), b"x").expect("non-utf8 file");

    // Act
    let result = check(&f);

    // Assert
    assert_eq!(result, RootCheck::Healthy);
}

#[test]
fn a_root_that_comes_back_returns_the_pair_to_active() {
    // Arrange
    let f = fixture();
    f.add_files("docs", 3);
    let moved = f.root.path().with_extension("moved");
    fs::rename(f.root.path(), &moved).expect("rename away");
    assert_eq!(check(&f), RootCheck::Faulted(RootFault::Unreachable));
    fs::rename(&moved, f.root.path()).expect("rename back");

    // Act
    let result = check(&f);

    // Assert
    assert_eq!(result, RootCheck::Healthy);
    assert_eq!(f.state(), PairState::Active);
}

#[test]
fn a_mass_delete_hold_is_kept_when_the_root_also_fails() {
    // Arrange
    let f = fixture();
    let paths = f.add_files("docs", 200);
    f.guard
        .evaluate(
            f.pair_id,
            &DeleteBatch {
                local: &paths,
                remote: &[],
            },
        )
        .expect("hold");
    fs::remove_file(f.sentinel_path()).expect("remove sentinel");

    // Act
    let result = check(&f);

    // Assert
    assert_eq!(result, RootCheck::Faulted(RootFault::SentinelMissing));
    assert_eq!(f.state(), PairState::HoldMassDelete);
}

#[test]
fn a_paused_pair_keeps_its_state_when_the_root_fails() {
    // Arrange
    let f = fixture();
    f.index
        .set_pair_state(f.pair_id, PairState::Paused, None)
        .expect("pause");
    fs::remove_file(f.sentinel_path()).expect("remove sentinel");

    // Act
    let result = check(&f);

    // Assert
    assert_eq!(result, RootCheck::Faulted(RootFault::SentinelMissing));
    assert_eq!(f.state(), PairState::Paused);
}

#[test]
fn create_sentinel_writes_a_fresh_id_and_replaces_an_old_one() {
    // Arrange
    let f = fixture();
    let root = f.fs.open_root(f.root.path()).expect("open root");

    // Act
    let second = create_sentinel(&f.fs, &root).expect("second sentinel");

    // Assert
    assert_ne!(second, f.sentinel);
    assert_eq!(second.len(), 36);
    let on_disk = fs::read_to_string(f.root.path().join(SENTINEL_NAME)).expect("read sentinel");
    assert_eq!(on_disk, second);
    let leftovers = fs::read_dir(f.root.path()).expect("list").count();
    assert_eq!(leftovers, 1, "no temp file is left next to the sentinel");
}
