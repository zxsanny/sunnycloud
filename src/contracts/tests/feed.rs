//! Recording doubles of the 05 sync feed ports. `FeedStamp` and `FeedHorizon` take a `&mut DbTx`, so their
//! doubles compile here and run in the component tests that own a database.

mod common;

use std::sync::Arc;
use std::time::Duration;

use common::{Calls, NOW, id, principal, sorted};
use contracts::{
    ChangeFeed, ChangeFeedError, ChangeItem, ChangePage, Cursor, EpochAdmin, EpochAdminError,
    FeedHorizon, FeedHorizonError, FeedNotifier, FeedStamp, FeedStampError, HeadReport, Marker,
    MarkerStatus, NodeKind, NodeState, Principal, PrincipalRevalidator, ResyncReason,
    RevalidatorError, Seq, TouchedNodes, async_trait,
};
use sc_db::DbTx;
use uuid::Uuid;

#[derive(Default)]
struct Double(Calls);

#[async_trait]
impl FeedStamp for Double {
    async fn stamp(
        &self,
        _tx: &mut DbTx,
        _account: Uuid,
        touched: TouchedNodes,
    ) -> Result<Seq, FeedStampError> {
        self.0.record("stamp");
        i64::try_from(touched.0.len()).map_err(|e| FeedStampError::Unexpected(Box::new(e)))
    }
}

#[async_trait]
impl FeedHorizon for Double {
    async fn raise_purge_horizon(
        &self,
        _tx: &mut DbTx,
        _account: Uuid,
        _seq: Seq,
    ) -> Result<(), FeedHorizonError> {
        self.0.record("raise_purge_horizon");
        Ok(())
    }
}

impl FeedNotifier for Double {
    fn notify(&self, _account: Uuid, _seq: Seq) {
        self.0.record("notify");
    }
}

#[async_trait]
impl ChangeFeed for Double {
    async fn read(
        &self,
        _principal: &Principal,
        revalidator: &dyn PrincipalRevalidator,
        cursor: Cursor,
        _wait: Option<Duration>,
        _limit: Option<u32>,
    ) -> Result<ChangePage, ChangeFeedError> {
        self.0.record("read");
        let valid = revalidator
            .revalidate(&principal())
            .await
            .map_err(|RevalidatorError::Unexpected(fault)| ChangeFeedError::Unexpected(fault))?;
        if !valid {
            return Err(ChangeFeedError::Unauthenticated);
        }
        if cursor.seq > 100 {
            return Err(ChangeFeedError::ResyncRequired {
                reason: ResyncReason::CursorAhead,
            });
        }
        Ok(ChangePage {
            items: vec![ChangeItem {
                node_id: id(),
                parent_id: None,
                kind: NodeKind::Folder,
                name: String::new(),
                state: NodeState::Live,
                revision_id: None,
                size: None,
                content_hash: None,
                seq: cursor.seq + 1,
            }],
            next_cursor: Cursor {
                epoch: cursor.epoch,
                seq: cursor.seq + 1,
            },
            has_more: false,
        })
    }

    async fn head(&self, _account: Uuid) -> Result<Cursor, ChangeFeedError> {
        self.0.record("head");
        Ok(Cursor { epoch: 1, seq: 0 })
    }
}

#[async_trait]
impl EpochAdmin for Double {
    async fn bump_epoch(&self, _now: sc_clock::Timestamp) -> Result<i64, EpochAdminError> {
        self.0.record("bump_epoch");
        Ok(2)
    }

    async fn marker_status(&self) -> Result<MarkerStatus, EpochAdminError> {
        self.0.record("marker_status");
        Ok(MarkerStatus {
            db_epoch: 2,
            marker: Marker::Present(1),
        })
    }

    async fn head_report(&self) -> Result<HeadReport, EpochAdminError> {
        self.0.record("head_report");
        Ok(HeadReport {
            max_epoch: 2,
            heads: vec![(id(), Cursor { epoch: 2, seq: 0 })],
        })
    }
}

struct Revalidator(bool);

#[async_trait]
impl PrincipalRevalidator for Revalidator {
    async fn revalidate(&self, _principal: &Principal) -> Result<bool, RevalidatorError> {
        Ok(self.0)
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn change_feed_double_reads_a_page_and_the_head_across_two_threads() {
    let calls = Calls::default();
    let feed: Arc<dyn ChangeFeed> = Arc::new(Double(calls.clone()));
    let reader = feed.clone();

    let page = tokio::spawn(async move {
        reader
            .read(
                &principal(),
                &Revalidator(true),
                Cursor { epoch: 1, seq: 4 },
                Some(Duration::from_secs(1)),
                Some(10),
            )
            .await
    });
    let head = feed.head(id()).await.expect("head");
    let page = page.await.expect("task").expect("read");

    assert_eq!(head, Cursor { epoch: 1, seq: 0 });
    assert_eq!(page.next_cursor, Cursor { epoch: 1, seq: 5 });
    assert!(!page.has_more);
    assert_eq!(calls.sorted(), sorted(vec!["read", "head"]));
}

#[tokio::test]
async fn a_refused_principal_ends_the_read_as_unauthenticated() {
    let feed = Double::default();

    let result = feed
        .read(
            &principal(),
            &Revalidator(false),
            Cursor { epoch: 1, seq: 0 },
            None,
            None,
        )
        .await;

    assert!(matches!(result, Err(ChangeFeedError::Unauthenticated)));
}

#[tokio::test]
async fn a_cursor_ahead_of_the_head_asks_for_a_resync_with_its_reason() {
    let feed = Double::default();

    let result = feed
        .read(
            &principal(),
            &Revalidator(true),
            Cursor { epoch: 1, seq: 101 },
            None,
            None,
        )
        .await;

    assert!(matches!(
        result,
        Err(ChangeFeedError::ResyncRequired {
            reason: ResyncReason::CursorAhead
        })
    ));
}

#[tokio::test]
async fn epoch_admin_double_records_each_method() {
    let calls = Calls::default();
    let admin: Arc<dyn EpochAdmin> = Arc::new(Double(calls.clone()));

    let epoch = admin.bump_epoch(NOW).await.expect("bump");
    let status = admin.marker_status().await.expect("status");
    let report = admin.head_report().await.expect("report");

    assert_eq!(epoch, 2);
    assert_eq!(status.marker, Marker::Present(1));
    assert_eq!(report.heads.len(), 1);
    assert_eq!(
        calls.sorted(),
        sorted(vec!["bump_epoch", "marker_status", "head_report"])
    );
}

#[test]
fn notifier_is_synchronous_and_usable_as_arc_dyn() {
    let calls = Calls::default();
    let notifier: Arc<dyn FeedNotifier> = Arc::new(Double(calls.clone()));

    notifier.notify(id(), 3);

    assert_eq!(calls.sorted(), vec!["notify"]);
}

/// Compiles only while the stamp and horizon ports are object safe. Never called.
#[allow(dead_code)]
fn tx_ports_are_object_safe(_stamp: Arc<dyn FeedStamp>, _horizon: Arc<dyn FeedHorizon>) {}
