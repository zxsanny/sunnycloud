//! AZ-3874 AC-5: the worker families after a scripted run of the real slot loop.

mod api_support;
mod slot_support;

use media_shared::decoder_protocol::{DecodeError, LimitKind};
use media_shared::types::JobKind;
use slot_support::{Answer, Entry, Rig, ScriptedApi, compliant_probe, photo_probe, remux_probe};
use sunnycloud_worker::{JobEnd, KillReason, SlotName};

const WORKER: &str = "worker-metrics";
const RESULTS: [&str; 9] = [
    "ok",
    "remuxed",
    "skipped_compliant",
    "obsolete",
    "retry",
    "permanent",
    "lease_lost",
    "refused",
    "released",
];
const KINDS: [&str; 3] = ["stills", "poster", "transcode"];
const REASONS: [&str; 3] = ["stall", "timeout", "memory"];

fn sample<'a>(text: &'a str, series: &str) -> Option<&'a str> {
    text.lines()
        .find_map(|line| line.strip_prefix(series))
        .map(str::trim)
}

fn label_values<'a>(text: &'a str, family: &str, label: &str) -> Vec<&'a str> {
    let needle = format!("{label}=\"");
    text.lines()
        .filter(|line| line.starts_with(family))
        .filter_map(|line| line.split_once(&needle))
        .filter_map(|(_, rest)| rest.split('"').next())
        .collect()
}

async fn scripted_run() -> (ScriptedApi, Rig) {
    let api = ScriptedApi::start().await;
    api.enqueue(Entry::new(1, JobKind::Transcode));
    api.enqueue(Entry::new(2, JobKind::Stills));
    api.enqueue(Entry::new(3, JobKind::Stills).input_status(404));
    api.enqueue(Entry::new(4, JobKind::Stills));
    let mut rig = Rig::new(&api, SlotName::General, WORKER);
    rig.decoder
        .probes([
            Answer::Probe(remux_probe()),
            Answer::Probe(photo_probe()),
            Answer::Probe(photo_probe()),
        ])
        .transcodes([Answer::Transcode(compliant_probe(60_000))])
        .stills([
            Answer::Image {
                width: 1440,
                height: 1080,
            },
            Answer::Image {
                width: 320,
                height: 240,
            },
            Answer::Error(DecodeError::LimitExceeded {
                limit: LimitKind::Memory,
            }),
        ]);
    for _ in 0..4 {
        rig.run_one().await;
    }
    (api, rig)
}

#[tokio::test]
async fn ac5_each_job_result_is_counted_under_its_kind_and_a_memory_kill_is_counted() {
    // Arrange
    let (_api, rig) = scripted_run().await;

    // Act
    let text = rig.metrics.render().await.expect("renders");

    // Assert
    let jobs = |kind: &str, result: &str| {
        sample(
            &text,
            &format!("sunnycloud_jobs_total{{kind=\"{kind}\",result=\"{result}\"}}"),
        )
    };
    assert_eq!(jobs("stills", "ok"), Some("1"), "{text}");
    assert_eq!(jobs("stills", "obsolete"), Some("1"), "{text}");
    assert_eq!(jobs("transcode", "remuxed"), Some("1"), "{text}");
    assert_eq!(jobs("stills", "permanent"), Some("1"), "{text}");
    assert_eq!(
        sample(&text, "sunnycloud_decoder_kills_total{reason=\"memory\"}"),
        Some("1"),
        "{text}"
    );
    assert_eq!(rig.jobs.kills(KillReason::Memory), 1);
    assert_eq!(rig.jobs.kills(KillReason::Stall), 0);
    assert_eq!(rig.jobs.jobs(JobKind::Stills, &JobEnd::Obsolete), 1);
}

#[tokio::test]
async fn ac5_the_decoder_families_are_set_after_the_gate() {
    // Arrange
    let (_api, rig) = scripted_run().await;

    // Act
    let text = rig.metrics.render().await.expect("renders");

    // Assert
    assert_eq!(
        sample(&text, "sunnycloud_worker_selfcheck_ok{slot=\"general\"}"),
        Some("1"),
        "{text}"
    );
    let built = sample(
        &text,
        "sunnycloud_worker_decoder_build_timestamp_seconds{slot=\"general\"}",
    )
    .and_then(|value| value.parse::<f64>().ok());
    assert_eq!(built, Some(1_700_000_000.0), "{text}");
    assert!(
        text.contains("sunnycloud_job_duration_seconds_count 4"),
        "{text}"
    );
}

#[tokio::test]
async fn ac5_every_label_value_is_from_a_fixed_enumeration() {
    // Arrange
    let (_api, rig) = scripted_run().await;

    // Act
    let text = rig.metrics.render().await.expect("renders");

    // Assert
    for (family, label, allowed) in [
        ("sunnycloud_jobs_total", "result", &RESULTS[..]),
        ("sunnycloud_jobs_total", "kind", &KINDS[..]),
        ("sunnycloud_decoder_kills_total", "reason", &REASONS[..]),
        (
            "sunnycloud_worker_selfcheck_ok",
            "slot",
            &["interactive", "general"][..],
        ),
    ] {
        let seen = label_values(&text, family, label);
        assert!(!seen.is_empty(), "no {label} on {family}: {text}");
        for value in seen {
            assert!(allowed.contains(&value), "{family} {label}={value}");
        }
    }
}
