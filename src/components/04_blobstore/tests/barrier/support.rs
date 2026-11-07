use std::fs::{self, File, OpenOptions};
use std::os::unix::fs::FileExt;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use contracts::DurableToken;
use sunnycloud_blobstore::{SyncBarrier, SyncHook};
use uuid::Uuid;

pub struct Scratch(pub PathBuf);

impl Scratch {
    pub fn new() -> Self {
        let dir = std::env::temp_dir().join(format!("sc-barrier-{}", Uuid::new_v4()));
        fs::create_dir_all(&dir).expect("scratch directory is created");
        Scratch(dir)
    }

    pub fn file(&self) -> (Uuid, Arc<File>) {
        let id = Uuid::new_v4();
        let file = OpenOptions::new()
            .create_new(true)
            .read(true)
            .write(true)
            .open(self.0.join(id.to_string()))
            .expect("blob file is created");
        (id, Arc::new(file))
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_dir_all(&self.0) {
            eprintln!(
                "scratch directory {} not removed: {error}",
                self.0.display()
            );
        }
    }
}

pub struct Rig {
    pub barrier: SyncBarrier,
    pub hook: Arc<SyncHook>,
    pub scratch: Scratch,
}

impl Rig {
    pub fn new(coalesce_delay: Duration) -> Self {
        let hook = SyncHook::new();
        Rig {
            barrier: SyncBarrier::with_hook(coalesce_delay, Arc::clone(&hook)),
            hook,
            scratch: Scratch::new(),
        }
    }
}

pub struct Written {
    pub token: DurableToken,
    pub stamp: u64,
}

pub fn write_at(file: &File, offset: u64) {
    file.write_all_at(&[0xAB; 4096], offset)
        .expect("block is written");
}

impl Rig {
    pub fn write_and_register(&self, id: Uuid, file: &Arc<File>, offset: u64) -> Written {
        write_at(file, offset);
        let stamp = self.hook.tick();
        let token = self.barrier.register(id, Arc::clone(file));
        Written { token, stamp }
    }
}

pub async fn wait_until(mut done: impl FnMut() -> bool) {
    for _ in 0..500 {
        if done() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("condition not reached in 5 s");
}
