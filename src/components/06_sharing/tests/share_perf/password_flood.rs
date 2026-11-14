//! PT-06-3 (AZ-3703 AC-6, AC-20, NFT-SEC-06): attackers flood `verify_password` with wrong passwords on password
//! links while visitors keep calling `open` on plain links. The harness's wrong-password window (500 ms) moves on
//! the manual clock in step with real time, so every link admits its 10 wrong passwords again each window and the
//! Argon2 checks queue for the `share_password_max_concurrency` permits for the whole run. An attacker that is
//! refused waits the retry time it was given. A monitor samples the permits in use; the result line gives the
//! peak, the checks run, the visitors' `open` p95 and the process CPU in cores. The CI run is a small flood with a
//! cheap Argon2 cost and the functional assertions only; REF-SRV runs the production cost.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use contracts::{PublicShareError, ShareOpen};
use sc_clock::ManualClock;
use sunnycloud_sharing::PgShareService;

use crate::harness::Harness;
use crate::link_factory::{FactoryLink, LinkFactory};
use crate::load::{address, host, percentile, process_cpu};

const MAX_CONNECTIONS: i64 = 12;
/// How far the manual clock moves per step, and how long a step is in real time.
const CLOCK_STEP: Duration = Duration::from_millis(10);
const MONITOR_STEP: Duration = Duration::from_micros(100);

struct Scale {
    password_links: usize,
    attackers: usize,
    visitors: usize,
    duration: Duration,
    /// `share_argon2_params`; `None` is the production default.
    argon2: Option<&'static str>,
}

const CI: Scale = Scale {
    password_links: 10,
    attackers: 100,
    visitors: 5,
    duration: Duration::from_secs(2),
    argon2: Some("8192,2,1"),
};

const REF_SRV: Scale = Scale {
    password_links: 100,
    attackers: 1000,
    visitors: 50,
    duration: Duration::from_secs(60),
    argon2: None,
};

#[derive(Default)]
struct Report {
    permits: usize,
    peak_permits: usize,
    checks_run: u64,
    wrong: usize,
    limited: usize,
    attacker_errors: Vec<String>,
    opens: Vec<Duration>,
    visitor_errors: Vec<String>,
    cores: f64,
    connections: i64,
}

async fn attacker(
    shares: Arc<PgShareService>,
    links: Arc<Vec<FactoryLink>>,
    index: usize,
    end: Instant,
) -> (usize, usize, Vec<String>) {
    let link = &links[index % links.len()];
    let (mut wrong, mut limited, mut errors) = (0, 0, Vec::new());
    while Instant::now() < end {
        let guess = format!("flood guess {index}");
        match shares
            .verify_password(&link.created.token, &guess, address(2, index))
            .await
        {
            Err(PublicShareError::WrongPassword) => wrong += 1,
            Err(PublicShareError::RateLimited { retry_after }) => {
                limited += 1;
                tokio::time::sleep(retry_after).await;
            }
            other => errors.push(format!("{other:?}")),
        }
    }
    (wrong, limited, errors)
}

async fn visitor(
    shares: Arc<PgShareService>,
    links: Arc<Vec<FactoryLink>>,
    index: usize,
    end: Instant,
) -> (Vec<Duration>, Vec<String>) {
    let (mut times, mut errors) = (Vec::new(), Vec::new());
    let mut n = index;
    while Instant::now() < end {
        let link = &links[n % links.len()];
        let began = Instant::now();
        let answer = shares.open(&link.created.token, address(3, index)).await;
        times.push(began.elapsed());
        match answer {
            Ok(ShareOpen::Available { name, size, .. })
                if name == link.file.name && size == link.file.size => {}
            other => errors.push(format!("{other:?}")),
        }
        n += 1;
    }
    (times, errors)
}

/// Moves `clock` by `CLOCK_STEP` every `CLOCK_STEP` of real time until `stop`.
async fn drive_clock(clock: Arc<ManualClock>, stop: Arc<AtomicBool>) {
    while !stop.load(Ordering::SeqCst) {
        tokio::time::sleep(CLOCK_STEP).await;
        clock.tick(CLOCK_STEP);
    }
}

async fn watch_permits(shares: Arc<PgShareService>, stop: Arc<AtomicBool>, peak: Arc<AtomicUsize>) {
    while !stop.load(Ordering::SeqCst) {
        peak.fetch_max(shares.password_checks_in_progress(), Ordering::SeqCst);
        tokio::time::sleep(MONITOR_STEP).await;
    }
}

async fn run(scale: &Scale) -> Report {
    let harness = Harness::new().await;
    let mut config = harness.config.clone();
    config.argon2_params = scale.argon2.map(str::to_owned);
    let shares = Arc::new(harness.shares_limited_by(&config));
    let mut factory = LinkFactory::new(&harness, 10, 10, 0x5106_0103).await;
    let protected = Arc::new(factory.links(&shares, scale.password_links, 100).await);
    let plain = Arc::new(factory.links(&shares, 20, 0).await);
    let (stop, peak) = (
        Arc::new(AtomicBool::new(false)),
        Arc::new(AtomicUsize::new(0)),
    );
    let clock = tokio::spawn(drive_clock(Arc::clone(&harness.clock), Arc::clone(&stop)));
    let monitor = tokio::spawn(watch_permits(
        Arc::clone(&shares),
        Arc::clone(&stop),
        Arc::clone(&peak),
    ));
    let (cpu_before, began) = (process_cpu(), Instant::now());
    let end = began + scale.duration;
    let attackers: Vec<_> = (0..scale.attackers)
        .map(|index| {
            tokio::spawn(attacker(
                Arc::clone(&shares),
                Arc::clone(&protected),
                index,
                end,
            ))
        })
        .collect();
    let visitors: Vec<_> = (0..scale.visitors)
        .map(|index| tokio::spawn(visitor(Arc::clone(&shares), Arc::clone(&plain), index, end)))
        .collect();
    let mut report = Report::default();
    for task in visitors {
        let (times, errors) = task.await.expect("visitor finished");
        report.opens.extend(times);
        report.visitor_errors.extend(errors);
    }
    for task in attackers {
        let (wrong, limited, errors) = task.await.expect("attacker finished");
        report.wrong += wrong;
        report.limited += limited;
        report.attacker_errors.extend(errors);
    }
    let cpu = process_cpu().saturating_sub(cpu_before);
    report.cores = cpu.as_secs_f64() / began.elapsed().as_secs_f64();
    stop.store(true, Ordering::SeqCst);
    clock.await.expect("clock driver finished");
    monitor.await.expect("monitor finished");
    report.permits = config.password_concurrency().expect("valid permits");
    report.peak_permits = peak.load(Ordering::SeqCst);
    report.checks_run = shares.password_checks_run();
    report.connections = harness.db.open_connections().await;
    assert_eq!(
        shares.password_checks_in_progress(),
        0,
        "every permit is back"
    );
    drop((shares, protected, plain));
    harness.finish().await;
    report
}

fn print_and_check(label: &str, report: &mut Report) -> Duration {
    let open_p95 = percentile(&mut report.opens, 95);
    println!(
        "PT-06-3 {label} on {}: {} wrong, {} limited, {} Argon2 checks, peak {} of {} permits, {} opens p95 \
         {open_p95:?}, process CPU {:.2} cores, {} DB connections",
        host(),
        report.wrong,
        report.limited,
        report.checks_run,
        report.peak_permits,
        report.permits,
        report.opens.len(),
        report.cores,
        report.connections
    );
    assert_eq!(
        report.attacker_errors,
        Vec::<String>::new(),
        "only WrongPassword or RateLimited"
    );
    assert_eq!(
        report.visitor_errors,
        Vec::<String>::new(),
        "open error rate 0%"
    );
    // Whether the per-link limit also refuses depends on the hash time against the window's refill: hardware.
    assert!(report.wrong > 0, "the flood reached Argon2");
    assert!(report.peak_permits >= 1, "the monitor saw the checks");
    assert!(
        report.peak_permits <= report.permits,
        "{} permits in use, at most {}",
        report.peak_permits,
        report.permits
    );
    assert!(
        report.connections <= MAX_CONNECTIONS,
        "{} connections",
        report.connections
    );
    open_p95
}

/// The functional part, in CI on any host: no unexpected answer, permits within `share_password_max_concurrency`,
/// every visitor `open` answered. Its times and CPU are printed only; they are `NOT-RUN(HW-REF)` here.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn pt_06_3_functional_wrong_password_flood_stays_within_the_permits() {
    // Arrange
    let scale = &CI;

    // Act
    let mut report = run(scale).await;

    // Assert
    assert!(!report.opens.is_empty());
    print_and_check("functional (timing NOT-RUN(HW-REF))", &mut report);
}

/// PT-06-3 on REF-SRV only, production Argon2 cost: `open` p95 during the flood at most 50 ms (fail 200 ms); CPU at
/// most permits x 1 core (fail permits x 1.5). The CPU is the whole process's, so it bounds the hashing from above.
#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
#[ignore = "NOT-RUN(HW-REF): PT-06-3 open time and hashing CPU under a password flood are measured on REF-SRV only"]
async fn pt_06_3_wrong_password_flood_does_not_starve_open() {
    // Arrange
    let scale = &REF_SRV;

    // Act
    let mut report = run(scale).await;

    // Assert
    let open_p95 = print_and_check("REF-SRV", &mut report);
    let permits = report.permits as f64;
    assert!(
        open_p95 <= Duration::from_millis(200),
        "fail above 200 ms: {open_p95:?}"
    );
    assert!(
        report.cores <= permits * 1.5,
        "fail above {} cores: {:.2}",
        permits * 1.5,
        report.cores
    );
    assert!(
        open_p95 <= Duration::from_millis(50),
        "target 50 ms: {open_p95:?}"
    );
    assert!(
        report.cores <= permits,
        "target {permits} cores: {:.2}",
        report.cores
    );
}
