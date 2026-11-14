//! AZ-3759 AC-1: after a mixed run (an upload committed by block list, a refused start, an aborted session, a batch
//! commit and a download) the scrape carries every transfer series of `observability.md` § Transfer with its
//! documented type and labels, the counters moved by the run, and no label value names an account, node, file or
//! address.

use std::sync::Arc;

use contracts::UploadError;
use sc_observability::{BuildInfo, Metrics};
use uuid::Uuid;

use crate::batch_support::{batch, entry};
use crate::download_support::read_all;
use crate::put_support::content;
use crate::start_support::{GIB, MIB, Rig, request, started};

/// Every exported name carries this prefix (`observability.md`); the table below names the families without it.
const PREFIX: &str = "sunnycloud_";

/// A label and its documented values.
type Label = (&'static str, &'static [&'static str]);

/// The transfer families 03 emits, their types and each label with its documented values (`le` is the
/// histogram bucket bound).
const FAMILIES: [(&str, &str, &[Label]); 8] = [
    ("upload_sessions_active", "gauge", &[]),
    (
        "upload_sessions_total",
        "counter",
        &[("result", &["committed", "expired", "aborted"])],
    ),
    (
        "upload_rejections_total",
        "counter",
        &[("reason", &["quota", "session_cap", "free_floor"])],
    ),
    ("blocks_received_total", "counter", &[]),
    ("block_bytes_received_total", "counter", &[]),
    (
        "commits_total",
        "counter",
        &[
            ("kind", &["block_list", "batch"]),
            ("result", &["committed", "refused", "failed"]),
        ],
    ),
    (
        "commit_duration_seconds",
        "histogram",
        &[("kind", &["block_list", "batch"])],
    ),
    ("download_bytes_total", "counter", &[]),
];

/// The family a sample line belongs to: its name without the prefix and the histogram suffixes.
fn family_of(sample: &str) -> &str {
    let name = sample.split(['{', ' ']).next().unwrap_or(sample);
    let name = name.strip_prefix(PREFIX).unwrap_or(name);
    ["_bucket", "_sum", "_count"]
        .iter()
        .find_map(|suffix| {
            name.strip_suffix(suffix)
                .filter(|base| *base == "commit_duration_seconds")
        })
        .unwrap_or(name)
}

/// The `(name, value)` label pairs of a sample line.
fn labels(sample: &str) -> Vec<(String, String)> {
    let Some((_, rest)) = sample.split_once('{') else {
        return Vec::new();
    };
    let inner = rest.split_once('}').map_or(rest, |(inner, _)| inner);
    inner
        .split(',')
        .filter(|pair| !pair.is_empty())
        .map(|pair| {
            let (name, value) = pair.split_once('=').expect("label pair");
            (name.to_owned(), value.trim_matches('"').to_owned())
        })
        .collect()
}

#[tokio::test]
async fn a_mixed_run_moves_every_transfer_series_with_documented_labels_only() {
    // Arrange
    let mut rig = Rig::new().await;
    let metrics = Metrics::new(BuildInfo {
        version: "test",
        commit: "test",
        test_clock: true,
    })
    .expect("metrics");
    rig.service = Arc::new(rig.instance().with_metrics(&metrics).expect("series"));
    let data = content(59, 3 * MIB as usize);
    let files = rig.stage_files(rig.account, "scraped", 2).await;
    let entries = files.iter().map(|file| entry(file, rig.root)).collect();

    // Act
    let committed = rig
        .commit_bytes("secret-name.bin", "scraped-key", data.clone(), None)
        .await
        .expect("block-list commit");
    let refused = rig
        .start(request(rig.root, "too-big.bin", 11 * GIB, "refused"))
        .await;
    let aborted = started(rig.start(request(rig.root, "gone.bin", MIB, "gone")).await);
    rig.service
        .abort_upload(rig.account, aborted.upload_id)
        .await
        .expect("abort");
    let batch_result = rig
        .commit_batch(batch(Uuid::new_v4(), entries))
        .await
        .expect("batch commit");
    let downloaded = read_all(rig.open_current(committed.node_id).await.expect("open")).await;
    let rendered = metrics.render().await.expect("render");

    // Assert
    assert!(matches!(refused, Err(UploadError::QuotaExceeded)));
    assert_eq!(batch_result.per_entry.len(), 2);
    assert_eq!(downloaded, data);
    for (family, kind, _) in FAMILIES {
        assert!(
            rendered.contains(&format!("# TYPE {PREFIX}{family} {kind}\n")),
            "{family} as {kind}: {rendered}"
        );
    }
    for moved in [
        "upload_sessions_active 0".to_owned(),
        "upload_sessions_total{result=\"committed\"} 1".to_owned(),
        "upload_sessions_total{result=\"aborted\"} 1".to_owned(),
        "upload_rejections_total{reason=\"quota\"} 1".to_owned(),
        "blocks_received_total 2".to_owned(),
        format!("block_bytes_received_total {}", 3 * MIB),
        "commits_total{kind=\"block_list\",result=\"committed\"} 1".to_owned(),
        "commits_total{kind=\"batch\",result=\"committed\"} 1".to_owned(),
        "commit_duration_seconds_count{kind=\"block_list\"} 1".to_owned(),
        "commit_duration_seconds_count{kind=\"batch\"} 1".to_owned(),
        format!("download_bytes_total {}", 3 * MIB),
    ] {
        assert!(
            rendered
                .lines()
                .any(|line| line.strip_prefix(PREFIX) == Some(moved.as_str())),
            "{moved}: {rendered}"
        );
    }
    let forbidden = [
        rig.account.to_string(),
        rig.root.to_string(),
        committed.node_id.to_string(),
        committed.revision_id.to_string(),
        "secret-name".to_owned(),
        "scraped".to_owned(),
        "127.0.0.1".to_owned(),
    ];
    let mut samples = 0;
    for line in rendered.lines().filter(|line| !line.starts_with('#')) {
        let Some((_, _, documented)) = FAMILIES
            .iter()
            .find(|(family, _, _)| *family == family_of(line))
        else {
            continue;
        };
        samples += 1;
        for (name, value) in labels(line) {
            if name == "le" {
                assert!(
                    value == "+Inf" || value.parse::<f64>().is_ok(),
                    "bucket bound {line}"
                );
                continue;
            }
            let allowed = documented
                .iter()
                .find(|(label, _)| *label == name)
                .map(|(_, values)| *values)
                .unwrap_or_else(|| panic!("undocumented label {name}: {line}"));
            assert!(
                allowed.contains(&value.as_str()),
                "undocumented value {line}"
            );
        }
        for text in &forbidden {
            assert!(!line.contains(text.as_str()), "{text} in {line}");
        }
    }
    assert!(samples >= FAMILIES.len(), "{samples} samples: {rendered}");
    rig.finish().await;
}
