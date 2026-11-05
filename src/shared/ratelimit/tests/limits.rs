mod common;

use std::time::Duration;

use common::{clock, id, ip, limiter, table};
use sc_ratelimit::{LimitId, LimitSpec, Subject, Verdict};

fn subject_for(limit: LimitId) -> Subject {
    match limit {
        LimitId::DeviceApprovalSession => Subject::Session(id(1)),
        LimitId::DeviceApprovalAccount => Subject::Account(id(2)),
        LimitId::SharePasswordLink => Subject::Link(id(3)),
        _ => ip(7),
    }
}

/// AC-20 numbers: (limit, capacity, window).
const AC20: [(LimitId, u32, u64); 6] = [
    (LimitId::SigninIp, 10, 60),
    (LimitId::DeviceApprovalSession, 5, 3600),
    (LimitId::DeviceApprovalAccount, 20, 3600),
    (LimitId::DeviceApprovalIp, 50, 3600),
    (LimitId::SharePasswordLink, 10, 600),
    (LimitId::ShareTokenIp, 100, 60),
];

#[test]
fn ac1_every_limit_trips_at_its_ac20_number_and_limits_after() {
    for (limit, capacity, _) in AC20 {
        let clock = clock();
        let limiter = limiter(&clock, table());
        let subject = subject_for(limit);

        for call in 1..capacity {
            assert_eq!(
                limiter.record_failure(limit, &subject),
                Verdict::Allowed,
                "{} call {call}",
                limit.label()
            );
        }
        let tripped = limiter.record_failure(limit, &subject);
        let limited = limiter.record_failure(limit, &subject);
        let checked = limiter.check(limit, &subject);

        assert!(
            matches!(tripped, Verdict::Tripped { retry_after } if !retry_after.is_zero()),
            "{}: {tripped:?}",
            limit.label()
        );
        assert!(
            matches!(limited, Verdict::Limited { retry_after } if !retry_after.is_zero()),
            "{}: {limited:?}",
            limit.label()
        );
        assert!(matches!(checked, Verdict::Limited { .. }));
    }
}

#[test]
fn ac1_the_tripped_wait_is_one_token_interval() {
    let clock = clock();
    let limiter = limiter(&clock, table());
    let subject = subject_for(LimitId::DeviceApprovalSession);

    let verdicts: Vec<Verdict> = (0..6)
        .map(|_| limiter.record_failure(LimitId::DeviceApprovalSession, &subject))
        .collect();

    assert_eq!(
        verdicts[4],
        Verdict::Tripped {
            retry_after: Duration::from_secs(720)
        }
    );
    assert_eq!(
        verdicts[5],
        Verdict::Limited {
            retry_after: Duration::from_secs(720)
        }
    );
}

#[test]
fn ac1_configured_limits_use_the_caller_numbers() {
    let clock = clock();
    let limiter = limiter(&clock, table());

    let grant = (0..31).map(|_| limiter.record_failure(LimitId::DeviceGrantIp, &ip(1)));
    let token = (0..41).map(|_| limiter.record_failure(LimitId::InvalidTokenIp, &ip(1)));

    assert_eq!(
        grant
            .filter(|v| matches!(v, Verdict::Tripped { .. }))
            .count(),
        1
    );
    assert_eq!(
        token
            .filter(|v| matches!(v, Verdict::Tripped { .. }))
            .count(),
        1
    );
    assert_eq!(limiter.stats().get(LimitId::DeviceGrantIp).limited, 1);
    assert_eq!(limiter.stats().get(LimitId::InvalidTokenIp).limited, 1);
}

#[test]
fn ac1_share_limits_can_come_from_config() {
    let clock = clock();
    let table = table().with_spec(
        LimitId::SharePasswordLink,
        LimitSpec::new(3, Duration::from_secs(30)),
    );
    let limiter = limiter(&clock, table);
    let link = subject_for(LimitId::SharePasswordLink);

    let verdicts: Vec<Verdict> = (0..4)
        .map(|_| limiter.record_failure(LimitId::SharePasswordLink, &link))
        .collect();

    assert!(matches!(verdicts[2], Verdict::Tripped { .. }));
    assert!(matches!(verdicts[3], Verdict::Limited { .. }));
}

#[test]
fn ac2_the_eleventh_wrong_share_password_is_limited() {
    let clock = clock();
    let limiter = limiter(&clock, table());
    let link = Subject::Link(id(9));

    let verdicts: Vec<Verdict> = (0..11)
        .map(|_| limiter.record_failure(LimitId::SharePasswordLink, &link))
        .collect();

    assert!(matches!(verdicts[9], Verdict::Tripped { .. }));
    assert!(matches!(verdicts[10], Verdict::Limited { .. }));
}

#[test]
fn ac3_a_bucket_of_ten_trips_exactly_once_over_thirteen_failures() {
    let clock = clock();
    let limiter = limiter(&clock, table());

    let verdicts: Vec<Verdict> = (0..13)
        .map(|_| limiter.record_failure(LimitId::SigninIp, &ip(1)))
        .collect();

    let tripped = verdicts
        .iter()
        .filter(|v| matches!(v, Verdict::Tripped { .. }))
        .count();
    let limited = verdicts
        .iter()
        .filter(|v| matches!(v, Verdict::Limited { .. }))
        .count();
    assert_eq!((tripped, limited), (1, 3));
    assert!(matches!(verdicts[9], Verdict::Tripped { .. }));
}

#[test]
fn subjects_and_limits_do_not_share_buckets() {
    let clock = clock();
    let limiter = limiter(&clock, table());
    for _ in 0..10 {
        limiter.record_failure(LimitId::SigninIp, &ip(1));
    }

    let other_ip = limiter.check(LimitId::SigninIp, &ip(2));
    let other_limit = limiter.check(LimitId::DeviceApprovalIp, &ip(1));

    assert_eq!(other_ip, Verdict::Allowed);
    assert_eq!(other_limit, Verdict::Allowed);
}

#[test]
fn check_takes_nothing_from_the_bucket() {
    let clock = clock();
    let limiter = limiter(&clock, table());

    for _ in 0..50 {
        assert_eq!(limiter.check(LimitId::SigninIp, &ip(1)), Verdict::Allowed);
    }

    assert_eq!(limiter.bucket_count(), 0);
    let verdicts: Vec<Verdict> = (0..10)
        .map(|_| limiter.record_failure(LimitId::SigninIp, &ip(1)))
        .collect();
    assert!(matches!(verdicts[9], Verdict::Tripped { .. }));
}

#[test]
fn clear_forgets_the_failures() {
    let clock = clock();
    let limiter = limiter(&clock, table());
    for _ in 0..10 {
        limiter.record_failure(LimitId::SigninIp, &ip(1));
    }

    limiter.clear(LimitId::SigninIp, &ip(1));

    assert_eq!(limiter.check(LimitId::SigninIp, &ip(1)), Verdict::Allowed);
    assert_eq!(limiter.bucket_count(), 0);
}
