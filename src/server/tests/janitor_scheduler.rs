//! AC-1 interval, AC-2 failures and panics survived, AC-3 no overlap, and the stop rules of AC-5.

mod janitor_support;

use std::sync::Arc;
use std::time::Duration;

use contracts::Janitor;
use janitor_support::{
    Counting, Erroring, PanicsOnce, Slow, capture_logs, metrics, series, system_clock, wait_until,
};
use sc_clock::{Clock, ManualClock, Timestamp};
use sunnycloud_server::janitor::{JanitorMetrics, Scheduled, Scheduler, SchedulerError};

const START: Timestamp = Timestamp::from_unix_micros(1_794_052_800_000_000);
const INTERVAL: Duration = Duration::from_millis(200);

fn scheduled(janitor: Arc<dyn Janitor>, interval: Duration) -> Scheduled {
    Scheduled { janitor, interval }
}

fn start(
    clock: Arc<dyn Clock>,
    jobs: Vec<Scheduled>,
) -> (Scheduler, Arc<sc_observability::Metrics>) {
    let metrics = metrics();
    let janitor_metrics = Arc::new(JanitorMetrics::new(&metrics).expect("janitor families"));
    let scheduler = Scheduler::start(clock, janitor_metrics, jobs).expect("scheduler starts");
    (scheduler, metrics)
}

#[tokio::test]
async fn cycles_start_one_interval_apart_on_a_manual_clock() {
    // Arrange
    let clock = Arc::new(ManualClock::new(START));
    let counting = Counting::new("counting");
    let (scheduler, _metrics) = start(clock.clone(), vec![scheduled(counting.clone(), INTERVAL)]);

    // Act
    clock.tick(INTERVAL - Duration::from_millis(1));
    tokio::time::sleep(Duration::from_millis(20)).await;
    let before_first_interval = counting.calls();
    for expected in 1..=10 {
        clock.tick(if expected == 1 {
            Duration::from_millis(1)
        } else {
            INTERVAL
        });
        wait_until("the next cycle", || counting.calls() == expected).await;
    }
    scheduler.stop().await;

    // Assert
    assert_eq!(before_first_interval, 0);
    let nows = counting.nows();
    assert_eq!(nows.len(), 10);
    assert_eq!(nows[0], START.checked_add(INTERVAL).expect("in range"));
    for pair in nows.windows(2) {
        assert_eq!(pair[1].checked_duration_since(pair[0]), Some(INTERVAL));
    }
}

#[tokio::test]
async fn cycles_start_200_ms_plus_minus_100_ms_apart_on_the_system_clock() {
    // Arrange
    let counting = Counting::new("counting");
    let (scheduler, _metrics) = start(system_clock(), vec![scheduled(counting.clone(), INTERVAL)]);

    // Act
    wait_until("15 cycles", || counting.calls() >= 15).await;
    scheduler.stop().await;

    // Assert
    for gap in counting.gaps() {
        assert!(
            gap >= INTERVAL - Duration::from_millis(100)
                && gap <= INTERVAL + Duration::from_millis(100),
            "gap {gap:?}"
        );
    }
}

#[tokio::test]
async fn failing_and_panicking_cycles_are_logged_counted_and_survived() {
    // Arrange
    let (logs, _guard) = capture_logs();
    let erroring = Erroring::new("erroring_janitor");
    let panicking = PanicsOnce::new("panicking_janitor");
    let counting = Counting::new("counting_janitor");
    let (scheduler, metrics) = start(
        system_clock(),
        vec![
            scheduled(erroring.clone(), INTERVAL),
            scheduled(panicking.clone(), INTERVAL),
            scheduled(counting.clone(), INTERVAL),
        ],
    );

    // Act
    wait_until("the loops to keep cycling", || {
        erroring.calls() >= 4 && panicking.calls() >= 4 && counting.calls() >= 4
    })
    .await;
    scheduler.stop().await;
    let text = metrics.render().await.expect("metrics render");

    // Assert
    let failed = |task| {
        series(
            &text,
            "sunnycloud_janitor_cycles_total",
            &[("task", task), ("result", "failed")],
        )
    };
    let ok = |task| {
        series(
            &text,
            "sunnycloud_janitor_cycles_total",
            &[("task", task), ("result", "ok")],
        )
    };
    let error_lines = logs.errors_of("erroring_janitor");
    assert_eq!(error_lines.len() as u64, erroring.calls());
    assert!(error_lines.iter().all(|line| {
        line["err"]
            .as_str()
            .is_some_and(|err| err.contains("the owner's store is down"))
    }));
    assert_eq!(failed("erroring_janitor"), Some(erroring.calls() as f64));
    assert_eq!(ok("erroring_janitor"), None);
    let panic_lines = logs.errors_of("panicking_janitor");
    assert_eq!(panic_lines.len(), 1);
    assert!(
        panic_lines[0]["msg"]
            .as_str()
            .expect("msg")
            .contains("panicked")
    );
    assert_eq!(failed("panicking_janitor"), Some(1.0));
    assert_eq!(
        ok("panicking_janitor"),
        Some((panicking.calls() - 1) as f64)
    );
    assert_eq!(ok("counting_janitor"), Some(counting.calls() as f64));
    assert_eq!(failed("counting_janitor"), None);
    for gap in counting.gaps() {
        assert!(
            gap >= INTERVAL - Duration::from_millis(100)
                && gap <= INTERVAL + Duration::from_millis(100),
            "gap {gap:?}"
        );
    }
}

#[tokio::test]
async fn a_cycle_three_times_the_interval_never_overlaps_itself() {
    // Arrange
    let interval = Duration::from_millis(20);
    let slow = Slow::new("slow_janitor", interval * 3);
    let (scheduler, _metrics) = start(system_clock(), vec![scheduled(slow.clone(), interval)]);

    // Act
    wait_until("10 cycles", || slow.finished() >= 10).await;
    scheduler.stop().await;

    // Assert
    assert_eq!(slow.max_in_flight(), 1);
    assert_eq!(slow.started(), slow.finished());
}

#[tokio::test]
async fn no_cycle_starts_after_stop_returns() {
    // Arrange
    let interval = Duration::from_millis(5);
    let counting = Counting::new("counting");
    let (scheduler, _metrics) = start(system_clock(), vec![scheduled(counting.clone(), interval)]);
    wait_until("a few cycles", || counting.calls() >= 3).await;

    // Act
    scheduler.stop().await;
    let at_stop = counting.calls();
    tokio::time::sleep(interval * 10).await;

    // Assert
    assert_eq!(counting.calls(), at_stop);
}

#[tokio::test]
async fn stop_waits_for_the_cycle_that_is_running() {
    // Arrange
    let interval = Duration::from_millis(5);
    let slow = Slow::new("slow_janitor", Duration::from_millis(80));
    let (scheduler, _metrics) = start(system_clock(), vec![scheduled(slow.clone(), interval)]);
    wait_until("a cycle in flight", || slow.started() > slow.finished()).await;

    // Act
    scheduler.stop().await;

    // Assert
    assert_eq!(slow.started(), slow.finished());
}

#[tokio::test]
async fn a_zero_interval_is_refused_before_any_loop_starts() {
    // Arrange
    let counting = Counting::new("counting");
    let metrics = metrics();
    let janitor_metrics = Arc::new(JanitorMetrics::new(&metrics).expect("janitor families"));

    // Act
    let result = Scheduler::start(
        system_clock(),
        janitor_metrics,
        vec![
            scheduled(counting.clone(), INTERVAL),
            scheduled(Erroring::new("zero_interval"), Duration::ZERO),
        ],
    );

    // Assert
    assert!(matches!(
        result,
        Err(SchedulerError::ZeroInterval("zero_interval"))
    ));
    tokio::time::sleep(Duration::from_millis(10)).await;
    assert_eq!(counting.calls(), 0);
}
