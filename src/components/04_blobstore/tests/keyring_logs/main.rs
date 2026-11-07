//! The log lines of the keyring: INFO "startup key check passed" and ERROR on a failed unwrap, with ids and no key
//! bytes. This binary holds this one test: the capturing subscriber is installed on the test's thread, and a scoped
//! default races with the callsite interest cache when other tests run in parallel.

#[allow(dead_code)]
#[path = "../keyring/support.rs"]
mod support;

use std::io;
use std::sync::{Arc, Mutex, PoisonError};

use contracts::BlobError;
use sc_observability::{Component, LineSink, LogConfig, LogFormat, subscriber};
use sunnycloud_blobstore::DataKey;
use support::{Env, KEY_ONE};
use tracing::Level;
use uuid::Uuid;

#[derive(Clone, Default)]
struct Lines(Arc<Mutex<Vec<String>>>);

impl Lines {
    fn all(&self) -> Vec<String> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
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

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[tokio::test]
async fn the_start_logs_info_and_a_failed_unwrap_logs_error_without_key_bytes() {
    // Arrange
    let env = Env::new().await;
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
    let keyring = env.keyring_with_key(&KEY_ONE);
    let (blob, other_blob) = (Uuid::new_v4(), Uuid::new_v4());
    let key = DataKey::generate().expect("data key");

    // Act
    keyring.load_and_verify().await.expect("start");
    let after_start = lines.all();
    let (wrapped, key_id) = keyring.wrap_data_key(blob, &key).expect("wrap");
    let after_wrap = lines.all().len();
    let refused = keyring.unwrap_data_key(other_blob, &wrapped, key_id);

    // Assert
    assert!(
        after_start
            .iter()
            .any(|l| l.contains("startup key check passed") && l.contains("\"level\":\"info\"")),
        "{after_start:?}"
    );
    assert_eq!(after_wrap, after_start.len());
    assert!(matches!(refused, Err(BlobError::KeyUnavailable)));
    let logged = lines.all();
    let errors: Vec<_> = logged
        .iter()
        .filter(|l| l.contains("\"level\":\"error\""))
        .collect();
    assert_eq!(errors.len(), 1, "{logged:?}");
    assert!(errors[0].contains(&other_blob.to_string()), "{}", errors[0]);
    for line in &logged {
        for secret in [hex(&KEY_ONE), hex(key.as_bytes()), hex(&wrapped)] {
            assert!(!line.contains(&secret), "{line}");
        }
    }
    drop(keyring);
    env.finish().await;
}
