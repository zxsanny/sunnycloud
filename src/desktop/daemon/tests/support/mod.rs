#![allow(dead_code)]

use std::path::Path;

use sunnycloud_daemon::platform_fs::PlatformFs;
use sunnycloud_daemon::platform_fs::linux::{LinuxDir, LinuxFs};

/// Both resolution strategies: whatever `LinuxFs::new` selects here, and the forced per-component fallback.
pub fn modes() -> [(&'static str, LinuxFs); 2] {
    [
        ("auto", LinuxFs::new()),
        ("component-walk", LinuxFs::new().without_openat2()),
    ]
}

pub fn root_of(fs: &LinuxFs, path: &Path) -> LinuxDir {
    fs.open_root(path).expect("open the sync root")
}
