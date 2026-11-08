//! The INFO lines of a rewrap batch (AC-1). This binary holds this one test: the capturing subscriber is installed
//! on the test's thread, and a scoped default races with the callsite interest cache when other tests run in
//! parallel.

#[allow(dead_code)]
#[path = "../scrub/support.rs"]
mod support;

use std::io;
use std::sync::{Arc, Mutex, PoisonError};

use sc_observability::{Component, LineSink, LogConfig, LogFormat, subscriber};
use support::Rig;
use tracing::Level;

#[derive(Clone, Default)]
struct Lines(Arc<Mutex<Vec<String>>>);

impl Lines {
    fn with(&self, level: &str, message: &str) -> usize {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .filter(|line| {
                line.contains(&format!("\"level\":\"{level}\"")) && line.contains(message)
            })
            .count()
    }
}

impl LineSink for Lines {
    fn write_line(&self, line: &str) -> io::Result<()> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(line.to_owned());
        Ok(())
    }
}

#[tokio::test]
async fn each_rewrap_batch_and_the_final_empty_one_log_at_info() {
    // Arrange
    let rig = Rig::new().await;
    for seed in 0..5 {
        rig.store(1024, seed).await;
    }
    let second = rig._scratch.0.join("master2.key");
    std::fs::write(&second, [0x44u8; 32]).expect("second key file");
    let mut config = rig.config.clone();
    config.master_key_ring = Some(format!(
        "1={},2={}",
        rig.config.master_key_file.expose(),
        second.display()
    ));
    let keyring =
        sunnycloud_blobstore::MasterKeyring::new(config, rig.api.clone(), rig.audit.clone());
    let lines = Lines::default();
    let (capture, _) = subscriber(
        LogConfig {
            format: LogFormat::Json,
            level: Level::INFO,
            component: Component::Api,
        },
        sc_testdb::clock_at_start(),
        Arc::new(lines.clone()),
    );
    let _guard = tracing::subscriber::set_default(capture);

    // Act
    let mut handled = Vec::new();
    loop {
        let rows = keyring.rewrap_batch(1, 2, 2).await.expect("batch");
        handled.push(rows);
        if rows == 0 {
            break;
        }
    }

    // Assert
    assert_eq!(handled, vec![2, 2, 1, 0]);
    assert_eq!(lines.with("info", "rewrap batch finished"), 3);
    assert_eq!(lines.with("info", "rewrap finished"), 1);
    assert_eq!(lines.with("error", ""), 0);
    drop(keyring);
    rig.finish().await;
}
