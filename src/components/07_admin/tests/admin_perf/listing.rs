//! PT-07-1 (AZ-3824 AC-1, AC-1 of the product): over the DS-29 volume of 500 accounts, 4 admin callers page
//! `AdminService::list_users` 20 per page at 20 calls/s in total, each cycling through the 25 pages, over the real
//! 01 (`IdentityAccounts`) and 07a (`PgQuotaAdmin`). Every page is checked against the seeded accounts; the result
//! line gives p95, p99, the call count and the database connections: the most sessions of the API role seen
//! active or in a transaction (sampled every 10 ms as the superuser), and the sessions the pool opened (the sqlx
//! pool counts a connection on its way back as in use, so it can open one more than ever ran at once). A separate test counts the statements of
//! each page from sqlx's statement events (`sc_testdb::statements`, the shared capture of the same events 07a's
//! `views_statement_repo` counts): the scope and transaction statements of `sc_db` are left out, so a page is two
//! data statements, the accounts page and the usage of its ids. The PT-07-1 targets are proposed (07 sets none)
//! and are replaced by the first REF-SRV baseline.

use std::sync::Arc;
use std::time::{Duration, Instant};

use contracts::{AccountStatus, Actor, AdminService, AdminUserView, Page, Role};
use sc_clock::CancellationToken;
use sc_testdb::statements::StatementLog;
use sqlx::{Connection, PgConnection};
use tokio::time::MissedTickBehavior;
use uuid::Uuid;

use crate::load::{host, percentile, report as report_metric};
use crate::support::harness::{START, admin_actor};
use crate::support::seed;
use crate::support::world::World;

/// DS-29: 500 registered accounts.
const ACCOUNTS: usize = 500;
const PAGE: u32 = 20;
const CALLERS: usize = 4;
const CALLS_PER_SECOND: u32 = 20;
const MAX_CONNECTIONS: i64 = 4;
const SAMPLE_EVERY: Duration = Duration::from_millis(10);
const ACTIVE_SESSIONS: &str = "SELECT count(*) FROM pg_stat_activity WHERE datname = current_database() \
     AND usename = 'sunnycloud' AND state <> 'idle'";
const STATEMENTS_PER_CALL: usize = 2;

struct Scale {
    duration: Duration,
}

const CI: Scale = Scale {
    duration: Duration::from_secs(3),
};

const REF_SRV: Scale = Scale {
    duration: Duration::from_secs(120),
};

/// 500 accounts, oldest first: the first two admins, every tenth disabled, every seventh with a limit; each with
/// its ledger row. Returns the views `list_users` must answer, in listing order.
async fn seed_500(conn: &mut PgConnection) -> Vec<AdminUserView> {
    let mut expected = Vec::with_capacity(ACCOUNTS);
    for position in 0..ACCOUNTS {
        let id = Uuid::new_v4();
        let (role, state) = match position {
            0 | 1 => ("admin", "active"),
            p if p % 10 == 9 => ("user", "disabled"),
            _ => ("user", "active"),
        };
        let created = START.as_unix_micros() + i64::try_from(position).expect("fits");
        seed::account(
            conn,
            id,
            &format!("Person {position}"),
            role,
            state,
            created,
        )
        .await;
        let used = i64::try_from(position).expect("fits") * 1000;
        let limit = (position % 7 == 0).then_some(1 << 30);
        seed::quota(conn, id, limit, used, 0).await;
        expected.push(AdminUserView {
            account: id,
            display_name: format!("Person {position}"),
            role: if role == "admin" {
                Role::Admin
            } else {
                Role::User
            },
            state: if state == "disabled" {
                AccountStatus::Disabled
            } else {
                AccountStatus::Active
            },
            last_login: None,
            used,
            limit,
        });
    }
    expected
}

async fn seeded_world() -> (World, Arc<Vec<AdminUserView>>) {
    let world = World::new().await;
    let mut conn = world.fixture().await;
    let expected = seed_500(&mut conn).await;
    conn.close().await.expect("close the fixture connection");
    (world, Arc::new(expected))
}

fn page_at(index: usize) -> Page {
    Page {
        offset: u64::try_from(index * PAGE as usize).expect("fits"),
        limit: PAGE,
    }
}

#[derive(Default)]
struct Report {
    times: Vec<Duration>,
    errors: Vec<String>,
}

/// One admin caller: a page every `period`, starting at its own page and moving on one page per call.
async fn caller(
    world: Arc<World>,
    expected: Arc<Vec<AdminUserView>>,
    index: usize,
    period: Duration,
    end: Instant,
) -> Report {
    let actor: Actor = admin_actor(Uuid::new_v4());
    let pages = ACCOUNTS / PAGE as usize;
    let mut ticker = tokio::time::interval(period);
    ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
    let mut report = Report::default();
    let mut n = index;
    loop {
        ticker.tick().await;
        if Instant::now() >= end {
            return report;
        }
        let page = n % pages;
        let began = Instant::now();
        let answer = world.admin.list_users(&actor, page_at(page)).await;
        report.times.push(began.elapsed());
        let start = page * PAGE as usize;
        let wanted = &expected[start..start + PAGE as usize];
        match answer {
            Ok(views) if views == wanted => {}
            other => report.errors.push(format!("page {page}: {other:?}")),
        }
        n += 1;
    }
}

/// The most API-role sessions active or in a transaction at once until `stop`.
async fn peak_active(mut conn: PgConnection, stop: CancellationToken) -> i64 {
    let mut ticker = tokio::time::interval(SAMPLE_EVERY);
    let mut peak = 0;
    loop {
        tokio::select! {
            biased;
            () = stop.cancelled() => break,
            _ = ticker.tick() => {}
        }
        let active: i64 = sqlx::query_scalar(ACTIVE_SESSIONS)
            .fetch_one(&mut conn)
            .await
            .expect("read pg_stat_activity");
        peak = peak.max(active);
    }
    conn.close().await.expect("close");
    peak
}

/// The report, the peak of active sessions and the sessions open at the end.
async fn run(scale: &Scale) -> (Report, i64, i64) {
    let (world, expected) = seeded_world().await;
    let world = Arc::new(world);
    let stop = CancellationToken::new();
    let sampler = tokio::spawn(peak_active(world.db.admin().await, stop.clone()));
    let period = Duration::from_secs(1) * u32::try_from(CALLERS).expect("fits") / CALLS_PER_SECOND;
    let end = Instant::now() + scale.duration;
    let tasks: Vec<_> = (0..CALLERS)
        .map(|index| {
            let (world, expected) = (Arc::clone(&world), Arc::clone(&expected));
            tokio::spawn(caller(world, expected, index, period, end))
        })
        .collect();
    let mut report = Report::default();
    for task in tasks {
        let part = task.await.expect("caller finished");
        report.times.extend(part.times);
        report.errors.extend(part.errors);
    }
    stop.cancel();
    let active = sampler.await.expect("sampler");
    let opened = world.db.open_connections().await;
    let world = Arc::into_inner(world).expect("the callers dropped their handles");
    world.finish().await;
    (report, active, opened)
}

fn print_and_check(
    label: &str,
    report: &mut Report,
    active: i64,
    opened: i64,
) -> (Duration, Duration) {
    let calls = report.times.len();
    let p95 = percentile(&mut report.times, 95);
    let p99 = percentile(&mut report.times, 99);
    println!(
        "PT-07-1 {label} on {}: {calls} list_users calls over {ACCOUNTS} accounts, {} errors, p95 {p95:?}, \
         p99 {p99:?}, at most {active} DB connections active ({opened} opened) (targets proposed; the first REF-SRV baseline replaces them)",
        host(),
        report.errors.len()
    );
    assert_eq!(report.errors, Vec::<String>::new(), "error rate 0%");
    assert!(
        active <= MAX_CONNECTIONS,
        "{active} connections active, at most {MAX_CONNECTIONS}"
    );
    (p95, p99)
}

/// The functional part, in CI on any host: 0% errors, every page equal to the seeded accounts, at most 4
/// connections. Its times are printed only; they are `NOT-RUN(HW-REF)` here.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn pt_07_1_functional_four_callers_page_500_accounts_with_no_error_and_few_connections() {
    // Arrange
    let scale = &CI;
    let calls_per_run = scale.duration.as_secs() * u64::from(CALLS_PER_SECOND);

    // Act
    let (mut report, active, opened) = run(scale).await;

    // Assert
    assert!(
        report.times.len() as u64 >= calls_per_run * 9 / 10,
        "{} calls",
        report.times.len()
    );
    print_and_check(
        "functional (timing NOT-RUN(HW-REF))",
        &mut report,
        active,
        opened,
    );
}

/// Each of the 25 pages is two data statements: one accounts page (01) and one usage lookup of its ids (07a).
#[tokio::test]
async fn pt_07_1_each_page_of_20_runs_two_data_statements() {
    // Arrange
    let (world, _) = seeded_world().await;
    let actor = admin_actor(Uuid::new_v4());
    let log = StatementLog::install();
    let mut per_call = Vec::new();

    // Act
    for index in 0..ACCOUNTS / PAGE as usize {
        let page = world
            .admin
            .list_users(&actor, page_at(index))
            .await
            .expect("list a page");
        per_call.push((page.len(), log.take_data_statements()));
    }

    // Assert
    let wrong: Vec<_> = per_call
        .iter()
        .enumerate()
        .filter(|(_, (rows, statements))| {
            *rows != PAGE as usize || statements.len() != STATEMENTS_PER_CALL
        })
        .collect();
    assert!(wrong.is_empty(), "{wrong:#?}");
    world.finish().await;
}

/// PT-07-1 on REF-SRV only: 20 calls/s for 2 minutes; p95 at most 30 ms (fail 100 ms), p99 at most 60 ms (fail
/// 200 ms), proposed targets until the first baseline replaces them.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "NOT-RUN(HW-REF): PT-07-1 list_users times are measured on REF-SRV only"]
async fn pt_07_1_four_callers_page_500_accounts_within_the_p95_and_p99_targets() {
    // Arrange
    let scale = &REF_SRV;

    // Act
    let (mut report, active, opened) = run(scale).await;

    // Assert
    let (p95, p99) = print_and_check("REF-SRV", &mut report, active, opened);
    let ms = Duration::from_millis;
    report_metric("PT-07-1", "p95", p95, ms(30), ms(100));
    report_metric("PT-07-1", "p99", p99, ms(60), ms(200));
    assert!(p95 <= ms(100), "fail above 100 ms: {p95:?}");
    assert!(p99 <= ms(200), "fail above 200 ms: {p99:?}");
}
