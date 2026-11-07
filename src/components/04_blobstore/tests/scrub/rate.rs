//! AC-3: the rate cap holds, and a `Due` run takes only the blobs not scrubbed within `scrub_interval`, the
//! oldest first.

use std::time::Instant;

use contracts::{ByteRate, ScrubMode, Scrubber};
use sc_clock::Clock;
use uuid::Uuid;

use crate::support::{MIB, Rig, run_to_done};

const RATE: u64 = 10 * MIB as u64;

async fn set_scrubbed_ago(rig: &Rig, id: Uuid, ago_secs: u64) {
    let at = rig
        .clock
        .now_utc()
        .checked_sub(std::time::Duration::from_secs(ago_secs))
        .expect("past instant");
    let mut conn = rig.owner().await;
    sqlx::query(
        "UPDATE blobs SET last_scrubbed_at = 'epoch'::timestamptz + $2::bigint * interval '1 microsecond' \
         WHERE id = $1",
    )
    .bind(id)
    .bind(at.as_unix_micros())
    .execute(&mut conn)
    .await
    .expect("set last_scrubbed_at");
    sqlx::Connection::close(conn).await.expect("close");
}

#[tokio::test(flavor = "multi_thread")]
async fn the_measured_read_rate_stays_within_ten_percent_of_the_cap() {
    // Arrange
    let rig = Rig::new().await;
    for seed in 0..12 {
        rig.store(MIB, seed).await;
    }
    let scrub = rig.scrub(4);
    let probe = scrub.scrubber.read_probe();
    let run = scrub
        .scrubber
        .start_run(ScrubMode::Full, Some(ByteRate(RATE)))
        .await
        .expect("start");

    // Act
    let began = Instant::now();
    let (total, _) = run_to_done(scrub.scrubber.as_ref(), &run, 10).await;
    let elapsed = began.elapsed().as_secs_f64();

    // Assert
    let rate = probe.chunk_bytes() as f64 / elapsed;
    assert_eq!(total.checked, 12);
    assert!(
        rate <= 1.1 * RATE as f64,
        "{:.2} MiB/s over {elapsed:.2} s",
        rate / MIB as f64
    );
    drop(scrub);
    rig.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_due_run_skips_recently_scrubbed_blobs_and_takes_the_oldest_first() {
    // Arrange
    let rig = Rig::new().await;
    let mut ids = Vec::new();
    for seed in 0..4 {
        ids.push(rig.store(64 * 1024, seed).await.blob_id);
    }
    let scrub = rig.scrub(1);
    let full = scrub
        .scrubber
        .start_run(ScrubMode::Full, None)
        .await
        .expect("full run");
    let (everything, _) = run_to_done(scrub.scrubber.as_ref(), &full, 10).await;
    scrub.scrubber.finish_run(&full).await.expect("finish full");
    set_scrubbed_ago(&rig, ids[0], 3 * 3600).await;
    set_scrubbed_ago(&rig, ids[1], 2 * 3600).await;
    set_scrubbed_ago(&rig, ids[2], 1800).await;

    // Act
    let due = scrub
        .scrubber
        .start_run(ScrubMode::Due, None)
        .await
        .expect("due run");
    let first = scrub.scrubber.run_step(&due).await.expect("first step");
    let oldest = rig.scrubbed_micros(ids[0]).await.expect("scrubbed");
    let next = rig.scrubbed_micros(ids[1]).await.expect("scrubbed");
    let recent_from = rig.clock.now_utc().as_unix_micros() - 3_600_000_000;
    let (rest, _) = run_to_done(scrub.scrubber.as_ref(), &due, 10).await;

    // Assert
    assert_eq!(everything.checked, 4);
    assert_eq!(first.checked, 1);
    assert!(oldest > recent_from, "the oldest was taken");
    assert!(next < recent_from, "the next one waits");
    assert_eq!(
        rest.checked, 1,
        "the second due blob; the recent two are skipped"
    );
    drop(scrub);
    rig.finish().await;
}
