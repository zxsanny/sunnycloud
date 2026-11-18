//! PT-07-2 (AZ-3824 AC-2, AC-27 of the product): a large account is deleted at the production purger settings
//! (batch 5,000, cycle budget 5 s, interval 5 s) while 20 other accounts each commit one small file per second through 03's
//! real upload and commit service. The purger runs over the real hooks of 06, 02, 03, 08, 05 and 07a on a pool of
//! its own, paced as the janitor scheduler paces it (a cycle every `account_delete_interval`, start to start),
//! each cycle followed by a cycle of 04's GC on the shared pool (blobs queued by the purge are due at once, Risk
//! R12): the result line gives the purger's and the GC's time apart. The purger's pool holds at most 2
//! connections: a step that needed a third would wait for its own connections until the acquire timeout and fail,
//! so a purge with no failed hook call and no failed finalize used at most 2 (the sqlx pool counts a connection
//! on its way back as in use, so the pool's in-use figure alone reads one high). Before the load the test holds
//! both pool connections at once and notes their server pids; a sampler then reads, every 50 ms as the superuser,
//! the longest open transaction of those pids (the purger's) and of the API role as a whole (with the GC and the
//! commits). The commit times before the request are the other accounts' baseline; those started between the
//! request and the finalize are compared with it. The CI run is a small account at a short interval with the
//! end-state and connection assertions only.

use std::sync::Arc;
use std::time::{Duration, Instant};

use contracts::Janitor;
use sc_clock::{CancellationToken, Clock, SystemClock};
use sc_db::{Db, DbConfig, DbRole};
use sqlx::{Connection, PgConnection};
use sunnycloud_admin::DeletionPurger;
use sunnycloud_blobstore::GcSweeper;
use tokio::time::MissedTickBehavior;
use uuid::Uuid;

use crate::background::{self, Commits};
use crate::load::{host, percentile, report as report_metric};
use crate::pool_repo::pool_pids;
use crate::purge_support::probes::{deleted_audits, files_left, rows_of, state, total};
use crate::purge_support::recording::{
    Log, RecordedAccounts, failed_calls, order_breach, record_hooks,
};
use crate::purge_support::seed::{self, Plan};
use crate::purge_support::world::{HOOK_NAMES, PurgeWorld};
use crate::support::seed as account_seed;

const PRODUCTION_BATCH: u64 = 5000;
const BATCH_LIMIT: u32 = 5000;
const PURGER_CONNECTIONS: u32 = 2;
const SAMPLE_EVERY: Duration = Duration::from_millis(50);
/// The longest open transaction of the purger's pids, of the whole API role, and whether a purger one is open.
const LONGEST_TRANSACTIONS: &str = "SELECT \
     coalesce(max(extract(epoch FROM clock_timestamp() - xact_start)) FILTER (WHERE pid = ANY($1)), 0)::float8, \
     coalesce(max(extract(epoch FROM clock_timestamp() - xact_start)), 0)::float8, \
     count(*) FILTER (WHERE pid = ANY($1)) > 0 \
     FROM pg_stat_activity WHERE datname = current_database() AND usename = 'sunnycloud' \
     AND xact_start IS NOT NULL";

struct Scale {
    files: usize,
    shares: usize,
    /// Sign-in sessions of the account.
    sessions: usize,
    derived: usize,
    /// `account_delete_interval` and `account_delete_cycle_budget`: production 5 s and 5 s on REF-SRV, short
    /// harness values in CI (a cycle spends its whole budget while the tree hook waits for the GC, so the
    /// production budget alone would make the CI run last 10 s or more).
    interval: &'static str,
    budget: &'static str,
    others: usize,
    /// How long the other accounts commit before the deletion request.
    baseline: Duration,
    /// Purger cycles after which the purge counts as stuck.
    max_cycles: usize,
}

const CI: Scale = Scale {
    files: 500,
    shares: 50,
    sessions: 10,
    derived: 50,
    interval: "200ms",
    budget: "300ms",
    others: 20,
    baseline: Duration::from_secs(2),
    max_cycles: 50,
};

const REF_SRV: Scale = Scale {
    files: 100_000,
    shares: 1_000,
    sessions: 100,
    derived: 5_000,
    interval: "5s",
    budget: "5s",
    others: 20,
    baseline: Duration::from_secs(60),
    max_cycles: 400,
};

fn plan(scale: &Scale) -> Plan {
    Plan {
        files: scale.files,
        folders: scale.files / 50,
        retained: scale.files / 25,
        shares: scale.shares,
        feed_rows: scale.files.min(120),
        receipts: 5,
        jobs: 30,
        derived: scale.derived,
    }
}

struct Outcome {
    request_to_finalize: Duration,
    cycles: usize,
    purger_time: Duration,
    gc_time: Duration,
    longest_purger_transaction: Duration,
    longest_transaction: Duration,
    /// Samples that saw a purger transaction open.
    purger_samples: u32,
    /// The largest in-use figure of the purger's pool (capped at 2).
    purger_in_use: u32,
    /// Hook and finalize calls of the purger.
    calls: usize,
    /// Failed hook calls and failed finalizes of the purger.
    failed_steps: usize,
    order_breach: Option<String>,
    baseline: Vec<Duration>,
    during: Vec<Duration>,
    commit_errors: Vec<String>,
    /// `(account, live files, files seeded plus committed)` of each other account whose counts differ.
    others_changed: Vec<(Uuid, i64, i64)>,
    rows_left: i64,
    files_left: usize,
    deleted_audits: i64,
}

/// Purger cycles paced by `account_delete_interval`, each followed by a GC cycle, until the account row is gone;
/// returns the cycles and the time spent in the purger and in the GC.
async fn scheduled_purge(
    world: &PurgeWorld,
    purger: &DeletionPurger,
    sweeper: &GcSweeper,
    account: Uuid,
    max_cycles: usize,
) -> (usize, Duration, Duration) {
    let mut conn = world.fixture().await;
    let mut ticker = tokio::time::interval(world.config.account_delete_interval);
    ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
    let (mut purging, mut collecting) = (Duration::ZERO, Duration::ZERO);
    for cycle in 1..=max_cycles {
        ticker.tick().await;
        let began = Instant::now();
        purger
            .run_cycle(world.clock.now_utc())
            .await
            .expect("purger cycle");
        purging += began.elapsed();
        if state(&mut conn, account).await.is_none() {
            conn.close().await.expect("close");
            return (cycle, purging, collecting);
        }
        let began = Instant::now();
        sweeper
            .run_cycle(world.clock.now_utc())
            .await
            .expect("gc cycle");
        collecting += began.elapsed();
    }
    panic!("the account is not gone after {max_cycles} cycles");
}

/// The purger's own pool, of [`PURGER_CONNECTIONS`] connections.
async fn purger_pool(world: &PurgeWorld) -> Db {
    let config = DbConfig {
        db_pool_max: PURGER_CONNECTIONS,
        ..world.db.config()
    };
    Db::connect(&config, DbRole::Api)
        .await
        .expect("connect the purger's pool")
}

#[derive(Default)]
struct Samples {
    longest_purger: Duration,
    longest: Duration,
    purger_open: u32,
    in_use: u32,
}

/// Samples the open transactions and the purger's pool until `stop`.
async fn sample(
    mut conn: PgConnection,
    purger_db: Db,
    pids: Vec<i32>,
    stop: CancellationToken,
) -> Samples {
    let mut ticker = tokio::time::interval(SAMPLE_EVERY);
    let mut samples = Samples::default();
    loop {
        tokio::select! {
            biased;
            () = stop.cancelled() => break,
            _ = ticker.tick() => {}
        }
        let (purger, any, open): (f64, f64, bool) = sqlx::query_as(LONGEST_TRANSACTIONS)
            .bind(&pids)
            .fetch_one(&mut conn)
            .await
            .expect("read pg_stat_activity");
        samples.longest_purger = samples
            .longest_purger
            .max(Duration::from_secs_f64(purger.max(0.0)));
        samples.longest = samples.longest.max(Duration::from_secs_f64(any.max(0.0)));
        samples.purger_open += u32::from(open);
        samples.in_use = samples.in_use.max(purger_db.stats().pool_in_use);
    }
    conn.close().await.expect("close");
    samples
}

async fn run(scale: &Scale) -> Outcome {
    let clock: Arc<dyn Clock> = Arc::new(SystemClock::new());
    let keys = [
        ("SUNNYCLOUD_ACCOUNT_DELETE_BATCH_SIZE", "5000"),
        ("SUNNYCLOUD_ACCOUNT_DELETE_INTERVAL", scale.interval),
        ("SUNNYCLOUD_ACCOUNT_DELETE_CYCLE_BUDGET", scale.budget),
    ];
    let world = PurgeWorld::with_keys(clock.clone(), &keys).await;
    assert_eq!(world.config.account_delete_batch_size, PRODUCTION_BATCH);
    let shared = world.wiring(world.api.clone()).await;
    let purger_db = purger_pool(&world).await;
    let wiring = world.wiring(purger_db.clone()).await;
    let log = Log::default();
    let purger = world.purger(
        RecordedAccounts::wrap(wiring.accounts.clone(), &log, false),
        record_hooks(wiring.hooks.clone(), &log, None),
        purger_db.clone(),
    );
    let sweeper = world.sweeper(&shared);
    let pids = pool_pids(&purger_db, PURGER_CONNECTIONS).await;
    let target = seed::account(&world, plan(scale)).await;
    let mut conn = world.fixture().await;
    for _ in 1..scale.sessions {
        account_seed::session(&mut conn, target.account).await;
    }
    let others = background::others(&world, scale.others).await;
    let stop_commits = CancellationToken::new();
    let committers = background::start(shared.upload.clone(), clock, &others, &stop_commits);
    // The baseline window of the load: the other accounts commit before the deletion is requested.
    tokio::time::sleep(scale.baseline).await;
    let stop_sampler = CancellationToken::new();
    let sampler = tokio::spawn(sample(
        world.db.admin().await,
        purger_db.clone(),
        pids,
        stop_sampler.clone(),
    ));

    let requested = Instant::now();
    world.request_deletion(target.account).await;
    let (cycles, purger_time, gc_time) =
        scheduled_purge(&world, &purger, &sweeper, target.account, scale.max_cycles).await;
    let request_to_finalize = requested.elapsed();
    let finalized = Instant::now();

    stop_commits.cancel();
    stop_sampler.cancel();
    let samples = sampler.await.expect("sampler");
    let calls = log.calls();
    let mut outcome = Outcome {
        request_to_finalize,
        cycles,
        purger_time,
        gc_time,
        longest_purger_transaction: samples.longest_purger,
        longest_transaction: samples.longest,
        purger_samples: samples.purger_open,
        purger_in_use: samples.in_use,
        calls: calls.len(),
        failed_steps: failed_calls(&calls),
        order_breach: order_breach(&calls, HOOK_NAMES.len(), BATCH_LIMIT),
        baseline: Vec::new(),
        during: Vec::new(),
        commit_errors: Vec::new(),
        others_changed: Vec::new(),
        rows_left: total(&rows_of(&mut conn, target.account).await),
        files_left: files_left(&world, &target.blobs),
        deleted_audits: deleted_audits(&mut conn, target.account).await,
    };
    for (other, committer) in others.iter().zip(committers) {
        let Commits { done, errors } = committer.await.expect("committer finished");
        let live = background::live_files(&mut conn, other.account).await;
        let wanted = 1 + i64::try_from(done.len()).expect("fits");
        if live != wanted {
            outcome.others_changed.push((other.account, live, wanted));
        }
        for (started, took) in done {
            if started < requested {
                outcome.baseline.push(took);
            } else if started < finalized {
                outcome.during.push(took);
            }
        }
        outcome.commit_errors.extend(errors);
    }
    conn.close().await.expect("close");
    drop((purger, sweeper, wiring, shared, purger_db));
    world.finish().await;
    outcome
}

/// Prints the result line and asserts the hardware-independent part; returns the commit p95 before and during
/// the purge (`None` during when no commit started in the purge window).
fn print_and_check(label: &str, outcome: &mut Outcome) -> (Duration, Option<Duration>) {
    let baseline_p95 = percentile(&mut outcome.baseline, 95);
    let during_p95 = (!outcome.during.is_empty()).then(|| percentile(&mut outcome.during, 95));
    println!(
        "PT-07-2 {label} on {}: request to finalize {:?} in {} cycles, {} purger calls (purger {:?}, GC {:?}), longest \
         transaction purger {:?} ({} samples) / API role {:?}, purger pool of {PURGER_CONNECTIONS} (peak in use {}, {} failed steps), other accounts' commits {} before / {} during, p95 {baseline_p95:?} \
         before / {during_p95:?} during, {} commit errors",
        host(),
        outcome.request_to_finalize,
        outcome.cycles,
        outcome.calls,
        outcome.purger_time,
        outcome.gc_time,
        outcome.longest_purger_transaction,
        outcome.purger_samples,
        outcome.longest_transaction,
        outcome.purger_in_use,
        outcome.failed_steps,
        outcome.baseline.len(),
        outcome.during.len(),
        outcome.commit_errors.len(),
    );
    assert_eq!(outcome.rows_left, 0, "rows of the account left");
    assert_eq!(outcome.files_left, 0, "blob files of the account left");
    assert_eq!(outcome.deleted_audits, 1, "one account_deleted audit row");
    assert_eq!(
        outcome.failed_steps, 0,
        "a purger step failed on a pool of {PURGER_CONNECTIONS}"
    );
    assert_eq!(outcome.order_breach, None, "hook order and batch limit");
    assert!(
        outcome.purger_samples > 0,
        "the sampler saw no purger transaction"
    );
    assert_eq!(outcome.commit_errors, Vec::<String>::new(), "commit errors");
    assert_eq!(outcome.others_changed, Vec::new(), "other accounts' files");
    (baseline_p95, during_p95)
}

/// The functional part, in CI on any host: 500 files at the production batch with a 300 ms budget and a 200 ms
/// interval; nothing of the account is left, one audit row, the purger on at most 2 connections, every other
/// account's commit succeeds and its files are all there. Times are printed only (`NOT-RUN(HW-REF)`).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn pt_07_2_functional_large_account_purge_leaves_nothing_while_other_accounts_commit() {
    // Arrange
    let scale = &CI;

    // Act
    let mut outcome = run(scale).await;

    // Assert
    assert!(outcome.baseline.len() >= scale.others, "baseline commits");
    print_and_check("functional (timing NOT-RUN(HW-REF))", &mut outcome);
}

/// PT-07-2 on REF-SRV only: 100,000 files, 1,000 shares, 100 sessions, 5,000 derived rows, the production
/// purger settings (batch 5,000, budget 5 s, interval 5 s); request
/// to finalize at most 5 min (fail 15 min), longest transaction at most 2 s (fail 10 s), the other accounts'
/// commit p95 within 20% of their baseline (fail at 2x).
#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
#[ignore = "NOT-RUN(HW-REF): PT-07-2 deletion time and commit impact are measured on REF-SRV only"]
async fn pt_07_2_large_account_is_deleted_in_time_without_slowing_other_accounts() {
    // Arrange
    let scale = &REF_SRV;

    // Act
    let mut outcome = run(scale).await;

    // Assert
    let (baseline_p95, during_p95) = print_and_check("REF-SRV", &mut outcome);
    let during_p95 = during_p95.expect("commits during the purge");
    let ratio = during_p95.as_secs_f64() / baseline_p95.as_secs_f64();
    let (minutes, seconds) = (|m: u64| Duration::from_secs(60 * m), Duration::from_secs);
    let finalize = outcome.request_to_finalize;
    let longest = outcome.longest_purger_transaction;
    report_metric(
        "PT-07-2",
        "request to finalize",
        finalize,
        minutes(5),
        minutes(15),
    );
    report_metric(
        "PT-07-2",
        "longest purger transaction",
        longest,
        seconds(2),
        seconds(10),
    );
    report_metric("PT-07-2", "commit p95 / baseline", ratio, 1.2, 2.0);
    assert!(finalize <= minutes(15), "fail above 15 min: {finalize:?}");
    assert!(longest <= seconds(10), "fail above 10 s: {longest:?}");
    assert!(ratio <= 2.0, "fail at 2x the baseline: {ratio:.2}");
}
