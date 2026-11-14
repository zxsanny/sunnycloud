//! PT-06-2 (AZ-3703 AC-6, AC-12, NFT-SEC-17): one client opens an unknown token and links that are expired,
//! revoked, on a tombstoned file and on a purged file, interleaved with a rotating start so no case always runs
//! first. Every answer must be the same `NotFound` value; the result line gives each case's median and the largest
//! difference between two medians. A single client sends far more than 100 unknown tokens a minute, so the
//! service under test counts on a limiter whose invalid-token limit is raised: every `NotFound` still records a
//! failure, so each case does the same limiter work as in production.

use std::time::{Duration, Instant};

use contracts::{PublicShareService, ShareOpen};
use sc_ids::RawToken;
use sqlx::Connection;

use crate::harness::Harness;
use crate::visitor_support::{VISITOR, cases, unknown_token};

const CI_SAMPLES: usize = 200;
const REF_SAMPLES: usize = 10_000;
/// Above the calls of the largest run, so the address is never limited.
const RAISED_INVALID_TOKEN_LIMIT: u64 = 1_000_000;

struct Report {
    medians: Vec<(&'static str, Duration)>,
    unequal: Vec<String>,
}

impl Report {
    fn largest_difference(&self) -> Duration {
        let times = self.medians.iter().map(|(_, median)| *median);
        let (low, high) = (times.clone().min(), times.max());
        high.expect("a case").saturating_sub(low.expect("a case"))
    }
}

fn median(mut times: Vec<Duration>) -> Duration {
    times.sort_unstable();
    times[times.len() / 2]
}

async fn run(samples: usize) -> Report {
    let harness = Harness::new().await;
    let cases = cases(&harness).await;
    let mut config = harness.config.clone();
    config.invalid_token_limit = RAISED_INVALID_TOKEN_LIMIT;
    let shares = harness.shares_limited_by(&config);
    let unknown = unknown_token();
    let missing: [(&'static str, &RawToken); 5] = [
        ("unknown", &unknown),
        ("expired", &cases.expired),
        ("revoked", &cases.revoked),
        ("tombstoned", &cases.tombstoned),
        ("purged", &cases.purged),
    ];
    let reference = format!("{:?}", Ok::<_, ()>(ShareOpen::NotFound));
    for (_, token) in &missing {
        shares.open(token, VISITOR).await.expect("warm-up open");
    }
    let mut times: Vec<Vec<Duration>> = missing
        .iter()
        .map(|_| Vec::with_capacity(samples))
        .collect();
    let mut unequal = Vec::new();
    for sample in 0..samples {
        for step in 0..missing.len() {
            let case = (sample + step) % missing.len();
            let began = Instant::now();
            let answer = shares.open(missing[case].1, VISITOR).await;
            times[case].push(began.elapsed());
            let written = format!("{:?}", answer.map_err(|_| ()));
            if written != reference {
                unequal.push(format!("{}: {written}", missing[case].0));
            }
        }
    }
    let medians = missing
        .iter()
        .zip(times)
        .map(|((name, _), case_times)| (*name, median(case_times)))
        .collect();
    let mut conn = harness.fixture_conn().await;
    let limited: i64 =
        sqlx::query_scalar("SELECT count(*) FROM audit_log WHERE event = 'abuse_trip'")
            .fetch_one(&mut conn)
            .await
            .expect("count trips");
    conn.close().await.expect("close fixture connection");
    assert_eq!(limited, 0, "the raised limit never trips");
    drop((shares, cases));
    harness.finish().await;
    Report { medians, unequal }
}

fn print_and_check(label: &str, samples: usize, report: &Report) -> Duration {
    let difference = report.largest_difference();
    println!(
        "PT-06-2 {label} on {}: {samples} samples per case, medians {:?}, largest difference {difference:?}, \
         {} unequal answers",
        crate::load::host(),
        report.medians,
        report.unequal.len()
    );
    assert_eq!(
        report.unequal,
        Vec::<String>::new(),
        "response values are equal"
    );
    difference
}

/// The functional part, in CI on any host: every missing case answers the same value. Its medians are printed
/// only; they are `NOT-RUN(HW-REF)` here.
#[tokio::test]
async fn pt_06_2_functional_missing_cases_answer_one_equal_value() {
    // Arrange
    let samples = CI_SAMPLES;

    // Act
    let report = run(samples).await;

    // Assert
    assert_eq!(report.medians.len(), 5);
    print_and_check("functional (timing NOT-RUN(HW-REF))", samples, &report);
}

/// PT-06-2 on a quiet REF-SRV only: 10,000 interleaved samples per case; the medians of any two cases differ by at
/// most 0.3 ms (fail 1 ms).
#[tokio::test]
#[ignore = "NOT-RUN(HW-REF): PT-06-2 timing uniformity of missing links is measured on REF-SRV only"]
async fn pt_06_2_missing_cases_take_the_same_time() {
    // Arrange
    let samples = REF_SAMPLES;

    // Act
    let report = run(samples).await;

    // Assert
    let difference = print_and_check("REF-SRV", samples, &report);
    crate::load::report(
        "PT-06-2",
        "largest median difference",
        difference,
        Duration::from_micros(300),
        Duration::from_millis(1),
    );
    assert!(
        difference <= Duration::from_millis(1),
        "fail above 1 ms: {difference:?}"
    );
    assert!(
        difference <= Duration::from_micros(300),
        "target 0.3 ms: {difference:?}"
    );
}
