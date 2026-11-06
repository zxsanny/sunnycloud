#![cfg(target_os = "linux")]

mod support;

use std::fs;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::thread;

use sunnycloud_daemon::platform_fs::{PlatformFs, TempFile};
use support::{modes, root_of};
use sync_core::FileSource;

const SIZE: usize = 256 * 1024;
const GENERATIONS: u8 = 60;

#[test]
fn target_shows_the_old_or_the_new_content_at_every_moment_of_a_replace() {
    for (mode, fs_impl) in modes() {
        // Arrange
        let tmp = tempfile::tempdir().expect("temp dir");
        fs::write(tmp.path().join("t.txt"), vec![0u8; SIZE]).expect("seed");
        let root = root_of(&fs_impl, tmp.path());
        let done = AtomicBool::new(false);
        let reads = AtomicU32::new(0);

        // Act
        thread::scope(|scope| {
            let reader = scope.spawn(|| {
                while !done.load(Ordering::Acquire) {
                    let source = fs_impl
                        .open_for_read(&root, "t.txt")
                        .expect("target is always there");
                    let len = source.len().expect("len") as usize;
                    let bytes = source.read_at(0, len as u32).expect("read");
                    let first = bytes[0];
                    assert_eq!(len, SIZE, "{mode}: partial file");
                    assert!(bytes.iter().all(|b| *b == first), "{mode}: mixed content");
                    reads.fetch_add(1, Ordering::Release);
                }
            });
            while reads.load(Ordering::Acquire) == 0 {
                thread::yield_now();
            }
            for generation in 1..=GENERATIONS {
                let temp = fs_impl.create_new_in(&root, "t.txt").expect("temp");
                temp.write_all_at(0, &vec![generation; SIZE])
                    .expect("write");
                fs_impl
                    .rename_over_in(&root, temp, "t.txt")
                    .expect("replace");
            }
            done.store(true, Ordering::Release);
            reader.join().expect("reader saw only whole files");
        });

        // Assert
        let finished = fs::read(tmp.path().join("t.txt")).expect("read");
        assert_eq!(finished, vec![GENERATIONS; SIZE], "{mode}");
        assert_eq!(
            fs::read_dir(tmp.path()).expect("dir").count(),
            1,
            "{mode}: temp left"
        );
        assert!(reads.load(Ordering::Acquire) > 0, "{mode}");
    }
}
