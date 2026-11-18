//! AC-6: no token crosses the surface: not in a return value, the status, an error value or the log.

use std::sync::Arc;

use mobile_backup::{BackupCore, BackupError, CheckpointDb, ServerProbe, Trigger};
use sc_config::Secret;
use sync_core::{BoxFuture, ConnectError, Credential, SyncConfig, SyncError};

use crate::support::Rig;
use crate::support::log::Capture;

const ACCESS: &str = "sentinel-access-5d1c9e";
const REFRESH: &str = "sentinel-refresh-b84f02";

struct Refusing(ConnectError);

impl ServerProbe for Refusing {
    fn check(&self, _config: SyncConfig) -> BoxFuture<'_, Result<(), ConnectError>> {
        let error = self.0.clone();
        Box::pin(async move { Err(error) })
    }
}

fn leaks(text: &str) -> bool {
    text.contains(ACCESS) || text.contains(REFRESH)
}

#[tokio::test]
async fn sentinel_tokens_of_a_signed_in_session_appear_in_no_output_of_runs_status_errors_or_log() {
    // Arrange
    let (log, _guard) = Capture::install();
    let rig = Rig::new(8, 50);
    rig.api.sign_in(Credential {
        access: Secret::new(ACCESS.to_owned()),
        refresh: Some(Secret::new(REFRESH.to_owned())),
    });
    for id in 1..=4 {
        rig.source.add(id, 1024);
    }
    rig.api.fail_hash(2, SyncError::SourceChanged);
    rig.api.fail_upload(
        "IMG_4.jpg",
        SyncError::UpgradeRequired {
            message: "Please update Sunnycloud".to_owned(),
            min_version: None,
            update_url: None,
        },
    );
    let mut outputs = Vec::new();

    // Act
    outputs.push(format!("{:?}", rig.try_run(Trigger::FirstSync).await));
    outputs.push(format!("{:?}", rig.core.status()));
    rig.api.clear_faults();
    outputs.push(format!("{:?}", rig.try_run(Trigger::AppOpen).await));
    outputs.push(format!("{:?}", rig.core.status()));
    for error in [
        ConnectError::InsecureScheme,
        ConnectError::Unreachable,
        ConnectError::Unauthorized,
        ConnectError::UpgradeRequired {
            message: "Please update".to_owned(),
            update_url: None,
        },
    ] {
        let db = Arc::new(CheckpointDb::new(rig.path.with_file_name("other.db")));
        let core = BackupCore::with_probe(db, "1.0.0".to_owned(), Arc::new(Refusing(error)));
        let refused = core.set_server("https://sunnycloud.test".to_owned()).await;
        let error: BackupError = refused.expect_err("the probe refuses");
        outputs.push(format!("{error:?} {error}"));
    }

    // Assert
    assert!(
        rig.api.record().bearer_uses > 10,
        "the sync layer used the credential on its requests"
    );
    assert_eq!(rig.server.file_count(), 3);
    for output in &outputs {
        assert!(!leaks(output), "a token reached an output: {output}");
    }
    let text = log.text();
    assert!(
        text.contains("backup error recorded"),
        "the log holds the failures: {text}"
    );
    assert!(!leaks(&text), "a token reached the log: {text}");
}
