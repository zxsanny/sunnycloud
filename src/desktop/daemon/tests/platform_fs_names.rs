#![cfg(target_os = "linux")]

mod support;

use std::fs;
use std::os::unix::fs::symlink;

use sunnycloud_daemon::platform_fs::linux::LinuxFs;
use sunnycloud_daemon::platform_fs::{FsError, PlatformFs, TEMP_PREFIX, TempFile};
use support::{modes, root_of};

struct Hostile {
    _tmp: tempfile::TempDir,
    root: std::path::PathBuf,
    outside: std::path::PathBuf,
}

/// `root/real/`, `root/link -> real`, `root/file_link -> outside/secret.txt` and a sibling `outside/`.
fn hostile_tree() -> Hostile {
    let tmp = tempfile::tempdir().expect("temp dir");
    let root = tmp.path().join("root");
    let outside = tmp.path().join("outside");
    fs::create_dir_all(root.join("real")).expect("real dir");
    fs::create_dir_all(&outside).expect("outside dir");
    fs::write(outside.join("secret.txt"), b"secret").expect("secret");
    symlink("real", root.join("link")).expect("dir link");
    symlink(outside.join("secret.txt"), root.join("file_link")).expect("file link");
    Hostile {
        _tmp: tmp,
        root,
        outside,
    }
}

fn names_are_refused(err: &FsError) -> bool {
    matches!(err, FsError::EscapesRoot | FsError::NameNotRepresentable)
}

#[test]
fn create_new_in_refuses_traversal_symlink_and_absolute_names_and_creates_nothing() {
    for (mode, fs) in modes() {
        // Arrange
        let h = hostile_tree();
        let root = root_of(&fs, &h.root);
        let absolute = format!("{}/x", h.outside.display());
        let cases = [
            "../x",
            "real/../../x",
            "link/x",
            "file_link/x",
            absolute.as_str(),
            "",
            "a//b",
            "bad\0name",
        ];

        for name in cases {
            // Act
            let result = fs.create_new_in(&root, name);

            // Assert
            let err = result
                .err()
                .unwrap_or_else(|| panic!("{mode}: {name:?} was accepted"));
            assert!(names_are_refused(&err), "{mode}: {name:?} gave {err:?}");
        }
        assert_eq!(
            fs::read_dir(&h.outside).expect("outside").count(),
            1,
            "{mode}: outside touched"
        );
        assert_eq!(
            fs::read_dir(h.root.join("real")).expect("real").count(),
            0,
            "{mode}: link target touched"
        );
    }
}

#[test]
fn symlink_components_are_escapes_root_for_every_operation() {
    for (mode, fs) in modes() {
        // Arrange
        let h = hostile_tree();
        let root = root_of(&fs, &h.root);

        // Act
        let read_through_dir_link = fs.open_for_read(&root, "link/x");
        let read_file_link = fs.open_for_read(&root, "file_link");
        let open_dir_link = fs.open_dir(&root, "link");
        let create_through_dir_link = fs.create_new_in(&root, "link/x");

        // Assert
        for (what, result) in [
            ("read through a dir link", read_through_dir_link.err()),
            ("read a file link", read_file_link.err()),
            ("open a dir link", open_dir_link.err()),
            ("create through a dir link", create_through_dir_link.err()),
        ] {
            assert!(
                matches!(result, Some(FsError::EscapesRoot)),
                "{mode}: {what} gave {result:?}"
            );
        }
    }
}

#[test]
fn open_for_read_refuses_traversal_and_absolute_names() {
    for (mode, fs) in modes() {
        // Arrange
        let h = hostile_tree();
        let root = root_of(&fs, &h.root);
        let absolute = h.outside.join("secret.txt").display().to_string();

        for name in ["../outside/secret.txt", absolute.as_str()] {
            // Act
            let result = fs.open_for_read(&root, name);

            // Assert
            let err = result
                .err()
                .unwrap_or_else(|| panic!("{mode}: {name:?} was opened"));
            assert!(names_are_refused(&err), "{mode}: {name:?} gave {err:?}");
        }
    }
}

#[test]
fn create_new_in_makes_a_prefixed_new_name_and_leaves_existing_files_alone() {
    for (mode, fs) in modes() {
        // Arrange
        let tmp = tempfile::tempdir().expect("temp dir");
        fs::write(tmp.path().join("a.txt"), b"keep me").expect("seed");
        let root = root_of(&fs, tmp.path());

        // Act
        let temps: Vec<_> = (0..3)
            .map(|_| fs.create_new_in(&root, "a.txt").expect("temp"))
            .collect();

        // Assert
        let mut names: Vec<_> = temps.iter().map(|t| t.name().to_owned()).collect();
        assert!(
            names
                .iter()
                .all(|n| n.starts_with(TEMP_PREFIX) && n != "a.txt"),
            "{mode}: {names:?}"
        );
        names.sort();
        names.dedup();
        assert_eq!(names.len(), 3, "{mode}: temp names collide");
        assert_eq!(
            fs::read(tmp.path().join("a.txt")).expect("read"),
            b"keep me",
            "{mode}"
        );
        for name in &names {
            assert!(
                tmp.path().join(name).is_file(),
                "{mode}: {name} not created"
            );
        }
    }
}

#[test]
fn rename_over_in_replaces_the_target_and_discard_temp_removes_only_the_temp() {
    for (mode, fs) in modes() {
        // Arrange
        let tmp = tempfile::tempdir().expect("temp dir");
        fs::create_dir(tmp.path().join("d")).expect("dir");
        fs::write(tmp.path().join("d/a.txt"), b"old").expect("seed");
        let root = root_of(&fs, tmp.path());
        let temp = fs.create_new_in(&root, "d/a.txt").expect("temp");
        temp.write_all_at(0, b"new").expect("write");
        let unused = fs.create_new_in(&root, "d/b.txt").expect("second temp");

        // Act
        fs.rename_over_in(&root, temp, "d/a.txt").expect("rename");
        fs.discard_temp(&root, unused).expect("discard");

        // Assert
        assert_eq!(
            fs::read(tmp.path().join("d/a.txt")).expect("read"),
            b"new",
            "{mode}"
        );
        let left: Vec<_> = fs::read_dir(tmp.path().join("d")).expect("dir").collect();
        assert_eq!(left.len(), 1, "{mode}: temp files left behind");
    }
}

#[test]
fn rename_over_in_refuses_a_target_in_another_directory() {
    // Arrange
    let fs = LinuxFs::new();
    let tmp = tempfile::tempdir().expect("temp dir");
    fs::create_dir(tmp.path().join("d1")).expect("d1");
    fs::create_dir(tmp.path().join("d2")).expect("d2");
    let root = root_of(&fs, tmp.path());
    let temp = fs.create_new_in(&root, "d1/a.txt").expect("temp");

    // Act
    let result = fs.rename_over_in(&root, temp, "d2/a.txt");

    // Assert
    assert!(matches!(result, Err(FsError::Io(_))), "{result:?}");
    assert!(!tmp.path().join("d2/a.txt").exists());
}

#[test]
fn default_resolver_is_openat2_whenever_the_kernel_allows_it() {
    // Arrange
    let kernel_has_openat2 = rustix::fs::openat2(
        rustix::fs::CWD,
        ".",
        rustix::fs::OFlags::RDONLY | rustix::fs::OFlags::CLOEXEC,
        rustix::fs::Mode::empty(),
        rustix::fs::ResolveFlags::BENEATH | rustix::fs::ResolveFlags::NO_SYMLINKS,
    )
    .is_ok();

    // Act
    let auto = LinuxFs::new();
    let forced = LinuxFs::new().without_openat2();

    // Assert
    assert_eq!(auto.uses_openat2(), kernel_has_openat2);
    assert!(!forced.uses_openat2());
    eprintln!("openat2 available on this kernel: {kernel_has_openat2}");
}
