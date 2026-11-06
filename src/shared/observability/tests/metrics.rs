//! AC-6 low cardinality, AC-7 prefix and format, collectors, NFR timings.

use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use prometheus_parse::{Scrape, Value};
#[cfg(feature = "db")]
use sc_audit::AuditStats;
use sc_clock::{ManualClock, Timestamp};
use sc_observability::{
    BuildInfo, HISTOGRAM_BUCKETS, LabelSet, Metrics, NoLabels, ObsError, ratelimit_collector,
};
use sc_ratelimit::{AbuseLimiter, LimitId, LimitSpec, LimitTable, Subject};

#[derive(Clone, Copy)]
enum Kind {
    Thumbnail,
    Transcode,
}

impl LabelSet for Kind {
    const NAMES: &'static [&'static str] = &["kind"];

    fn value(&self, _index: usize) -> &'static str {
        match self {
            Kind::Thumbnail => "thumbnail",
            Kind::Transcode => "transcode",
        }
    }
}

#[derive(Clone, Copy)]
struct Account;

impl LabelSet for Account {
    const NAMES: &'static [&'static str] = &["account_id"];

    fn value(&self, _index: usize) -> &'static str {
        "00000000-0000-0000-0000-000000000000"
    }
}

fn metrics() -> Metrics {
    Metrics::new(BuildInfo {
        version: "1.2.3",
        commit: "abc1234",
        test_clock: true,
    })
    .expect("metrics")
}

fn parse(text: &str) -> Scrape {
    Scrape::parse(text.lines().map(|line| Ok(line.to_owned())))
        .expect("a Prometheus text parser accepts the output")
}

fn value_of(scrape: &Scrape, name: &str, label: Option<(&str, &str)>) -> Option<f64> {
    scrape
        .samples
        .iter()
        .find(|sample| {
            sample.metric == name
                && label.is_none_or(|(key, value)| sample.labels.get(key) == Some(value))
        })
        .map(|sample| match sample.value {
            Value::Counter(v) | Value::Gauge(v) | Value::Untyped(v) => v,
            _ => f64::NAN,
        })
}

fn bad_label_reason<T>(result: Result<T, ObsError>) -> String {
    match result {
        Err(ObsError::BadLabel { reason, .. }) => reason,
        Err(other) => panic!("expected BadLabel, got {other:?}"),
        Ok(_) => panic!("expected BadLabel, the constructor accepted the label set"),
    }
}

#[test]
fn ac6_a_label_set_naming_an_unbounded_value_is_refused_with_bad_label() {
    let metrics = metrics();

    let reason = bad_label_reason(metrics.counter::<Account>("requests_total"));

    assert!(reason.contains("account_id"), "{reason}");
}

#[test]
fn ac6_the_unbounded_names_of_the_spec_are_all_refused() {
    macro_rules! labelled {
        ($($name:ident => $label:literal),*) => {$(
            #[derive(Clone, Copy)]
            struct $name;
            impl LabelSet for $name {
                const NAMES: &'static [&'static str] = &[$label];
                fn value(&self, _index: usize) -> &'static str { "x" }
            }
            let reason = bad_label_reason(metrics().gauge::<$name>("probe"));
            assert!(reason.contains($label), "{}: {reason}", $label);
        )*};
    }
    labelled!(
        Node => "node_id", File => "file", FilePath => "file_path", Share => "share", ShareId => "share_id",
        Ip => "ip", ClientIp => "client_ip", Email => "email", Token => "token", Session => "session_id",
        RequestId => "request_id", UserAgent => "user_agent_host", Url => "url"
    );
}

#[test]
fn ac6_malformed_label_sets_are_refused() {
    macro_rules! labelled {
        ($name:ident, [$($label:literal),*]) => {{
            #[derive(Clone, Copy)]
            struct $name;
            impl LabelSet for $name {
                const NAMES: &'static [&'static str] = &[$($label),*];
                fn value(&self, _index: usize) -> &'static str { "x" }
            }
            bad_label_reason(metrics().counter::<$name>("probe"))
        }};
    }
    assert!(labelled!(Twice, ["kind", "kind"]).contains("twice"));
    assert!(labelled!(Reserved, ["le"]).contains("usable"));
    assert!(labelled!(Upper, ["Kind"]).contains("usable"));
    assert!(labelled!(Empty, [""]).contains("usable"));
    assert!(labelled!(TooMany, ["a", "b", "c", "d", "e"]).contains("more than"));
}

#[test]
fn ac6_names_with_a_prefix_or_bad_characters_and_taken_names_are_refused() {
    let metrics = metrics();

    assert!(matches!(
        metrics.counter::<NoLabels>("sunnycloud_requests_total"),
        Err(ObsError::BadName(_))
    ));
    assert!(matches!(
        metrics.counter::<NoLabels>("Requests"),
        Err(ObsError::BadName(_))
    ));
    assert!(matches!(
        metrics.counter::<NoLabels>(""),
        Err(ObsError::BadName(_))
    ));
    metrics
        .counter::<NoLabels>("requests_total")
        .expect("first");
    assert!(matches!(
        metrics.counter::<NoLabels>("requests_total"),
        Err(ObsError::Duplicate(name)) if name == "requests_total"
    ));
    assert!(matches!(
        metrics.gauge::<Kind>("requests_total"),
        Err(ObsError::Duplicate(_) | ObsError::Registry(_))
    ));
}

#[tokio::test]
async fn ac6_after_ten_thousand_increments_only_enumerated_label_values_are_exported() {
    let metrics = metrics();
    let jobs = metrics.counter::<Kind>("jobs_total").expect("counter");
    for index in 0..10_000 {
        jobs.inc(if index % 3 == 0 {
            Kind::Thumbnail
        } else {
            Kind::Transcode
        });
    }

    let scrape = parse(&metrics.render().await.expect("render"));

    let kinds: Vec<_> = scrape
        .samples
        .iter()
        .filter(|sample| sample.metric == "sunnycloud_jobs_total")
        .map(|sample| sample.labels.get("kind").expect("kind label").to_owned())
        .collect();
    assert_eq!(kinds.len(), 2);
    assert!(
        kinds
            .iter()
            .all(|kind| kind == "thumbnail" || kind == "transcode")
    );
    assert_eq!(jobs.get(Kind::Thumbnail), 3334);
    assert_eq!(jobs.get(Kind::Transcode), 6666);
}

fn limiter() -> Arc<AbuseLimiter> {
    let clock = Arc::new(ManualClock::new(Timestamp::from_unix_micros(0)));
    let spec = LimitSpec::new(2, Duration::from_secs(60));
    Arc::new(AbuseLimiter::new(clock, LimitTable::new(spec, spec)).expect("limiter"))
}

#[tokio::test]
async fn ac7_scrape_parses_every_name_has_the_prefix_once_and_helper_families_are_present() {
    let metrics = metrics();
    let limiter = limiter();
    let subject = Subject::Ip("10.0.0.1".parse().expect("ip"));
    for _ in 0..4 {
        limiter.record_failure(LimitId::DeviceGrantIp, &subject);
    }
    {
        let limiter = Arc::clone(&limiter);
        metrics
            .register_collector(ratelimit_collector(move || limiter.stats()))
            .expect("ratelimit collector");
    }
    #[cfg(feature = "db")]
    metrics
        .register_collector(sc_observability::audit_collector(|| AuditStats {
            write_failures: 3,
        }))
        .expect("audit collector");
    let jobs = metrics.counter::<Kind>("jobs_total").expect("counter");
    jobs.inc(Kind::Thumbnail);
    let queued = metrics.gauge::<Kind>("jobs_queued").expect("gauge");
    queued.set(Kind::Transcode, 7.0);
    let duration = metrics
        .histogram::<Kind>("job_duration_seconds")
        .expect("histogram");
    duration.observe(Kind::Thumbnail, 0.3);

    let text = metrics.render().await.expect("render");
    let scrape = parse(&text);

    for line in text.lines().filter(|line| !line.is_empty()) {
        let name = line
            .strip_prefix("# HELP ")
            .or_else(|| line.strip_prefix("# TYPE "))
            .unwrap_or(line);
        assert!(name.starts_with("sunnycloud_"), "{line}");
        assert!(!name.starts_with("sunnycloud_sunnycloud_"), "{line}");
    }
    assert!(!scrape.samples.is_empty());
    assert!(
        scrape
            .samples
            .iter()
            .all(|sample| sample.metric.starts_with("sunnycloud_"))
    );
    assert_eq!(
        value_of(&scrape, "sunnycloud_build_info", Some(("version", "1.2.3"))),
        Some(1.0)
    );
    let build = scrape
        .samples
        .iter()
        .find(|sample| sample.metric == "sunnycloud_build_info")
        .expect("build_info");
    assert_eq!(build.labels.get("commit"), Some("abc1234"));
    assert_eq!(build.labels.get("test_clock"), Some("true"));
    for family in [
        "sunnycloud_process_resident_memory_bytes",
        "sunnycloud_process_open_fds",
        "sunnycloud_process_cpu_seconds_total",
    ] {
        assert!(
            value_of(&scrape, family, None).is_some_and(|v| v >= 0.0),
            "{family}"
        );
    }
    #[cfg(feature = "db")]
    assert_eq!(
        value_of(&scrape, "sunnycloud_audit_write_failures_total", None),
        Some(3.0)
    );
    let limited: Vec<_> = scrape
        .samples
        .iter()
        .filter(|sample| sample.metric == "sunnycloud_auth_rate_limited_total")
        .collect();
    assert_eq!(
        limited.len(),
        LimitId::ALL.len(),
        "every limit is exported, also at zero"
    );
    assert_eq!(
        value_of(
            &scrape,
            "sunnycloud_auth_rate_limited_total",
            Some(("limit", "device_grant_ip"))
        ),
        Some(3.0),
        "capacity 2: the second failure trips, the third and fourth are limited"
    );
    assert_eq!(
        value_of(
            &scrape,
            "sunnycloud_auth_rate_limited_total",
            Some(("limit", "share_token_ip"))
        ),
        Some(0.0)
    );
    assert_eq!(
        value_of(
            &scrape,
            "sunnycloud_jobs_queued",
            Some(("kind", "transcode"))
        ),
        Some(7.0)
    );
    assert!(text.contains("# TYPE sunnycloud_job_duration_seconds histogram"));
}

#[tokio::test]
async fn ac7_every_histogram_family_uses_the_one_bucket_set() {
    let metrics = metrics();
    let first = metrics
        .histogram::<Kind>("blob_read_duration_seconds")
        .expect("first");
    let second = metrics
        .histogram::<NoLabels>("commit_duration_seconds")
        .expect("second");
    first.observe(Kind::Thumbnail, 0.002);
    second.observe(NoLabels, 45.0);

    let text = metrics.render().await.expect("render");

    for family in [
        "sunnycloud_blob_read_duration_seconds",
        "sunnycloud_commit_duration_seconds",
    ] {
        let bounds: Vec<String> = text
            .lines()
            .filter(|line| line.starts_with(&format!("{family}_bucket")))
            .map(|line| {
                line.split("le=\"")
                    .nth(1)
                    .expect("le")
                    .split('"')
                    .next()
                    .expect("bound")
                    .to_owned()
            })
            .collect();
        let expected: Vec<String> = HISTOGRAM_BUCKETS
            .iter()
            .map(|bound| format!("{bound}"))
            .chain(["+Inf".to_owned()])
            .collect();
        assert_eq!(bounds, expected, "{family}");
    }
}

#[test]
fn register_collector_refuses_a_second_registration_with_duplicate() {
    let metrics = metrics();
    let limiter = limiter();
    let first = Arc::clone(&limiter);
    metrics
        .register_collector(ratelimit_collector(move || first.stats()))
        .expect("first");

    let second = metrics.register_collector(ratelimit_collector(move || limiter.stats()));

    assert!(matches!(second, Err(ObsError::Duplicate(_))), "{second:?}");
}

#[tokio::test]
async fn gauge_and_unlabelled_counter_round_trip_through_the_scrape() {
    let metrics = metrics();
    let bytes = metrics
        .counter::<NoLabels>("download_bytes_total")
        .expect("counter");
    bytes.inc_by(NoLabels, 1500);
    bytes.inc(NoLabels);
    let free = metrics.gauge::<NoLabels>("blob_free_bytes").expect("gauge");
    free.set(NoLabels, 10.0);
    free.add(NoLabels, 2.5);

    let scrape = parse(&metrics.render().await.expect("render"));

    assert_eq!(
        value_of(&scrape, "sunnycloud_download_bytes_total", None),
        Some(1501.0)
    );
    assert_eq!(
        value_of(&scrape, "sunnycloud_blob_free_bytes", None),
        Some(12.5)
    );
}

#[derive(Clone, Copy)]
struct Many(usize);

fn many_values() -> &'static [&'static str] {
    static VALUES: OnceLock<Vec<&'static str>> = OnceLock::new();
    VALUES.get_or_init(|| {
        (0..5000)
            .map(|index| &*Box::leak(format!("v{index}").into_boxed_str()))
            .collect()
    })
}

impl LabelSet for Many {
    const NAMES: &'static [&'static str] = &["bucket"];

    fn value(&self, _index: usize) -> &'static str {
        many_values()[self.0]
    }
}

#[tokio::test]
async fn nfr_five_thousand_series_render_quickly() {
    let metrics = metrics();
    let family = metrics.counter::<Many>("series_total").expect("counter");
    for index in 0..5000 {
        family.inc(Many(index));
    }

    let started = Instant::now();
    let text = metrics.render().await.expect("render");
    let elapsed = started.elapsed();

    eprintln!("render of 5000 series: {elapsed:?} ({} bytes)", text.len());
    assert!(
        text.lines()
            .filter(|line| line.starts_with("sunnycloud_series_total"))
            .count()
            == 5000
    );
    let limit = if cfg!(debug_assertions) {
        Duration::from_millis(500)
    } else {
        Duration::from_millis(50)
    };
    assert!(elapsed < limit, "{elapsed:?} against {limit:?}");
}

#[test]
fn nfr_a_disabled_level_event_costs_microseconds() {
    let sink = Arc::new(NullSink);
    let clock = Arc::new(ManualClock::new(Timestamp::from_unix_micros(0)));
    let (subscriber, _) = sc_observability::subscriber(
        sc_observability::LogConfig {
            format: sc_observability::LogFormat::Json,
            level: tracing::Level::ERROR,
            component: sc_observability::Component::Api,
        },
        clock,
        sink,
    );

    let per_event = tracing::subscriber::with_default(subscriber, || {
        let events = 200_000_u32;
        let started = Instant::now();
        for _ in 0..events {
            sc_observability::log_debug!("disabled", status = 200);
        }
        started.elapsed() / events
    });

    eprintln!("disabled-level event: {per_event:?}");
    assert!(per_event < Duration::from_micros(5), "{per_event:?}");
}

struct NullSink;

impl sc_observability::LineSink for NullSink {
    fn write_line(&self, _line: &str) -> std::io::Result<()> {
        Ok(())
    }
}
