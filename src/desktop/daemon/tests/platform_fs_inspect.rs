#![cfg(target_os = "linux")]

mod support;

use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::symlink;
use std::os::unix::net::UnixListener;
use std::time::{Duration, SystemTime};

use rustix::fs::{CWD, FileType, Mode, mknodat};
use sunnycloud_daemon::platform_fs::{EntryKind, FsError, PlatformFs};
use support::{modes, root_of};
use sync_core::FileSource;

#[test]
fn walk_reports_every_kind_without_following_and_hard_links_as_two_files() {
    for (mode, fs_impl) in modes() {
        // Arrange
        let tmp = tempfile::tempdir().expect("temp dir");
        let p = tmp.path();
        fs::write(p.join("f1"), b"data").expect("file");
        fs::hard_link(p.join("f1"), p.join("f1_hard")).expect("hard link");
        fs::create_dir(p.join("d")).expect("dir");
        symlink("f1", p.join("s_file")).expect("file link");
        symlink("/definitely/not/there", p.join("s_dangling")).expect("dangling link");
        mknodat(
            CWD,
            p.join("fifo"),
            FileType::Fifo,
            Mode::from_raw_mode(0o600),
            0,
        )
        .expect("fifo");
        let _socket = UnixListener::bind(p.join("sock")).expect("socket");
        fs::write(p.join(OsStr::from_bytes(b"bad\xffname")), b"x").expect("non-utf8 name");
        let root = root_of(&fs_impl, p);

        // Act
        let listing = fs_impl.walk(&root).expect("walk");
        let by_name: BTreeMap<_, _> = listing
            .entries
            .iter()
            .map(|e| (e.name.as_str(), e.stat))
            .collect();

        // Assert
        let kind = |n: &str| by_name[n].kind;
        assert_eq!(kind("f1"), EntryKind::File, "{mode}");
        assert_eq!(kind("f1_hard"), EntryKind::File, "{mode}");
        assert_eq!(kind("d"), EntryKind::Dir, "{mode}");
        assert_eq!(kind("s_file"), EntryKind::Symlink, "{mode}");
        assert_eq!(kind("s_dangling"), EntryKind::Symlink, "{mode}");
        assert_eq!(kind("fifo"), EntryKind::Fifo, "{mode}");
        assert_eq!(kind("sock"), EntryKind::Socket, "{mode}");
        assert_eq!(
            by_name["f1"].id, by_name["f1_hard"].id,
            "{mode}: same inode"
        );
        assert_eq!(
            listing.entries.len(),
            7,
            "{mode}: two hard links are two entries"
        );
        assert_eq!(listing.unrepresentable, 1, "{mode}");
        assert_eq!(
            by_name["s_file"].size, 2,
            "{mode}: symlink size is its target, not followed"
        );
    }
}

#[test]
fn open_for_read_refuses_a_fifo_and_a_directory_without_blocking() {
    for (mode, fs_impl) in modes() {
        // Arrange
        let tmp = tempfile::tempdir().expect("temp dir");
        mknodat(
            CWD,
            tmp.path().join("fifo"),
            FileType::Fifo,
            Mode::from_raw_mode(0o600),
            0,
        )
        .expect("fifo");
        fs::create_dir(tmp.path().join("d")).expect("dir");
        let root = root_of(&fs_impl, tmp.path());

        // Act
        let fifo = fs_impl.open_for_read(&root, "fifo");
        let dir = fs_impl.open_for_read(&root, "d");

        // Assert
        assert!(matches!(fifo, Err(FsError::Io(_))), "{mode}: {fifo:?}");
        assert!(matches!(dir, Err(FsError::Io(_))), "{mode}: {dir:?}");
    }
}

#[test]
fn file_and_directory_ids_are_stable_across_renames() {
    for (mode, fs_impl) in modes() {
        // Arrange
        let tmp = tempfile::tempdir().expect("temp dir");
        fs::write(tmp.path().join("a.txt"), b"x").expect("file");
        fs::create_dir(tmp.path().join("dir")).expect("dir");
        let root = root_of(&fs_impl, tmp.path());
        let file_before = fs_impl.file_id(&root, "a.txt").expect("file id");
        let dir_before = fs_impl.file_id(&root, "dir").expect("dir id");

        // Act
        fs::rename(tmp.path().join("a.txt"), tmp.path().join("b.txt")).expect("rename file");
        fs::rename(tmp.path().join("dir"), tmp.path().join("dir2")).expect("rename dir");

        // Assert
        assert_eq!(
            fs_impl.file_id(&root, "b.txt").expect("file id"),
            file_before,
            "{mode}"
        );
        assert_eq!(
            fs_impl.file_id(&root, "dir2").expect("dir id"),
            dir_before,
            "{mode}"
        );
        assert!(
            fs_impl
                .stat_no_follow(&root, "a.txt")
                .expect_err("old name is gone")
                .is_not_found()
        );
    }
}

#[test]
fn case_probe_answers_false_on_a_case_sensitive_directory_and_leaves_nothing() {
    for (mode, fs_impl) in modes() {
        // Arrange
        let tmp = tempfile::tempdir().expect("temp dir");
        let root = root_of(&fs_impl, tmp.path());

        // Act
        let insensitive = fs_impl.is_case_insensitive(&root).expect("probe");

        // Assert
        assert!(!insensitive, "{mode}");
        assert_eq!(
            fs::read_dir(tmp.path()).expect("dir").count(),
            0,
            "{mode}: probe file left behind"
        );
    }
}

/// Run by hand against a casefold directory (`SC_CASEFOLD_DIR`), see the task report: a fixture
/// that needs a mount cannot be created by an unprivileged `cargo test`.
#[test]
#[ignore = "needs SC_CASEFOLD_DIR: an ext4/tmpfs directory with the casefold attribute"]
fn case_probe_answers_true_on_a_casefold_directory_and_leaves_nothing() {
    let dir = std::env::var_os("SC_CASEFOLD_DIR")
        .expect("SC_CASEFOLD_DIR must name a casefold directory");
    for (mode, fs_impl) in modes() {
        // Arrange
        let root = root_of(&fs_impl, dir.as_ref());

        // Act
        let insensitive = fs_impl.is_case_insensitive(&root).expect("probe");

        // Assert
        assert!(insensitive, "{mode}");
        assert_eq!(
            fs::read_dir(&dir).expect("dir").count(),
            0,
            "{mode}: probe file left behind"
        );
    }
}

#[test]
fn identity_reports_the_new_size_and_mtime_after_the_file_changes_under_the_reader() {
    for (mode, fs_impl) in modes() {
        // Arrange
        let tmp = tempfile::tempdir().expect("temp dir");
        let path = tmp.path().join("up.bin");
        fs::write(&path, b"0123456789").expect("seed");
        let root = root_of(&fs_impl, tmp.path());
        let source = fs_impl.open_for_read(&root, "up.bin").expect("open");
        let before = source.identity().expect("identity");

        // Act
        let mut file = OpenOptions::new().append(true).open(&path).expect("reopen");
        file.write_all(b"abc").expect("append");
        file.set_modified(SystemTime::now() + Duration::from_secs(5))
            .expect("touch");
        let after_size = source.identity().expect("identity");
        file.set_modified(SystemTime::now() + Duration::from_secs(60))
            .expect("touch again");
        let after_mtime_only = source.identity().expect("identity");

        // Assert
        assert_eq!((before.size, after_size.size), (10, 13), "{mode}");
        assert_ne!(before.mtime_ns, after_size.mtime_ns, "{mode}");
        assert_eq!(after_mtime_only.size, 13, "{mode}");
        assert_ne!(after_size.mtime_ns, after_mtime_only.mtime_ns, "{mode}");
        assert_eq!(source.len().expect("len"), 13, "{mode}");
    }
}

#[test]
fn read_at_returns_whole_blocks_and_fewer_bytes_only_at_the_end() {
    for (mode, fs_impl) in modes() {
        // Arrange
        let tmp = tempfile::tempdir().expect("temp dir");
        fs::write(tmp.path().join("r.bin"), b"0123456789").expect("seed");
        let root = root_of(&fs_impl, tmp.path());
        let source = fs_impl.open_for_read(&root, "r.bin").expect("open");

        // Act
        let middle = source.read_at(2, 4).expect("middle");
        let tail = source.read_at(8, 100).expect("tail");
        let past_end = source.read_at(50, 4).expect("past end");

        // Assert
        assert_eq!(&middle[..], b"2345", "{mode}");
        assert_eq!(&tail[..], b"89", "{mode}");
        assert!(past_end.is_empty(), "{mode}");
    }
}
