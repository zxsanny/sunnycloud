//! ST-04-7 (AC-9): no key bytes anywhere observable. Every error path (wrong key, wrong candidate, short key,
//! missing key file, tampered chunk, missing file, foreign wrapped key, a scrub that finds the tamper) runs with
//! the capturing log subscriber installed process-wide and real metric registries; then the log lines, the
//! `/metrics` text and the `Display` and `Debug` text of every error and its causes are searched for the master
//! keys in hex, base64 and byte-list form. Zeroize on drop of the key types is asserted at compile time in the
//! crate (`data_key.rs`, `keyring/secret.rs`).

use std::error::Error;
use std::fs;
use std::io;
use std::sync::{Arc, Mutex, PoisonError};

use contracts::{BlobError, BlobRef, KeyError, MasterKey, ScrubMode, Scrubber};
use sc_config::Secret;
use sc_observability::{BuildInfo, Component, LineSink, LogConfig, LogFormat, Metrics, subscriber};
use sunnycloud_blobstore::{BlobMetrics, BlobstoreConfig, DataKey, FileBlobReader, MasterKeyring};
use tracing::Level;

use crate::kit::{key_forms, read_all, store};
use crate::support::{KEY, MIB, Rig, run_to_done};

const OTHER: [u8; 32] = [0x6B; 32];

#[derive(Clone, Default)]
struct Lines(Arc<Mutex<Vec<String>>>);

impl LineSink for Lines {
    fn write_line(&self, line: &str) -> io::Result<()> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(line.to_owned());
        Ok(())
    }
}

fn texts(error: &(dyn Error + 'static)) -> Vec<String> {
    let mut all = Vec::new();
    let mut next = Some(error);
    while let Some(current) = next {
        all.push(current.to_string());
        all.push(format!("{current:?}"));
        next = current.source();
    }
    all
}

fn keyring_with(rig: &Rig, name: &str, key: &[u8]) -> MasterKeyring {
    let path = rig._scratch.0.join(name);
    if !key.is_empty() {
        fs::write(&path, key).expect("key file");
    }
    let config = BlobstoreConfig {
        master_key_file: Secret::new(path.to_string_lossy().into_owned()),
        ..rig.config.clone()
    };
    MasterKeyring::new(config, rig.api.clone(), rig.audit.clone())
}

fn registry() -> Metrics {
    Metrics::new(BuildInfo {
        version: "test",
        commit: "test",
        test_clock: false,
    })
    .expect("metrics")
}

/// Every key error a start or a key-check init can give for a bad key.
async fn key_errors(rig: &Rig) -> Vec<KeyError> {
    let mut errors = Vec::new();
    for (name, key) in [
        ("other.key", &OTHER[..]),
        ("short.key", &OTHER[..31]),
        ("absent.key", &[][..]),
    ] {
        let keyring = keyring_with(rig, name, key);
        errors.push(keyring.load_and_verify().await.expect_err("refused"));
    }
    errors.push(
        rig.keyring
            .init_key_check(&MasterKey(OTHER.to_vec()))
            .await
            .expect_err("wrong candidate"),
    );
    errors
}

#[tokio::test(flavor = "multi_thread")]
async fn logs_metrics_and_error_texts_of_every_error_path_hold_no_key_bytes() {
    // Arrange
    let lines = Lines::default();
    let (capture, _) = subscriber(
        LogConfig {
            format: LogFormat::Json,
            level: Level::DEBUG,
            component: Component::Api,
        },
        sc_testdb::clock_at_start(),
        Arc::new(lines.clone()),
    );
    tracing::subscriber::set_global_default(capture).expect("first subscriber of this process");
    let rig = Rig::new().await;
    let read_metrics = registry();
    let blob_metrics =
        BlobMetrics::register(&read_metrics, rig.clock.clone()).expect("blob series");
    let reader = FileBlobReader::new(
        &rig.config,
        Arc::clone(&rig.keyring),
        rig.api.clone(),
        rig.audit.clone(),
        rig.clock.clone(),
    )
    .expect("reader")
    .with_metrics(blob_metrics);
    let tampered = store(&rig, rig.account, &vec![1; 3 * MIB]).await;
    let late_tamper = store(&rig, rig.account, &vec![2; 3 * MIB]).await;
    let missing = store(&rig, rig.account, &vec![3; MIB]).await;
    let other = store(&rig, rig.account, &vec![4; MIB]).await;
    rig.flip_bit(tampered.blob_id);
    rig.delete_file(missing.blob_id);
    let foreign = BlobRef {
        wrapped_key: other.wrapped_key.clone(),
        ..store(&rig, rig.account, &vec![5; MIB]).await
    };
    let scrub = rig.scrub(10);

    // Act
    let key_errors = key_errors(&rig).await;
    let mut blob_errors: Vec<BlobError> = Vec::new();
    let mut stream_texts = Vec::new();
    for blob in [&tampered, &missing, &foreign] {
        let read = read_all(&reader, blob.blob_id, blob).await;
        blob_errors.extend(read.open);
    }
    let file = fs::OpenOptions::new()
        .write(true)
        .open(rig.path(late_tamper.blob_id))
        .expect("blob file");
    std::os::unix::fs::FileExt::write_all_at(&file, &[0; 32], 2 * MIB as u64).expect("damage");
    let late = read_all(&reader, late_tamper.blob_id, &late_tamper).await;
    if let Some(error) = &late.stream {
        stream_texts.extend(texts(error));
    }
    let run = scrub
        .scrubber
        .start_run(ScrubMode::Full, None)
        .await
        .expect("scrub");
    let (scrubbed, _) = run_to_done(scrub.scrubber.as_ref(), &run, 10).await;
    scrub.scrubber.finish_run(&run).await.expect("finish");
    let scrape = [
        read_metrics.render().await.expect("render"),
        scrub.metrics.render().await.expect("render"),
    ]
    .concat();
    let logged = lines
        .0
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone();
    let data_key = DataKey::generate().expect("data key");
    let key_debug = [
        format!("{data_key:?} {data_key:#?}"),
        format!("{:?}", MasterKey(KEY.to_vec())),
    ];

    // Assert
    assert_eq!(key_errors.len(), 4);
    assert!(matches!(key_errors[0], KeyError::KeyCheckMismatch));
    assert!(matches!(key_errors[1], KeyError::WrongLength));
    assert!(matches!(key_errors[2], KeyError::FileMissing));
    assert!(matches!(key_errors[3], KeyError::KeyCheckMismatch));
    assert!(matches!(
        blob_errors[..],
        [
            BlobError::Corrupt,
            BlobError::BlobMissing,
            BlobError::KeyUnavailable
        ]
    ));
    assert!(!stream_texts.is_empty(), "the late tamper ends the stream");
    assert!(
        scrubbed.mismatched >= 1 && scrubbed.missing >= 1,
        "{scrubbed:?}"
    );
    assert!(!logged.is_empty() && scrape.contains("sunnycloud_blob_read_duration_seconds"));
    let mut observed: Vec<String> = key_errors.iter().flat_map(|error| texts(error)).collect();
    observed.extend(blob_errors.iter().flat_map(|error| texts(error)));
    observed.extend(stream_texts);
    observed.extend(logged);
    observed.push(scrape);
    let forbidden: Vec<String> = [
        key_forms(&KEY),
        key_forms(&OTHER),
        key_forms(data_key.as_bytes()),
    ]
    .concat();
    for text in &observed {
        for form in &forbidden {
            assert!(!text.contains(form.as_str()), "key bytes in `{text}`");
        }
    }
    for text in &key_debug {
        assert!(text.contains("redacted"), "{text}");
        assert!(
            forbidden.iter().all(|form| !text.contains(form.as_str())),
            "{text}"
        );
    }
    drop((scrub, reader));
    rig.finish().await;
}
