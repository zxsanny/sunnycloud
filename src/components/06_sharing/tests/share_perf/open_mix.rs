//! PT-06-1 (AZ-3703 AC-6, AC-12): visitors on links of the link factory (20% password links, opened with a grant
//! from the right password) send a paced mix of half `open`, half `authorize_content` (every other one starting a
//! download), each visitor from an address of its own, ramped up over the first seconds. Every answer is checked
//! against the link it was sent to; the result line gives both p95 times, the request count and the database
//! connections. The CI run is a small load with the functional assertions only.

use std::sync::Arc;
use std::time::{Duration, Instant};

use contracts::{AccessGrant, ShareOpen};
use sunnycloud_sharing::PgShareService;
use tokio::time::MissedTickBehavior;

use crate::harness::Harness;
use crate::link_factory::{FactoryLink, LinkFactory, MEDIA_TYPE};
use crate::load::{address, host, percentile};
use crate::visitor_support::VISITOR;

const MAX_CONNECTIONS: i64 = 12;

struct Scale {
    links: usize,
    visitors: usize,
    requests_per_second: usize,
    duration: Duration,
    ramp_up: Duration,
}

const CI: Scale = Scale {
    links: 100,
    visitors: 20,
    requests_per_second: 200,
    duration: Duration::from_secs(2),
    ramp_up: Duration::from_millis(500),
};

const REF_SRV: Scale = Scale {
    links: 1000,
    visitors: 200,
    requests_per_second: 1000,
    duration: Duration::from_secs(180),
    ramp_up: Duration::from_secs(20),
};

/// A link with the grant its visitors present (password links only).
struct Target {
    link: FactoryLink,
    grant: Option<AccessGrant>,
}

#[derive(Default)]
struct Report {
    opens: Vec<Duration>,
    contents: Vec<Duration>,
    errors: Vec<String>,
}

impl Report {
    fn merge(&mut self, other: Report) {
        self.opens.extend(other.opens);
        self.contents.extend(other.contents);
        self.errors.extend(other.errors);
    }
}

/// One request to `target`: `open` on even `n`, `authorize_content` on odd `n`; a wrong answer is an error.
async fn request(
    shares: &PgShareService,
    target: &Target,
    n: usize,
    visitor: usize,
    report: &mut Report,
) {
    let (link, ip) = (&target.link, address(1, visitor));
    let began = Instant::now();
    if n.is_multiple_of(2) {
        let answer = shares.open(&link.created.token, ip).await;
        report.opens.push(began.elapsed());
        let expected = match link.password {
            Some(_) => ShareOpen::PasswordRequired,
            None => ShareOpen::Available {
                name: link.file.name.clone(),
                size: link.file.size,
                media_type: Some(MEDIA_TYPE.to_owned()),
                expires_at: link.created.expires_at,
            },
        };
        if answer.as_ref().ok() != Some(&expected) {
            report.errors.push(format!("open: {answer:?}"));
        }
    } else {
        let starts_download = n % 4 == 1;
        let answer = shares
            .authorize_content(
                &link.created.token,
                target.grant.as_ref(),
                ip,
                starts_download,
            )
            .await;
        report.contents.push(began.elapsed());
        let file = &link.file;
        match answer {
            Ok(content)
                if (content.account_id, content.node_id, content.revision_id)
                    == (file.account, file.node, file.revision) => {}
            other => report.errors.push(format!("content: {other:?}")),
        }
    }
}

async fn visitor(
    shares: Arc<PgShareService>,
    targets: Arc<Vec<Target>>,
    index: usize,
    first: Instant,
    period: Duration,
    end: Instant,
) -> Report {
    let mut ticker = tokio::time::interval_at(first.into(), period);
    ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
    let mut report = Report::default();
    let mut n = 0;
    loop {
        ticker.tick().await;
        if Instant::now() >= end {
            return report;
        }
        let target = &targets[(index + n) % targets.len()];
        request(&shares, target, n, index, &mut report).await;
        n += 1;
    }
}

async fn run(scale: &Scale) -> (Report, i64) {
    let harness = Harness::new().await;
    let mut factory = LinkFactory::new(&harness, 10, 10, 0x5106_0101).await;
    let shares = Arc::new(harness.shares());
    let mut targets = Vec::new();
    for link in factory.links(&shares, scale.links, 20).await {
        let grant = match link.password {
            Some(password) => Some(
                shares
                    .verify_password(&link.created.token, password, VISITOR)
                    .await
                    .expect("the right password"),
            ),
            None => None,
        };
        targets.push(Target { link, grant });
    }
    let targets = Arc::new(targets);
    let visitors = u32::try_from(scale.visitors).expect("fits");
    let rate = u32::try_from(scale.requests_per_second).expect("fits");
    let period = Duration::from_secs(1) * visitors / rate;
    let start = Instant::now();
    let end = start + scale.duration;
    let tasks: Vec<_> = (0..scale.visitors)
        .map(|index| {
            let step = u32::try_from(index).expect("fits");
            let first = start + scale.ramp_up * step / visitors;
            let (shares, targets) = (Arc::clone(&shares), Arc::clone(&targets));
            tokio::spawn(visitor(shares, targets, index, first, period, end))
        })
        .collect();
    let mut report = Report::default();
    for task in tasks {
        report.merge(task.await.expect("visitor finished"));
    }
    let connections = harness.db.open_connections().await;
    drop((shares, targets));
    harness.finish().await;
    (report, connections)
}

fn print_and_check(label: &str, report: &mut Report, connections: i64) -> (Duration, Duration) {
    let requests = report.opens.len() + report.contents.len();
    let open_p95 = percentile(&mut report.opens, 95);
    let content_p95 = percentile(&mut report.contents, 95);
    println!(
        "PT-06-1 {label} on {}: {requests} requests, {} errors, open p95 {open_p95:?}, \
         authorize_content p95 {content_p95:?}, {connections} DB connections",
        host(),
        report.errors.len()
    );
    assert_eq!(report.errors, Vec::<String>::new(), "error rate 0%");
    assert!(connections <= MAX_CONNECTIONS, "{connections} connections");
    (open_p95, content_p95)
}

/// The functional part, in CI on any host: 0% errors, every answer equal to its link's, at most 12 connections.
/// Its times are printed only; they are `NOT-RUN(HW-REF)` here.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn pt_06_1_functional_open_and_content_mix_has_no_error_and_few_connections() {
    // Arrange
    let scale = &CI;

    // Act
    let (mut report, connections) = run(scale).await;

    // Assert
    assert!(report.opens.len() + report.contents.len() >= scale.requests_per_second);
    print_and_check(
        "functional (timing NOT-RUN(HW-REF))",
        &mut report,
        connections,
    );
}

/// PT-06-1 on REF-SRV only: 1,000 req/s for 3 minutes; `open` p95 at most 10 ms (fail 50 ms),
/// `authorize_content` p95 at most 15 ms (fail 60 ms).
#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
#[ignore = "NOT-RUN(HW-REF): PT-06-1 open and authorize_content times are measured on REF-SRV only"]
async fn pt_06_1_open_and_content_mix_meets_the_p95_targets() {
    // Arrange
    let scale = &REF_SRV;

    // Act
    let (mut report, connections) = run(scale).await;

    // Assert
    let (open_p95, content_p95) = print_and_check("REF-SRV", &mut report, connections);
    assert!(
        open_p95 <= Duration::from_millis(50),
        "fail above 50 ms: {open_p95:?}"
    );
    assert!(
        content_p95 <= Duration::from_millis(60),
        "fail above 60 ms: {content_p95:?}"
    );
    assert!(
        open_p95 <= Duration::from_millis(10),
        "target 10 ms: {open_p95:?}"
    );
    assert!(
        content_p95 <= Duration::from_millis(15),
        "target 15 ms: {content_p95:?}"
    );
}
