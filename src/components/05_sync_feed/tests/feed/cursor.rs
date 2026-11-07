//! AC-4 (classification table) and AC-5 (cursor corpus) of the pure cursor rules, no database.

use contracts::{ChangeFeedError, Cursor, ResyncReason};
use sunnycloud_sync_feed::{FeedState, judge_cursor, page_limit, parse_cursor};

const FEED: FeedState = FeedState {
    epoch: 7,
    seq: 50,
    purge_horizon: 20,
};

fn cursor(epoch: i64, seq: i64) -> Cursor {
    Cursor { epoch, seq }
}

#[test]
fn each_cursor_of_the_table_is_classified_as_listed() {
    // Arrange
    let table = [
        (cursor(8, 10), Err(ResyncReason::EpochChanged)),
        (cursor(7, 19), Err(ResyncReason::BelowPurgeHorizon)),
        (cursor(7, 20), Ok(())),
        (cursor(7, 50), Ok(())),
        (cursor(7, 51), Err(ResyncReason::CursorAhead)),
        (cursor(7, 9_999_999_999), Err(ResyncReason::CursorAhead)),
        (cursor(8, 9_999_999_999), Err(ResyncReason::EpochChanged)),
        (cursor(6, 0), Err(ResyncReason::EpochChanged)),
        (cursor(7, 0), Err(ResyncReason::BelowPurgeHorizon)),
    ];

    // Act
    let judged: Vec<_> = table
        .iter()
        .map(|(cursor, _)| judge_cursor(*cursor, FEED))
        .collect();

    // Assert
    let expected: Vec<_> = table.iter().map(|(_, expected)| *expected).collect();
    assert_eq!(judged, expected);
}

#[test]
fn every_malformed_cursor_text_is_bad_cursor() {
    // Arrange
    let corpus = [
        "",
        "abc",
        "1.",
        ".5",
        ".",
        "1.2.3",
        "-1.5",
        "1.-5",
        "+1.5",
        "1.5 ",
        " 1.5",
        "1 .5",
        "1_0.5",
        "1,5",
        "1:5",
        "0x1.5",
        "7.1 OR 1=1",
        "99999999999999999999.1",
        "1.99999999999999999999",
        "9223372036854775808.1",
        "\u{0661}.\u{0662}",
    ];

    // Act
    let results: Vec<_> = corpus.iter().map(|text| parse_cursor(text)).collect();

    // Assert
    for (text, result) in corpus.iter().zip(results) {
        assert!(
            matches!(result, Err(ChangeFeedError::BadCursor)),
            "{text:?} gave {result:?}"
        );
    }
}

#[test]
fn well_formed_cursor_texts_parse_to_epoch_and_seq() {
    // Arrange
    let texts = [
        "7.0",
        "0.0",
        "1761000000.42",
        "9223372036854775807.9223372036854775807",
        "007.05",
    ];

    // Act
    let parsed: Vec<_> = texts
        .iter()
        .map(|text| parse_cursor(text).expect(text))
        .collect();

    // Assert
    assert_eq!(
        parsed,
        [
            cursor(7, 0),
            cursor(0, 0),
            cursor(1_761_000_000, 42),
            cursor(i64::MAX, i64::MAX),
            cursor(7, 5)
        ]
    );
}

#[test]
fn page_limit_defaults_caps_and_never_reaches_zero() {
    // Arrange
    let cases = [
        (None, 500, None, 500),
        (Some(100), 500, None, 100),
        (Some(100_000), 500, None, 100_000),
        (Some(100_000), 500, Some(1000), 1000),
        (None, 500, Some(200), 200),
        (Some(0), 500, None, 1),
        (Some(u32::MAX), 500, None, i64::from(u32::MAX)),
    ];

    // Act
    let limits: Vec<_> = cases
        .iter()
        .map(|(requested, default, ceiling, _)| page_limit(*requested, *default, *ceiling))
        .collect();

    // Assert
    let expected: Vec<_> = cases.iter().map(|case| case.3).collect();
    assert_eq!(limits, expected);
}
