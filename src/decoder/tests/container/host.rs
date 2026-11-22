//! Host-side helpers of the harness: the seccomp profile, removing trees the runner wrote into, and reading
//! the peak memory of the runner from the host's `/proc`.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::Duration;

pub fn seccomp_profile() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../deploy/seccomp/decoder.json")
        .canonicalize()
        .expect("deploy/seccomp/decoder.json exists")
}

/// Removes a tree the runner may have written into: the host removes what it owns; a directory the
/// runner made itself (`work/` after a killed container) needs the image's own user 0.
pub fn remove_tree(path: &Path, image: &str) {
    if !path.exists() || fs::remove_dir_all(path).is_ok() {
        return;
    }
    let parent = path.parent().expect("a tree has a parent");
    let name = path
        .file_name()
        .expect("a tree has a name")
        .to_string_lossy();
    let helper = Command::new("docker")
        .args([
            "run",
            "--rm",
            "--label",
            "sunnycloud-suite=h3",
            "--network",
            "none",
            "--user",
            "0",
        ])
        .args(["--entrypoint", "rm", "--volume"])
        .arg(format!("{}:/volume", parent.display()))
        .args([image, "-rf", "--", &format!("/volume/{name}")])
        .output()
        .expect("run the cleanup helper");
    assert!(
        helper.status.success() && !path.exists(),
        "cannot remove {}: {}",
        path.display(),
        String::from_utf8_lossy(&helper.stderr)
    );
}

/// The peak resident memory (`VmHWM`, bytes) of a process, read from the host's `/proc`; `None` once the
/// process is gone.
pub fn peak_rss(pid: u32) -> Option<u64> {
    let status = fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
    let kib: u64 = status
        .lines()
        .find_map(|line| line.strip_prefix("VmHWM:"))?
        .split_whitespace()
        .next()?
        .parse()
        .ok()?;
    Some(kib * 1024)
}

/// Samples [`peak_rss`] on a thread until the process ends; the last reading is the peak before the exit.
pub struct RssWatch {
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    thread: thread::JoinHandle<u64>,
}

impl RssWatch {
    pub fn start(pid: u32) -> Self {
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let flag = std::sync::Arc::clone(&stop);
        let thread = thread::spawn(move || {
            let mut peak = 0;
            while !flag.load(std::sync::atomic::Ordering::SeqCst) {
                match peak_rss(pid) {
                    Some(now) => peak = peak.max(now),
                    None => break,
                }
                thread::sleep(Duration::from_millis(1));
            }
            peak
        });
        Self { stop, thread }
    }

    pub fn peak(self) -> u64 {
        self.stop.store(true, std::sync::atomic::Ordering::SeqCst);
        self.thread.join().expect("the RSS watch thread")
    }
}
