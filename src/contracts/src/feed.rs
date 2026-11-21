//! Ports of 05 sync feed. `stamp` is the last statement of a tree-changing transaction; `notify` runs only
//! after COMMIT.

use std::time::Duration;

use async_trait::async_trait;
use sc_clock::Timestamp;
use sc_db::DbTx;
use thiserror::Error;
use uuid::Uuid;

use crate::common::Fault;
use crate::identity::{Principal, PrincipalRevalidator};
use crate::media::ThumbState;
use crate::tree::{NodeKind, NodeState};
use sc_ids::FileHash;

/// Per-account feed counter.
pub type Seq = i64;

/// The nodes one transaction changed. Never empty: with nothing touched there is no stamp.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TouchedNodes(pub Vec<Uuid>);

#[derive(Debug, Error)]
pub enum FeedStampError {
    #[error("account feed missing")]
    AccountFeedMissing,
    #[error("unexpected failure")]
    Unexpected(#[source] Fault),
}

#[async_trait]
pub trait FeedStamp: Send + Sync {
    /// Bumps the account counter and stamps the touched nodes with the new `seq`.
    async fn stamp(
        &self,
        tx: &mut DbTx,
        account: Uuid,
        touched: TouchedNodes,
    ) -> Result<Seq, FeedStampError>;
}

/// Wakes held reads of an account. Non-blocking; a lost wake is healed by the reader's periodic recheck.
pub trait FeedNotifier: Send + Sync {
    fn notify(&self, account: Uuid, seq: Seq);
}

/// A preview that reached a final state. Derived outputs are keyed by account and content hash, not by node.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ThumbEvent {
    pub account: Uuid,
    pub content_hash: FileHash,
    /// `Ready` or `Failed`; the other states are never announced.
    pub state: ThumbState,
}

/// What a [`ThumbSubscription`] hands out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThumbWake {
    Event(ThumbEvent),
    /// The subscriber was too slow and events were dropped; it must treat its picture of previews as stale.
    Lagged,
}

/// The wake of the realtime channel for previews, beside `FeedNotifier` for the tree: 08 announces a preview that
/// is ready (its job closed with the thumb stored) or failed (its job died), after the commit that made it so;
/// 11's sockets subscribe and push `ThumbState` frames. `notify` never blocks; an event is lost only to a
/// subscriber that lags, which is told so.
pub trait ThumbNotifier: Send + Sync {
    fn notify(&self, event: ThumbEvent);

    /// Sees every event announced after this call returns.
    fn subscribe(&self) -> Box<dyn ThumbSubscription>;
}

#[async_trait]
pub trait ThumbSubscription: Send {
    /// The next event or lag notice; never completes once the notifier is gone.
    async fn next(&mut self) -> ThumbWake;
}

#[derive(Debug, Error)]
pub enum FeedHorizonError {
    #[error("unexpected failure")]
    Unexpected(#[source] Fault),
}

#[async_trait]
pub trait FeedHorizon: Send + Sync {
    /// Zero affected rows is not an error.
    async fn raise_purge_horizon(
        &self,
        tx: &mut DbTx,
        account: Uuid,
        seq: Seq,
    ) -> Result<(), FeedHorizonError>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cursor {
    pub epoch: i64,
    pub seq: Seq,
}

/// The latest state of one node, not an event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangeItem {
    pub node_id: Uuid,
    pub parent_id: Option<Uuid>,
    pub kind: NodeKind,
    pub name: String,
    pub state: NodeState,
    pub revision_id: Option<Uuid>,
    pub size: Option<i64>,
    pub content_hash: Option<FileHash>,
    /// The sniffed media type of the current revision (files only). Read by the realtime channel; `GET /changes`
    /// does not carry it.
    pub media_type: Option<String>,
    pub seq: Seq,
}

/// A page ends on a `seq` boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangePage {
    pub items: Vec<ChangeItem>,
    pub next_cursor: Cursor,
    pub has_more: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResyncReason {
    EpochChanged,
    BelowPurgeHorizon,
    CursorAhead,
}

#[derive(Debug, Error)]
pub enum ChangeFeedError {
    #[error("resync required")]
    ResyncRequired { reason: ResyncReason },
    #[error("unauthenticated")]
    Unauthenticated,
    #[error("bad cursor")]
    BadCursor,
    /// The principal already holds `max_holds_per_principal` long-polls; the mapper answers it as `rate_limited`
    /// with a retry delay.
    #[error("too many holds")]
    TooManyHolds { retry_after: Duration },
    #[error("unexpected failure")]
    Unexpected(#[source] Fault),
}

/// The shutdown signal of the held reads is given at construction (by 12), not per call.
#[async_trait]
pub trait ChangeFeed: Send + Sync {
    /// Holds up to `wait` when nothing is newer than `cursor`; ends the hold when `revalidator` refuses the
    /// principal.
    async fn read(
        &self,
        principal: &Principal,
        revalidator: &dyn PrincipalRevalidator,
        cursor: Cursor,
        wait: Option<Duration>,
        limit: Option<u32>,
    ) -> Result<ChangePage, ChangeFeedError>;

    async fn head(&self, account: Uuid) -> Result<Cursor, ChangeFeedError>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Marker {
    Absent,
    Present(i64),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MarkerStatus {
    pub db_epoch: i64,
    pub marker: Marker,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeadReport {
    pub max_epoch: i64,
    pub heads: Vec<(Uuid, Cursor)>,
}

#[derive(Debug, Error)]
pub enum EpochAdminError {
    /// Includes a failed marker write after the epoch was committed.
    #[error("unexpected failure")]
    Unexpected(#[source] Fault),
}

#[async_trait]
pub trait EpochAdmin: Send + Sync {
    async fn bump_epoch(&self, now: Timestamp) -> Result<i64, EpochAdminError>;

    async fn marker_status(&self) -> Result<MarkerStatus, EpochAdminError>;

    async fn head_report(&self) -> Result<HeadReport, EpochAdminError>;
}
