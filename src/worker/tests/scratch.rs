//! AZ-3870: tree and modes (AC-1), start-up sweep (AC-2), removal on every path (AC-3), the free-space
//! refusal (AC-6). The hostile-output matrix (AC-4, AC-5) is in `src/scratch/hostile_tests.rs`, where it can
//! run through both name resolvers.

use std::fs;
use std::os::unix::fs::{MetadataExt, symlink};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;

use rustix::fs::statvfs;
use rustix::process::{getegid, geteuid, getgroups};
use sunnycloud_worker::{ScratchDir, ScratchError, ScratchRoot, sweep_all};
use tempfile::TempDir;
use uuid::Uuid;

fn own_gid(dir: &Path) -> u32 {
    fs::metadata(dir).expect("metadata").gid()
}

fn root() -> (TempDir, ScratchRoot) {
    let dir = tempfile::tempdir().expect("temporary root");
    let root = ScratchRoot::new(dir.path(), own_gid(dir.path()));
    (dir, root)
}

fn job_id(n: u128) -> Uuid {
    Uuid::from_u128(n)
}

fn entries(dir: &Path) -> usize {
    fs::read_dir(dir).expect("list").count()
}

fn every_path(dir: &Path, found: &mut Vec<std::path::PathBuf>) {
    for entry in fs::read_dir(dir).expect("list") {
        let path = entry.expect("entry").path();
        found.push(path.clone());
        if path.is_dir() {
            every_path(&path, found);
        }
    }
}

#[test]
fn tree_has_the_documented_modes_and_no_world_bits() {
    // Arrange
    let (_guard, root) = root();

    // Act
    let job = root.create(job_id(1)).expect("create");
    drop(job.create_source().expect("source"));

    // Assert
    let mode = |path: &Path| fs::metadata(path).expect("metadata").mode() & 0o7777;
    assert_eq!(mode(job.path()), 0o750);
    assert_eq!(mode(&job.path().join("in")), 0o750);
    assert_eq!(mode(&job.path().join("out")), 0o770);
    assert_eq!(mode(&job.source_path()), 0o440);
    let mut all = vec![job.path().to_path_buf()];
    every_path(job.path(), &mut all);
    for path in all {
        assert_eq!(mode(&path) & 0o007, 0, "world bits on {path:?}");
    }
}

#[test]
fn tree_carries_the_process_group_when_it_is_the_decoder_group() {
    // Arrange
    let (guard, root) = root();
    let gid = own_gid(guard.path());

    // Act
    let job = root.create(job_id(1)).expect("create");
    drop(job.create_source().expect("source"));

    // Assert
    for sub in ["", "in", "out", "in/source"] {
        let path = job.path().join(sub);
        assert_eq!(fs::metadata(&path).expect("metadata").gid(), gid, "{sub}");
    }
}

#[test]
fn tree_carries_a_different_group_the_process_belongs_to() {
    // Arrange
    let primary = getegid();
    let Some(other) = getgroups()
        .expect("supplementary groups")
        .into_iter()
        .find(|gid| *gid != primary)
    else {
        eprintln!("no supplementary group: only the process's own gid is covered");
        return;
    };
    let dir = tempfile::tempdir().expect("temporary root");
    let root = ScratchRoot::new(dir.path(), other.as_raw());

    // Act
    let job = root.create(job_id(1)).expect("create");
    drop(job.create_source().expect("source"));

    // Assert
    for sub in ["", "in", "out", "in/source"] {
        let path = job.path().join(sub);
        assert_eq!(
            fs::metadata(&path).expect("metadata").gid(),
            other.as_raw(),
            "{sub}"
        );
    }
}

#[test]
fn input_file_is_created_once_and_is_not_writable_afterwards() {
    // Arrange
    let (_guard, root) = root();
    let job = root.create(job_id(1)).expect("create");
    drop(job.create_source().expect("source"));

    // Act
    let second = job.create_source();

    // Assert
    assert!(matches!(second, Err(ScratchError::Io { .. })));
    let mode = fs::metadata(job.source_path()).expect("metadata").mode();
    assert_eq!(mode & 0o222, 0);
}

#[test]
fn creating_the_same_job_twice_fails_and_keeps_the_first_tree() {
    // Arrange
    let (_guard, root) = root();
    let first = root.create(job_id(1)).expect("create");

    // Act
    let second = root.create(job_id(1));

    // Assert
    assert!(matches!(second, Err(ScratchError::Io { .. })));
    assert!(first.path().join("out").is_dir());
}

#[test]
fn a_failed_setup_removes_the_half_built_tree() {
    // Arrange
    if geteuid().is_root() {
        eprintln!("running as root: chown to a foreign group cannot be refused");
        return;
    }
    let dir = tempfile::tempdir().expect("temporary root");
    let root = ScratchRoot::new(dir.path(), 3_999_999_999);

    // Act
    let result = root.create(job_id(1));

    // Assert
    assert!(matches!(result, Err(ScratchError::Io { .. })));
    assert_eq!(entries(dir.path()), 0);
}

#[test]
fn sweep_empties_both_roots_and_leaves_symlink_targets_alone() {
    // Arrange
    let (interactive_dir, interactive) = root();
    let (general_dir, general) = root();
    let outside = tempfile::tempdir().expect("outside");
    let precious = outside.path().join("precious");
    fs::write(&precious, b"keep").expect("precious file");
    for dir in [interactive_dir.path(), general_dir.path()] {
        let job = dir.join(job_id(7).to_string());
        fs::create_dir_all(job.join("out")).expect("leftover tree");
        fs::write(job.join("out/result.webp"), b"plaintext").expect("leftover output");
        fs::write(dir.join("stray"), b"stray").expect("stray file");
        symlink(&precious, dir.join("link-to-file")).expect("link");
        symlink(outside.path(), dir.join("link-to-dir")).expect("link");
    }

    // Act
    sweep_all(&[interactive, general]).expect("sweep");

    // Assert
    assert_eq!(entries(interactive_dir.path()), 0);
    assert_eq!(entries(general_dir.path()), 0);
    assert_eq!(fs::read(&precious).expect("precious"), b"keep");
}

#[test]
fn sweep_of_a_missing_root_is_an_error() {
    // Arrange
    let (guard, _) = root();
    let missing = ScratchRoot::new(guard.path().join("absent"), 0);

    // Act
    let result = sweep_all(&[missing]);

    // Assert
    assert!(matches!(result, Err(ScratchError::Io { .. })));
}

#[test]
fn explicit_remove_deletes_the_tree_and_is_repeatable() {
    // Arrange
    let (guard, root) = root();
    let mut job = root.create(job_id(1)).expect("create");
    drop(job.create_source().expect("source"));

    // Act
    job.remove().expect("remove");
    let again = job.remove();

    // Assert
    assert!(again.is_ok());
    assert_eq!(entries(guard.path()), 0);
}

fn finish_with(outcome: Result<(), &'static str>, root: &ScratchRoot) -> Result<(), &'static str> {
    let job = root.create(job_id(2)).expect("create");
    drop(job.create_source().expect("source"));
    outcome?;
    Ok(())
}

#[test]
fn tree_is_gone_after_success_and_after_an_early_error_return() {
    for outcome in [Ok(()), Err("permanent failure"), Err("lease lost")] {
        // Arrange
        let (guard, root) = root();

        // Act
        let returned = finish_with(outcome, &root);

        // Assert
        assert_eq!(returned, outcome);
        assert_eq!(entries(guard.path()), 0, "{outcome:?}");
    }
}

#[test]
fn tree_is_gone_after_a_panic_unwinds_through_it() {
    // Arrange
    let (guard, root) = root();

    // Act
    let result = catch_unwind(AssertUnwindSafe(|| {
        let job: ScratchDir = root.create(job_id(3)).expect("create");
        drop(job.create_source().expect("source"));
        panic!("job task panicked");
    }));

    // Assert
    assert!(result.is_err());
    assert_eq!(entries(guard.path()), 0);
}

#[test]
fn clear_out_empties_the_output_directory_and_keeps_its_mode() {
    // Arrange
    let (_guard, root) = root();
    let job = root.create(job_id(1)).expect("create");
    let out = job.path().join("out");
    let outside = tempfile::tempdir().expect("outside");
    let precious = outside.path().join("precious");
    fs::write(&precious, b"keep").expect("precious file");
    fs::write(out.join("result.webp"), b"first render").expect("output");
    fs::create_dir_all(out.join("work/deeper")).expect("decoder scratch");
    symlink(&precious, out.join("link")).expect("link");
    symlink(outside.path(), out.join("dirlink")).expect("link");

    // Act
    job.clear_out().expect("clear");

    // Assert
    assert_eq!(entries(&out), 0);
    assert_eq!(fs::metadata(&out).expect("metadata").mode() & 0o7777, 0o770);
    assert_eq!(fs::read(&precious).expect("precious"), b"keep");
}

#[test]
fn input_larger_than_the_free_space_is_refused_before_anything_is_created() {
    // Arrange
    let (guard, root) = root();
    let stat = statvfs(guard.path()).expect("statvfs");
    let free = stat.f_bavail.saturating_mul(stat.f_frsize);
    let declared = free.saturating_add(1 << 30);

    // Act
    let result = root.ensure_fits(declared);

    // Assert
    match result {
        Err(ScratchError::InputTooLarge {
            declared: d,
            free: f,
        }) => {
            assert_eq!(d, declared);
            assert!(f <= free.saturating_add(1 << 30));
        }
        other => panic!("expected input_too_large, got {other:?}"),
    }
    assert_eq!(entries(guard.path()), 0);
}

#[test]
fn input_within_the_free_space_is_accepted() {
    // Arrange
    let (_guard, root) = root();

    // Act
    let result = root.ensure_fits(0);

    // Assert
    assert!(result.is_ok());
}
