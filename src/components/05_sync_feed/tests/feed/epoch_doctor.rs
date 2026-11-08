//! AC-6 (IT-05-14): the levels of the epoch doctor check, from the real database epoch and a marker double.
//! Whether the volume holds blobs is an input of the judgement; its source is not specified (flagged concern).

use contracts::{EpochAdmin, Finding, FindingLevel, Marker, MarkerStatus};
use sunnycloud_sync_feed::epoch_findings;

use crate::epoch_support::Epochs;
use crate::marker_double::MarkerDouble;

fn levels(findings: &[Finding]) -> Vec<FindingLevel> {
    findings.iter().map(|finding| finding.level).collect()
}

#[test]
fn the_level_table_is_fail_fail_ok_ok() {
    // Arrange
    let cases = [
        (Marker::Absent, 9, true, FindingLevel::Fail),
        (Marker::Present(5), 9, true, FindingLevel::Fail),
        (Marker::Present(9), 9, true, FindingLevel::Ok),
        (Marker::Absent, 0, false, FindingLevel::Ok),
    ];

    // Act
    let judged: Vec<_> = cases
        .iter()
        .map(|(marker, db_epoch, holds_blobs, _)| {
            epoch_findings(
                MarkerStatus {
                    db_epoch: *db_epoch,
                    marker: *marker,
                },
                *holds_blobs,
            )
        })
        .collect();

    // Assert
    for (findings, (.., expected)) in judged.iter().zip(cases) {
        assert_eq!(levels(findings), vec![expected]);
    }
}

#[test]
fn only_the_failures_name_the_epoch_bump_remedy() {
    // Arrange
    let failing = MarkerStatus {
        db_epoch: 9,
        marker: Marker::Present(5),
    };
    let matching = MarkerStatus {
        db_epoch: 9,
        marker: Marker::Present(9),
    };

    // Act
    let failed = epoch_findings(failing, false);
    let passed = epoch_findings(matching, false);

    // Assert
    assert!(failed[0].remedy.contains("admin epoch-bump"));
    assert!(passed[0].remedy.is_empty());
}

#[tokio::test]
async fn the_four_marker_states_of_a_real_database_give_fail_fail_ok_ok() {
    // Arrange
    let absent = Epochs::new(MarkerDouble::default()).await;
    absent.accounts(&[9]).await;
    let lagging = Epochs::new(MarkerDouble::holding(5)).await;
    lagging.accounts(&[9]).await;
    let equal = Epochs::new(MarkerDouble::holding(9)).await;
    equal.accounts(&[9]).await;
    let empty = Epochs::new(MarkerDouble::default()).await;

    // Act
    let results = [
        epoch_findings(absent.admin.marker_status().await.expect("status"), true),
        epoch_findings(lagging.admin.marker_status().await.expect("status"), true),
        epoch_findings(equal.admin.marker_status().await.expect("status"), true),
        epoch_findings(empty.admin.marker_status().await.expect("status"), false),
    ];

    // Assert
    let levels: Vec<_> = results.iter().map(|findings| levels(findings)[0]).collect();
    assert_eq!(
        levels,
        [
            FindingLevel::Fail,
            FindingLevel::Fail,
            FindingLevel::Ok,
            FindingLevel::Ok
        ]
    );
    for epochs in [absent, lagging, equal, empty] {
        epochs.finish().await;
    }
}
