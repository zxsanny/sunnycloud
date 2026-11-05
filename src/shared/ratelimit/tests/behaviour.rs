mod common;

use std::sync::Arc;
use std::time::Duration;

use common::{clock, ip, limiter, table};
use sc_clock::ManualClock;
use sc_ratelimit::{AbuseLimiter, ConfigError, LimitId, LimitSpec, Subject, Verdict};

#[test]
fn ac4_an_exhausted_bucket_allows_again_after_one_window() {
    let clock = clock();
    let limiter = limiter(&clock, table());
    for _ in 0..10 {
        limiter.record_failure(LimitId::SigninIp, &ip(1));
    }
    assert!(matches!(
        limiter.check(LimitId::SigninIp, &ip(1)),
        Verdict::Limited { .. }
    ));

    clock.tick(Duration::from_secs(60));

    assert_eq!(limiter.check(LimitId::SigninIp, &ip(1)), Verdict::Allowed);
}

#[test]
fn ac4_one_token_comes_back_every_window_over_capacity() {
    let clock = clock();
    let limiter = limiter(&clock, table());
    for _ in 0..10 {
        limiter.record_failure(LimitId::SigninIp, &ip(1));
    }

    clock.tick(Duration::from_secs(5));
    let early = limiter.check(LimitId::SigninIp, &ip(1));
    clock.tick(Duration::from_secs(1));
    let on_time = limiter.check(LimitId::SigninIp, &ip(1));

    assert_eq!(
        early,
        Verdict::Limited {
            retry_after: Duration::from_secs(1)
        }
    );
    assert_eq!(on_time, Verdict::Allowed);
}

#[test]
fn ac4_a_refilled_bucket_trips_again_on_its_next_exhaustion() {
    let clock = clock();
    let limiter = limiter(&clock, table());
    for _ in 0..10 {
        limiter.record_failure(LimitId::SigninIp, &ip(1));
    }
    clock.tick(Duration::from_secs(60));

    let verdicts: Vec<Verdict> = (0..11)
        .map(|_| limiter.record_failure(LimitId::SigninIp, &ip(1)))
        .collect();

    assert!(matches!(verdicts[9], Verdict::Tripped { .. }));
    assert!(matches!(verdicts[10], Verdict::Limited { .. }));
}

#[test]
fn ac5_the_map_never_exceeds_the_cap_and_keeps_the_newest_buckets() {
    let clock = clock();
    let limiter = limiter(&clock, table().with_max_entries(1000));

    for n in 0..2000 {
        limiter.record_failure(LimitId::SigninIp, &ip(n));
        assert!(limiter.bucket_count() <= 1000, "after address {n}");
    }

    // The newest failure of each address is still counted: its bucket is not at capacity.
    let newest_kept = (1990..2000)
        .filter(|&n| limiter.check(LimitId::SigninIp, &ip(n)) == Verdict::Allowed)
        .count();
    assert_eq!(newest_kept, 10);
    assert!(limiter.bucket_count() > 900);
}

#[test]
fn ac5_an_evicted_bucket_starts_fresh_and_a_recent_one_keeps_counting() {
    let clock = clock();
    let limiter = limiter(&clock, table().with_max_entries(1000));
    for _ in 0..9 {
        limiter.record_failure(LimitId::SigninIp, &ip(0));
    }
    for n in 1..3000 {
        limiter.record_failure(LimitId::SigninIp, &ip(n));
    }

    // ip(0) was the oldest idle bucket: evicted, so the tenth failure is the first of a new bucket.
    assert_eq!(
        limiter.record_failure(LimitId::SigninIp, &ip(0)),
        Verdict::Allowed
    );
}

#[test]
fn ac5_full_buckets_are_dropped() {
    let clock = clock();
    let limiter = limiter(&clock, table().with_max_entries(1000));
    for n in 0..16 {
        limiter.record_failure(LimitId::SigninIp, &ip(n));
    }
    let before = limiter.bucket_count();

    clock.tick(Duration::from_secs(60));
    for n in 100..132 {
        limiter.record_failure(LimitId::SigninIp, &ip(n));
    }

    assert_eq!(before, 16);
    assert!(
        limiter.bucket_count() <= 32 + 15,
        "{}",
        limiter.bucket_count()
    );
}

#[test]
fn ac5_a_tiny_cap_is_still_held() {
    let clock = clock();
    let limiter = limiter(&clock, table().with_max_entries(3));

    for n in 0..50 {
        limiter.record_failure(LimitId::SigninIp, &ip(n));
    }

    assert!(limiter.bucket_count() <= 3);
}

#[test]
fn ac6_a_thousand_parallel_failures_trip_exactly_once() {
    let clock = clock();
    let limiter = Arc::new(limiter(&clock, table()));
    let link = Subject::Link(common::id(1));

    let verdicts: Vec<Verdict> = std::thread::scope(|scope| {
        let workers: Vec<_> = (0..8)
            .map(|_| {
                let limiter = limiter.clone();
                scope.spawn(move || {
                    (0..125)
                        .map(|_| limiter.record_failure(LimitId::SharePasswordLink, &link))
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        workers
            .into_iter()
            .flat_map(|worker| worker.join().expect("worker panicked"))
            .collect()
    });

    let count = |wanted: fn(&Verdict) -> bool| verdicts.iter().filter(|v| wanted(v)).count();
    assert_eq!(verdicts.len(), 1000);
    assert_eq!(count(|v| matches!(v, Verdict::Tripped { .. })), 1);
    assert_eq!(count(|v| matches!(v, Verdict::Allowed)), 9);
    assert_eq!(count(|v| matches!(v, Verdict::Limited { .. })), 990);
}

fn build(table: sc_ratelimit::LimitTable) -> Result<AbuseLimiter, ConfigError> {
    let clock: Arc<ManualClock> = clock();
    AbuseLimiter::new(clock, table)
}

#[test]
fn ac7_a_zero_capacity_is_rejected_naming_the_limit() {
    let table = table().with_spec(
        LimitId::DeviceApprovalAccount,
        LimitSpec::new(0, Duration::from_secs(60)),
    );

    let error = build(table).expect_err("zero capacity");

    assert_eq!(
        error,
        ConfigError::ZeroCapacity(LimitId::DeviceApprovalAccount)
    );
    assert!(error.to_string().contains("device_approval_account"));
}

#[test]
fn ac7_a_zero_window_is_rejected_naming_the_limit() {
    let table = table().with_spec(LimitId::ShareTokenIp, LimitSpec::new(5, Duration::ZERO));

    let error = build(table).expect_err("zero window");

    assert_eq!(error, ConfigError::ZeroWindow(LimitId::ShareTokenIp));
    assert!(error.to_string().contains("share_token_ip"));
}

#[test]
fn ac7_a_zero_bucket_cap_is_rejected() {
    let error = build(table().with_max_entries(0)).expect_err("zero cap");

    assert_eq!(error, ConfigError::ZeroMaxEntries);
}

#[test]
fn ac7_the_two_unset_limits_are_validated_too() {
    let table = sc_ratelimit::LimitTable::new(
        LimitSpec::new(0, Duration::from_secs(60)),
        LimitSpec::new(5, Duration::from_secs(60)),
    );

    let error = build(table).expect_err("zero capacity");

    assert_eq!(error, ConfigError::ZeroCapacity(LimitId::DeviceGrantIp));
}

#[test]
fn ac8_stats_count_limited_and_tripped_under_the_limit_only() {
    let clock = clock();
    let limiter = limiter(&clock, table());
    for _ in 0..13 {
        limiter.record_failure(LimitId::SigninIp, &ip(1));
    }
    limiter.record_failure(LimitId::DeviceApprovalIp, &ip(1));

    let stats = limiter.stats();

    // 13 failures on capacity 10: 1 Tripped, 3 Limited; the one other-limit failure is Allowed.
    assert_eq!(stats.get(LimitId::SigninIp).tripped, 1);
    assert_eq!(stats.get(LimitId::SigninIp).limited, 3);
    for (limit, counts) in stats
        .iter()
        .filter(|(limit, _)| *limit != LimitId::SigninIp)
    {
        assert_eq!(
            (counts.limited, counts.tripped),
            (0, 0),
            "{}",
            limit.label()
        );
    }
}

#[test]
fn ac8_a_limited_check_counts_as_limited() {
    let clock = clock();
    let limiter = limiter(&clock, table());
    for _ in 0..10 {
        limiter.record_failure(LimitId::SigninIp, &ip(1));
    }

    limiter.check(LimitId::SigninIp, &ip(1));
    limiter.check(LimitId::SigninIp, &ip(2));

    assert_eq!(limiter.stats().get(LimitId::SigninIp).limited, 1);
}

#[test]
fn the_limit_labels_match_the_metric_label_values() {
    let labels: Vec<&str> = LimitId::ALL.iter().map(|limit| limit.label()).collect();

    assert_eq!(
        labels,
        [
            "signin_ip",
            "device_grant_ip",
            "device_approval_session",
            "device_approval_account",
            "device_approval_ip",
            "invalid_token_ip",
            "share_password",
            "share_token_ip",
        ]
    );
}
