//! AZ-3709 AC-1 (IT-02-5: `Created`, `Updated`, `Replaced`, `NoChange` and the entry the effects receive) and
//! AC-10 (the guard refuses before anything is written).

use contracts::{
    EntryResult, PublishOutcome, PublishResult, PublishedEntry, TreeCommit, TreeCommitError,
};
use uuid::Uuid;

use crate::mutation_support::Tree;
use crate::publish_support::{Call, SIZE, TestEffect, actor, effects, existing, hash_of, new_file};

fn published(result: PublishResult) -> Vec<EntryResult> {
    match result {
        PublishResult::Published(results) => results,
        PublishResult::Replayed(_) => panic!("unexpected replay"),
    }
}

fn only(results: &[EntryResult]) -> (PublishOutcome, Uuid, Uuid) {
    match results {
        [
            EntryResult::Published {
                outcome,
                node_id,
                revision_id,
                conflict_copy: None,
            },
        ] => (*outcome, *node_id, *revision_id),
        other => panic!("expected one published entry, got {other:?}"),
    }
}

async fn current_revision(tree: &Tree, node: Uuid) -> Uuid {
    let found = tree
        .count(&format!(
            "SELECT count(*) FROM nodes WHERE id = '{node}' AND kind = 'file'"
        ))
        .await;
    assert_eq!(found, 1, "file node {node}");
    let mut conn = tree.fixture.db.admin().await;
    let current: Uuid = sqlx::query_scalar("SELECT current_revision_id FROM nodes WHERE id = $1")
        .bind(node)
        .fetch_one(&mut conn)
        .await
        .expect("read current revision");
    crate::mutation_support::close(conn).await;
    current
}

#[tokio::test]
async fn a_new_file_is_created_with_its_revision_usage_release_and_one_stamp() {
    // Arrange
    let tree = Tree::new().await;
    let blob = tree.blobs(1).await[0];
    let mut entry = new_file(tree.root, "a.txt", blob);
    entry.revision.reserved_bytes = 40;
    tree.set_reserved(40).await;
    let effect = TestEffect::recording();
    let wakes = tree.notifier.subscribe(tree.account);

    // Act
    let result = tree
        .service
        .publish(
            actor(tree.account),
            vec![entry.clone()],
            effects(&effect),
            sc_testdb::START,
        )
        .await
        .expect("publish");

    // Assert
    let (outcome, node, revision) = only(&published(result));
    assert_eq!(outcome, PublishOutcome::Created);
    assert_eq!(revision, entry.revision.revision_id);
    assert_eq!(current_revision(&tree, node).await, revision);
    assert_eq!(tree.revisions().await, 1);
    assert_eq!(
        tree.quota().await,
        (SIZE, 0),
        "used +size, reservation released"
    );
    assert_eq!(tree.seq().await, 1);
    assert_eq!(
        tree.count(&format!("SELECT seq FROM nodes WHERE id = '{node}'"))
            .await,
        1
    );
    assert_eq!(wakes.notified_seq(), 1, "notified after COMMIT");
    drop(wakes);
    tree.finish().await;
}

#[tokio::test]
async fn an_edit_on_the_current_base_is_updated_and_one_without_a_base_is_replaced() {
    // Arrange
    let tree = Tree::new().await;
    let blobs = tree.blobs(3).await;
    let effect = TestEffect::recording();
    let created = tree
        .service
        .publish(
            actor(tree.account),
            vec![new_file(tree.root, "a.txt", blobs[0])],
            effects(&effect),
            sc_testdb::START,
        )
        .await
        .expect("create");
    let (_, node, first) = only(&published(created));

    // Act
    let updated = tree
        .service
        .publish(
            actor(tree.account),
            vec![existing(node, Some(first), blobs[1])],
            effects(&effect),
            sc_testdb::START,
        )
        .await
        .expect("update");
    let (updated_outcome, updated_node, second) = only(&published(updated));
    let replaced = tree
        .service
        .publish(
            actor(tree.account),
            vec![existing(node, None, blobs[2])],
            effects(&effect),
            sc_testdb::START,
        )
        .await
        .expect("replace");
    let (replaced_outcome, replaced_node, third) = only(&published(replaced));

    // Assert
    assert_eq!(updated_outcome, PublishOutcome::Updated);
    assert_eq!(replaced_outcome, PublishOutcome::Replaced);
    assert_eq!((updated_node, replaced_node), (node, node), "same node id");
    assert_eq!(current_revision(&tree, node).await, third);
    assert!(second != first && third != second);
    assert_eq!(tree.revisions().await, 3, "old revision rows kept");
    assert_eq!(tree.seq().await, 3);
    tree.finish().await;
}

#[tokio::test]
async fn the_same_content_again_is_no_change_with_no_stamp_or_usage_and_on_batch_still_runs() {
    // Arrange
    let tree = Tree::new().await;
    let blobs = tree.blobs(2).await;
    let first = new_file(tree.root, "a.txt", blobs[0]);
    let (_, node, current) = only(&published(
        tree.service
            .publish(
                actor(tree.account),
                vec![first.clone()],
                effects(&TestEffect::recording()),
                sc_testdb::START,
            )
            .await
            .expect("create"),
    ));
    let mut again = existing(node, Some(Uuid::new_v4()), blobs[1]);
    again.revision.content_hash = first.revision.content_hash;
    let effect = TestEffect::recording();
    let usage = tree.quota().await;
    let wakes = tree.notifier.subscribe(tree.account);

    // Act
    let result = tree
        .service
        .publish(
            actor(tree.account),
            vec![again],
            effects(&effect),
            sc_testdb::START,
        )
        .await
        .expect("publish the same content");

    // Assert
    let results = published(result);
    assert_eq!(
        only(&results),
        (
            PublishOutcome::NoChange {
                existing_revision: current
            },
            node,
            current
        )
    );
    assert_eq!(tree.seq().await, 1, "no stamp");
    assert_eq!(wakes.notified_seq(), 0, "no notify");
    assert_eq!(tree.quota().await, usage, "no usage change");
    assert_eq!(tree.revisions().await, 1);
    let calls = effect.calls();
    assert_eq!(calls.len(), 3, "{calls:?}");
    assert!(matches!(&calls[2], Call::Batch(outcomes) if outcomes.len() == 1));
    drop(wakes);
    tree.finish().await;
}

#[tokio::test]
async fn the_effect_receives_node_revision_hash_media_type_size_import_hint_and_vault() {
    // Arrange
    let tree = Tree::new().await;
    let blob = tree.blobs(1).await[0];
    let mut entry = new_file(tree.root, "photo.jpg", blob);
    entry.vault = true;
    entry.revision.import_hint = true;
    entry.revision.media_type = Some("image/jpeg".to_owned());
    entry.revision.size = 4321;
    let effect = TestEffect::recording();

    // Act
    let result = tree
        .service
        .publish(
            actor(tree.account),
            vec![entry.clone()],
            effects(&effect),
            sc_testdb::START,
        )
        .await
        .expect("publish");

    // Assert
    let (_, node, revision) = only(&published(result));
    let expected = PublishedEntry {
        node_id: node,
        revision_id: revision,
        content_hash: hash_of(entry.revision.revision_id),
        media_type: Some("image/jpeg".to_owned()),
        size: 4321,
        import_hint: true,
        vault: true,
    };
    assert_eq!(effect.calls()[1], Call::Entry(expected));
    tree.finish().await;
}

#[tokio::test]
async fn an_account_that_is_not_active_is_refused_whole_with_nothing_written() {
    // Arrange
    let tree = Tree::new().await;
    let blob = tree.blobs(1).await[0];
    let mut entry = new_file(tree.root, "a.txt", blob);
    entry.revision.reserved_bytes = 10;
    tree.set_reserved(10).await;
    tree.exec(&format!(
        "UPDATE accounts SET state = 'deleting' WHERE id = '{}'",
        tree.account
    ))
    .await;
    let effect = TestEffect::recording();
    let nodes = tree.nodes().await;

    // Act
    let result = tree
        .service
        .publish(
            actor(tree.account),
            vec![entry],
            effects(&effect),
            sc_testdb::START,
        )
        .await;

    // Assert
    assert!(matches!(result, Err(TreeCommitError::AccountNotActive)));
    assert_eq!(tree.nodes().await, nodes, "0 node rows");
    assert_eq!(tree.revisions().await, 0, "0 revision rows");
    assert_eq!(tree.quota().await, (0, 10), "no quota change");
    assert_eq!(tree.seq().await, 0, "no seq consumed");
    assert!(effect.calls().is_empty(), "no effect ran");
    assert_eq!(tree.guard.calls(), [true], "the guard ran first");
    tree.finish().await;
}
