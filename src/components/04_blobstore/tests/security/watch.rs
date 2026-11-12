//! The path recorder of ST-04-4: inotify watches on chosen directories record every open, create, delete,
//! modify, access and attribute change of an entry in them, as `<directory>/<name>`. Events are queued by the
//! kernel at the system call, so reading them after the calls under test misses none.

use std::collections::HashMap;
use std::mem::MaybeUninit;
use std::path::{Path, PathBuf};

use rustix::fd::OwnedFd;
use rustix::fs::inotify::{self, CreateFlags, WatchFlags};
use rustix::io::Errno;

pub struct PathRecorder {
    fd: OwnedFd,
    dirs: HashMap<i32, PathBuf>,
}

impl PathRecorder {
    pub fn new() -> Self {
        PathRecorder {
            fd: inotify::init(CreateFlags::CLOEXEC | CreateFlags::NONBLOCK).expect("inotify"),
            dirs: HashMap::new(),
        }
    }

    pub fn watch(&mut self, dir: &Path) {
        let flags = WatchFlags::OPEN
            | WatchFlags::ACCESS
            | WatchFlags::CREATE
            | WatchFlags::DELETE
            | WatchFlags::MODIFY
            | WatchFlags::ATTRIB
            | WatchFlags::MOVE;
        let wd = inotify::add_watch(&self.fd, dir, flags).expect("watch directory");
        self.dirs.insert(wd, dir.to_path_buf());
    }

    /// Every path touched since the watches were set (or since the last call).
    pub fn paths(&self) -> Vec<PathBuf> {
        let mut buffer = [MaybeUninit::<u8>::uninit(); 8192];
        let mut reader = inotify::Reader::new(&self.fd, &mut buffer);
        let mut paths = Vec::new();
        loop {
            match reader.next() {
                Ok(event) => {
                    let dir = self
                        .dirs
                        .get(&event.wd())
                        .expect("event of a watched directory");
                    let name = event
                        .file_name()
                        .map(|name| name.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    paths.push(dir.join(name));
                }
                Err(Errno::AGAIN) => return paths,
                Err(error) => panic!("inotify read failed: {error}"),
            }
        }
    }
}
