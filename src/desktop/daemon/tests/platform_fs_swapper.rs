#![cfg(target_os = "linux")]

//! ST-14-1 (Linux half): a directory swapped for a symlink to the outside, 10,000 times, while the
//! daemon creates and renames files in it. Real file system, real threads; no OS call is faked.

mod support;

use std::fs::{self, File};
use std::os::unix::fs::symlink;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;

use rustix::fs::{RenameFlags, renameat_with};
use sunnycloud_daemon::platform_fs::{FsError, PlatformFs, TempFile};
use support::{modes, root_of};

const SWAPS: u32 = 10_000;

#[derive(Default)]
struct Tally {
    written: u32,
    redirected: u32,
}

#[test]
fn writes_never_leave_the_root_while_the_directory_is_swapped_for_a_symlink() {
    for (mode, fs) in modes() {
        // Arrange
        let tmp = tempfile::tempdir().expect("temp dir");
        let root_path = tmp.path().join("root");
        let outside = tmp.path().join("outside");
        fs::create_dir_all(root_path.join("sub")).expect("sub");
        fs::create_dir(&outside).expect("outside");
        symlink(&outside, root_path.join("link")).expect("link");
        let root = root_of(&fs, &root_path);
        let swap_dir = File::open(&root_path).expect("swap handle");
        let done = AtomicBool::new(false);

        // Act
        let tally = thread::scope(|scope| {
            scope.spawn(|| {
                for _ in 0..SWAPS {
                    renameat_with(&swap_dir, "sub", &swap_dir, "link", RenameFlags::EXCHANGE)
                        .expect("exchange the directory and the symlink");
                }
                done.store(true, Ordering::Release);
            });
            let mut tally = Tally::default();
            let mut i = 0u64;
            while !done.load(Ordering::Acquire) {
                let name = format!("sub/f{i}.txt");
                i += 1;
                match fs.create_new_in(&root, &name) {
                    Ok(temp) => {
                        temp.write_all_at(0, b"payload").expect("write temp");
                        match fs.rename_over_in(&root, temp, &name) {
                            Ok(()) => tally.written += 1,
                            Err(FsError::EscapesRoot) => tally.redirected += 1,
                            Err(other) => panic!("{mode}: rename failed with {other:?}"),
                        }
                    }
                    Err(FsError::EscapesRoot) => tally.redirected += 1,
                    Err(other) => panic!("{mode}: create failed with {other:?}"),
                }
            }
            tally
        });

        // Assert
        let leaked: Vec<_> = fs::read_dir(&outside).expect("outside").collect();
        assert!(
            leaked.is_empty(),
            "{mode}: {} entries appeared outside the root",
            leaked.len()
        );
        assert!(
            tally.redirected > 0,
            "{mode}: no write was ever redirected, the race did not happen"
        );
        assert!(tally.written > 0, "{mode}: no write ever succeeded");
        eprintln!(
            "{mode}: {} written, {} refused as EscapesRoot",
            tally.written, tally.redirected
        );
    }
}
