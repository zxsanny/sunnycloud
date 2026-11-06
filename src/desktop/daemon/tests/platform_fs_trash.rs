#![cfg(target_os = "linux")]

mod support;

use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use sunnycloud_daemon::platform_fs::linux::LinuxFs;
use sunnycloud_daemon::platform_fs::{FsError, PlatformFs};
use support::{modes, root_of};

fn uid() -> u32 {
    rustix::process::geteuid().as_raw()
}

fn fs_with_home(mode_fs: LinuxFs, data_home: &Path) -> LinuxFs {
    mode_fs.with_data_home(data_home.to_path_buf())
}

fn info_of(trash: &Path, name: &str) -> String {
    fs::read_to_string(trash.join("info").join(format!("{name}.trashinfo"))).expect("trashinfo")
}

fn assert_deletion_date(info: &str) {
    let date = info
        .lines()
        .find_map(|l| l.strip_prefix("DeletionDate="))
        .expect("DeletionDate line");
    let b = date.as_bytes();
    let digits = [0, 1, 2, 3, 5, 6, 8, 9, 11, 12, 14, 15, 17, 18];
    assert_eq!(b.len(), 19, "{date}");
    assert!(digits.iter().all(|&i| b[i].is_ascii_digit()), "{date}");
    assert_eq!(
        (b[4], b[7], b[10], b[13], b[16]),
        (b'-', b'-', b'T', b':', b':'),
        "{date}"
    );
}

#[test]
fn remote_delete_moves_the_file_into_the_home_trash_with_an_info_file() {
    for (mode, base) in modes() {
        // Arrange
        let data_home = tempfile::tempdir().expect("data home");
        let tmp = tempfile::tempdir().expect("sync root");
        let fs_impl = fs_with_home(base, data_home.path());
        fs::create_dir(tmp.path().join("my dir")).expect("dir");
        fs::write(tmp.path().join("my dir/doomed file.txt"), b"precious").expect("file");
        let root = root_of(&fs_impl, tmp.path());
        let before = fs_impl
            .file_id(&root, "my dir/doomed file.txt")
            .expect("id");

        // Act
        fs_impl
            .trash(&root, "my dir/doomed file.txt")
            .expect("trash");

        // Assert
        let trash = data_home.path().join("Trash");
        assert!(
            !tmp.path().join("my dir/doomed file.txt").exists(),
            "{mode}"
        );
        let moved = trash.join("files/doomed file.txt");
        assert_eq!(fs::read(&moved).expect("data"), b"precious", "{mode}");
        let meta = fs::metadata(&moved).expect("meta");
        assert_eq!(
            (meta.dev(), meta.ino()),
            (before.volume, before.index),
            "{mode}: moved, not copied"
        );
        let info = info_of(&trash, "doomed file.txt");
        let original = tmp
            .path()
            .canonicalize()
            .expect("canon")
            .join("my dir/doomed file.txt");
        let expected_path = original.to_str().expect("utf8").replace(' ', "%20");
        assert!(info.starts_with("[Trash Info]\n"), "{mode}: {info}");
        assert!(
            info.contains(&format!("Path={expected_path}\n")),
            "{mode}: {info}"
        );
        assert_deletion_date(&info);
        assert_eq!(
            fs::metadata(&trash).expect("trash").mode() & 0o777,
            0o700,
            "{mode}"
        );
    }
}

#[test]
fn a_second_delete_of_the_same_name_gets_a_new_trash_name_and_overwrites_nothing() {
    for (mode, base) in modes() {
        // Arrange
        let data_home = tempfile::tempdir().expect("data home");
        let tmp = tempfile::tempdir().expect("sync root");
        let fs_impl = fs_with_home(base, data_home.path());
        let root = root_of(&fs_impl, tmp.path());
        fs::write(tmp.path().join("a.txt"), b"first").expect("file");
        fs_impl.trash(&root, "a.txt").expect("first trash");
        fs::write(tmp.path().join("a.txt"), b"second").expect("file again");

        // Act
        fs_impl.trash(&root, "a.txt").expect("second trash");

        // Assert
        let trash = data_home.path().join("Trash");
        assert_eq!(
            fs::read(trash.join("files/a.txt")).expect("first"),
            b"first",
            "{mode}"
        );
        assert_eq!(
            fs::read(trash.join("files/a.txt.2")).expect("second"),
            b"second",
            "{mode}"
        );
        assert!(trash.join("info/a.txt.2.trashinfo").is_file(), "{mode}");
    }
}

#[test]
fn a_name_near_the_length_limit_still_gets_an_info_file() {
    // Arrange
    let data_home = tempfile::tempdir().expect("data home");
    let tmp = tempfile::tempdir().expect("sync root");
    let fs_impl = LinuxFs::new().with_data_home(data_home.path().to_path_buf());
    let long = "n".repeat(250);
    fs::write(tmp.path().join(&long), b"x").expect("file");
    let root = root_of(&fs_impl, tmp.path());

    // Act
    fs_impl.trash(&root, &long).expect("trash");

    // Assert
    let trash = data_home.path().join("Trash");
    assert!(!tmp.path().join(&long).exists());
    let entries: Vec<_> = fs::read_dir(trash.join("info")).expect("info").collect();
    assert_eq!(entries.len(), 1);
    let info = fs::read_to_string(entries[0].as_ref().expect("entry").path()).expect("read");
    assert!(
        info.contains(&format!("/{long}\n")),
        "the original name is kept in Path="
    );
}

#[test]
fn an_unusable_home_trash_returns_trash_unavailable_and_the_file_stays() {
    for (mode, base) in modes() {
        // Arrange
        let data_home = tempfile::tempdir().expect("data home");
        fs::write(data_home.path().join("Trash"), b"not a directory").expect("squat");
        let tmp = tempfile::tempdir().expect("sync root");
        let fs_impl = fs_with_home(base, data_home.path());
        fs::write(tmp.path().join("keep.txt"), b"still here").expect("file");
        let root = root_of(&fs_impl, tmp.path());

        // Act
        let result = fs_impl.trash(&root, "keep.txt");

        // Assert
        assert!(
            matches!(result, Err(FsError::TrashUnavailable)),
            "{mode}: {result:?}"
        );
        assert_eq!(
            fs::read(tmp.path().join("keep.txt")).expect("kept"),
            b"still here",
            "{mode}"
        );
    }
}

#[test]
fn trashing_a_missing_file_is_not_found_and_leaves_no_info_file() {
    // Arrange
    let data_home = tempfile::tempdir().expect("data home");
    let tmp = tempfile::tempdir().expect("sync root");
    let fs_impl = LinuxFs::new().with_data_home(data_home.path().to_path_buf());
    let root = root_of(&fs_impl, tmp.path());

    // Act
    let result = fs_impl.trash(&root, "ghost.txt");

    // Assert
    assert!(result.expect_err("missing").is_not_found());
    assert!(!data_home.path().join("Trash").exists());
}

/// Removes what the volume-trash test created, also when it fails half way.
struct VolumeTrashGuard(PathBuf);

impl Drop for VolumeTrashGuard {
    fn drop(&mut self) {
        let removed = match fs::symlink_metadata(&self.0) {
            Ok(m) if m.is_dir() => fs::remove_dir_all(&self.0),
            Ok(_) => fs::remove_file(&self.0),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e),
        };
        if let Err(e) = removed {
            eprintln!("cannot clean up {}: {e}", self.0.display());
        }
    }
}

/// A directory on a mount other than the one `home_probe` is on: `/dev/shm` against the temp dir, the
/// manifest dir or `/var/tmp`.
fn two_volumes() -> (PathBuf, PathBuf) {
    let shm = PathBuf::from("/dev/shm");
    let dev = |p: &Path| {
        fs::metadata(p)
            .unwrap_or_else(|e| panic!("{}: {e}", p.display()))
            .dev()
    };
    let other = [
        std::env::temp_dir(),
        PathBuf::from(env!("CARGO_MANIFEST_DIR")),
        PathBuf::from("/var/tmp"),
    ]
    .into_iter()
    .find(|p| p.is_dir() && dev(p) != dev(&shm))
    .expect("no second mount available: /dev/shm is on the same device as every other candidate");
    (shm, other)
}

/// One test, because both cases use the same `<mount>/.Trash-<uid>` and must not overlap.
#[test]
fn a_file_on_another_mount_uses_that_volumes_trash_and_a_squatted_trash_leaves_the_file() {
    for (mode, base) in modes() {
        // Arrange
        let (volume, home_volume) = two_volumes();
        let topdir_trash = volume.join(format!(".Trash-{}", uid()));
        assert!(
            !topdir_trash.exists(),
            "{} exists before the test: remove it",
            topdir_trash.display()
        );
        let _guard = VolumeTrashGuard(topdir_trash.clone());
        let data_home = tempfile::tempdir_in(&home_volume).expect("data home");
        let tmp = tempfile::tempdir_in(&volume).expect("sync root on the other mount");
        let fs_impl = fs_with_home(base, data_home.path());
        fs::write(tmp.path().join("stay.txt"), b"still here").expect("file");
        fs::write(tmp.path().join("other.txt"), b"on tmpfs").expect("file");
        let root = root_of(&fs_impl, tmp.path());
        let before = fs_impl.file_id(&root, "other.txt").expect("id");
        fs::write(&topdir_trash, b"someone else's file").expect("squat the trash name");

        // Act
        let squatted = fs_impl.trash(&root, "stay.txt");
        fs::remove_file(&topdir_trash).expect("lift the squat");
        fs_impl.trash(&root, "other.txt").expect("trash");

        // Assert
        assert!(
            matches!(squatted, Err(FsError::TrashUnavailable)),
            "{mode}: {squatted:?}"
        );
        assert_eq!(
            fs::read(tmp.path().join("stay.txt")).expect("kept"),
            b"still here",
            "{mode}"
        );
        assert!(!tmp.path().join("other.txt").exists(), "{mode}");
        assert!(
            !data_home.path().join("Trash").exists(),
            "{mode}: used the home trash for another mount"
        );
        let moved = topdir_trash.join("files/other.txt");
        let meta = fs::metadata(&moved).expect("moved file");
        assert_eq!(
            (meta.dev(), meta.ino()),
            (before.volume, before.index),
            "{mode}: moved, not copied"
        );
        assert_eq!(fs::read(&moved).expect("data"), b"on tmpfs", "{mode}");
        let info = info_of(&topdir_trash, "other.txt");
        let relative = tmp
            .path()
            .strip_prefix(&volume)
            .expect("under the mount")
            .join("other.txt");
        assert!(
            info.contains(&format!("Path={}\n", relative.display())),
            "{mode}: Path= is relative to the mount point: {info}"
        );
        assert_eq!(
            fs::metadata(&topdir_trash).expect("trash dir").mode() & 0o777,
            0o700,
            "{mode}"
        );
        assert_deletion_date(&info);
    }
}
