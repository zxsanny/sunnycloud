use ipc_schema::{HoldKind, HoldView, PairId};

use super::{Fixture, fixture, strings};
use crate::delete_guard::{DeleteBatch, Verdict};
use crate::local_index::PairState;

fn hold(fixture: &Fixture, local: u32, remote: u32) -> Verdict {
    Verdict::Hold(HoldView {
        pair_id: PairId(fixture.pair_id),
        kind: HoldKind::MassDelete,
        local_deletes: local,
        remote_deletes: remote,
    })
}

#[test]
fn a_batch_of_199_local_deletes_is_released_without_a_hold() {
    // Arrange
    let f = fixture();
    let paths = f.add_files("docs", 199);

    // Act
    let verdict = f.guard.evaluate(
        f.pair_id,
        &DeleteBatch {
            local: &paths,
            remote: &[],
        },
    );

    // Assert
    assert_eq!(verdict.expect("evaluate"), Verdict::Release);
    assert_eq!(f.state(), PairState::Active);
}

#[test]
fn a_batch_of_200_local_deletes_is_held_and_none_is_released() {
    // Arrange
    let f = fixture();
    let paths = f.add_files("docs", 200);

    // Act
    let verdict = f.guard.evaluate(
        f.pair_id,
        &DeleteBatch {
            local: &paths,
            remote: &[],
        },
    );

    // Assert
    assert_eq!(verdict.expect("evaluate"), hold(&f, 200, 0));
    assert_eq!(f.state(), PairState::HoldMassDelete);
}

#[test]
fn a_batch_of_200_remote_deletes_is_held_and_none_is_released() {
    // Arrange
    let f = fixture();
    let paths = f.add_files("docs", 200);

    // Act
    let verdict = f.guard.evaluate(
        f.pair_id,
        &DeleteBatch {
            local: &[],
            remote: &paths,
        },
    );

    // Assert
    assert_eq!(verdict.expect("evaluate"), hold(&f, 0, 200));
    assert_eq!(f.state(), PairState::HoldMassDelete);
}

#[test]
fn each_direction_is_counted_on_its_own() {
    // Arrange
    let f = fixture();
    let paths = f.add_files("docs", 199);
    let others = f.add_files("pics", 199);

    // Act
    let verdict = f.guard.evaluate(
        f.pair_id,
        &DeleteBatch {
            local: &paths,
            remote: &others,
        },
    );

    // Assert
    assert_eq!(verdict.expect("evaluate"), Verdict::Release);
}

#[test]
fn a_deleted_directory_counts_its_200_descendant_files_individually() {
    // Arrange
    let f = fixture();
    f.add_files("album", 200);

    // Act
    let verdict = f.guard.evaluate(
        f.pair_id,
        &DeleteBatch {
            local: &strings(&["album"]),
            remote: &[],
        },
    );

    // Assert
    assert_eq!(verdict.expect("evaluate"), hold(&f, 200, 0));
}

#[test]
fn a_deleted_directory_with_199_descendant_files_is_released() {
    // Arrange
    let f = fixture();
    f.add_files("album", 199);

    // Act
    let verdict = f.guard.evaluate(
        f.pair_id,
        &DeleteBatch {
            local: &strings(&["album"]),
            remote: &[],
        },
    );

    // Assert
    assert_eq!(verdict.expect("evaluate"), Verdict::Release);
}

#[test]
fn nested_directories_are_not_counted_only_the_files_below_them() {
    // Arrange
    let f = fixture();
    f.add_files("album/2023", 100);
    f.add_files("album/2024", 100);

    // Act
    let verdict = f.guard.evaluate(
        f.pair_id,
        &DeleteBatch {
            local: &strings(&["album"]),
            remote: &[],
        },
    );

    // Assert
    assert_eq!(verdict.expect("evaluate"), hold(&f, 200, 0));
}

#[test]
fn a_directory_listed_together_with_its_files_counts_each_file_once() {
    // Arrange
    let f = fixture();
    let mut batch = f.add_files("album", 199);
    batch.push("album".to_owned());
    batch.extend(batch.clone());

    // Act
    let verdict = f.guard.evaluate(
        f.pair_id,
        &DeleteBatch {
            local: &batch,
            remote: &[],
        },
    );

    // Assert
    assert_eq!(verdict.expect("evaluate"), Verdict::Release);
}

#[test]
fn a_sibling_sharing_the_directory_name_as_a_prefix_is_not_a_descendant() {
    // Arrange
    let f = fixture();
    f.add_files("album", 100);
    f.add_files("album-old", 150);
    f.add_files("album0", 150);

    // Act
    let verdict = f.guard.evaluate(
        f.pair_id,
        &DeleteBatch {
            local: &strings(&["album"]),
            remote: &[],
        },
    );
    let under = f.index.entries_under(f.pair_id, "album").expect("under");

    // Assert
    assert_eq!(verdict.expect("evaluate"), Verdict::Release);
    assert_eq!(under.len(), 100);
}

#[test]
fn paths_the_index_does_not_know_count_as_one_file_each() {
    // Arrange
    let f = fixture();
    let unknown: Vec<String> = (0..200).map(|n| format!("never-synced/f{n}")).collect();

    // Act
    let verdict = f.guard.evaluate(
        f.pair_id,
        &DeleteBatch {
            local: &unknown,
            remote: &[],
        },
    );

    // Assert
    assert_eq!(verdict.expect("evaluate"), hold(&f, 200, 0));
}

#[test]
fn descendants_of_a_case_insensitive_pair_are_found_whatever_the_spelling_of_the_directory() {
    // Arrange
    let f = fixture();
    let pair_id = f
        .index
        .add_pair(&crate::local_index::NewPair {
            server_node_id: uuid::Uuid::from_u128(8),
            local_root: "/unused".to_owned(),
            rules: String::new(),
            root_sentinel: "s".to_owned(),
            case_insensitive: true,
        })
        .expect("add pair");
    let mut entry = f.add_entry_template();
    entry.pair_id = pair_id;
    entry.rel_path = "Docs/Report.txt".to_owned();
    f.index.upsert_entries(&[entry]).expect("upsert");

    // Act
    let under = f.index.entries_under(pair_id, "DOCS").expect("under");

    // Assert
    assert_eq!(under.len(), 1);
    assert_eq!(under[0].rel_path, "Docs/Report.txt");
}
