//! AC-4 (ST-05-4): cursor text is data. The feed port takes a parsed `Cursor`, so the one place text meets SQL is
//! `parse_cursor`: a hostile string is `BadCursor` there and no statement is built from it. The database
//! checks show `account_feeds` and `nodes` unchanged and present afterwards.

use contracts::ChangeFeedError;
use sqlx::Connection;
use sunnycloud_sync_feed::parse_cursor;

use crate::hold_support::{Rig, config, principal};
use crate::read_support::EPOCH;

async fn snapshot(rig: &Rig) -> Vec<String> {
    let mut conn = rig.fixture.owner_conn().await;
    let rows = sqlx::query_scalar(
        "SELECT 'feed ' || account_id || ' ' || epoch || ' ' || seq || ' ' || purge_horizon \
         FROM account_feeds \
         UNION ALL SELECT 'node ' || id || ' ' || seq FROM nodes ORDER BY 1",
    )
    .fetch_all(&mut conn)
    .await
    .expect("snapshot the tables");
    conn.close().await.expect("close owner connection");
    rows
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn injection_strings_are_bad_cursor_and_the_tables_are_unchanged() {
    // Arrange
    let rig = Rig::new(&config(2_000, 10_000), 4).await;
    let account = rig.account().await;
    rig.commit(&account).await;
    let me = principal(account.id);
    let hostile = [
        format!("{EPOCH}.1; DROP TABLE account_feeds"),
        format!("{EPOCH}.1 OR 1=1"),
        format!("{EPOCH}.1'; DELETE FROM nodes; --"),
        "\u{0661}.\u{0662}".to_owned(),
        "\u{ff11}.\u{ff12}".to_owned(),
        format!("{EPOCH}.{}", "9".repeat(10 * 1024)),
        "A".repeat(10 * 1024),
    ];
    let before = snapshot(&rig).await;

    // Act
    let parsed: Vec<_> = hostile.iter().map(|text| parse_cursor(text)).collect();
    let still_reads = rig.feed_read_now(&me, crate::hold_support::cursor(0)).await;

    // Assert
    for (text, result) in hostile.iter().zip(parsed) {
        assert!(
            matches!(result, Err(ChangeFeedError::BadCursor)),
            "{:?} gave {result:?}",
            text.chars().take(40).collect::<String>()
        );
    }
    assert_eq!(snapshot(&rig).await, before);
    assert_eq!(still_reads.expect("the table still answers").items.len(), 1);
    rig.finish().await;
}
