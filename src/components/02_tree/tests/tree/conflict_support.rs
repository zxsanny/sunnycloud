//! Helpers of the conflict-outcome tests (AZ-3710): one entry published as a named device, and the node and
//! revision columns the outcomes change, read as the superuser.

use contracts::{
    CommitActor, EntryResult, PublishEntry, PublishOutcome, PublishResult, TreeCommit,
};
use sqlx::Row;
use uuid::Uuid;

use crate::mutation_support::{Tree, close};
use crate::publish_support::{TestEffect, effects};

/// The one entry result of a call.
#[derive(Debug, Clone, Copy)]
pub struct Done {
    pub outcome: PublishOutcome,
    pub node: Uuid,
    pub revision: Uuid,
    pub conflict_copy: Option<Uuid>,
}

pub fn done(result: PublishResult) -> Done {
    let PublishResult::Published(results) = result else {
        panic!("unexpected replay");
    };
    match results.as_slice() {
        [
            EntryResult::Published {
                outcome,
                node_id,
                revision_id,
                conflict_copy,
            },
        ] => Done {
            outcome: *outcome,
            node: *node_id,
            revision: *revision_id,
            conflict_copy: *conflict_copy,
        },
        other => panic!("expected one published entry, got {other:?}"),
    }
}

pub async fn publish_as(tree: &Tree, device: &str, entry: PublishEntry) -> Done {
    let actor = CommitActor {
        account_id: tree.account,
        device_label: device.to_owned(),
    };
    let result = tree
        .service
        .publish(
            actor,
            vec![entry],
            effects(&TestEffect::recording()),
            sc_testdb::START,
        )
        .await
        .expect("publish");
    done(result)
}

/// The columns of one node row that the conflict outcomes change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeRow {
    pub parent: Option<Uuid>,
    pub name: String,
    pub live: bool,
    pub cascade_pending: bool,
    pub current: Option<Uuid>,
    pub seq: i64,
}

impl Tree {
    pub async fn row(&self, id: Uuid) -> NodeRow {
        let mut conn = self.fixture.db.admin().await;
        let row = sqlx::query(
            "SELECT parent_id, name, deleted_at IS NULL, cascade_pending, current_revision_id, seq \
             FROM nodes WHERE id = $1",
        )
        .bind(id)
        .fetch_one(&mut conn)
        .await
        .expect("read node");
        close(conn).await;
        NodeRow {
            parent: row.get(0),
            name: row.get(1),
            live: row.get(2),
            cascade_pending: row.get(3),
            current: row.get(4),
            seq: row.get(5),
        }
    }

    /// `(node_id, blob_id)` of a revision row.
    pub async fn revision_row(&self, id: Uuid) -> (Uuid, Uuid) {
        let mut conn = self.fixture.db.admin().await;
        let row = sqlx::query("SELECT node_id, blob_id FROM revisions WHERE id = $1")
            .bind(id)
            .fetch_one(&mut conn)
            .await
            .expect("read revision");
        close(conn).await;
        (row.get(0), row.get(1))
    }
}
