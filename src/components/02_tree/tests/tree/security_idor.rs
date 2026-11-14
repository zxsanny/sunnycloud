//! AZ-3719 AC-1 (ST-02-1, AC-59): account B, empty, calls every `TreeService` and `TreeReader` operation and
//! `publish` with account A's ids, hashes and devices. Each answer is the one B gets for random ids (`NotFound`,
//! `ParentNotFound`, `Gone` or empty), none succeeds, and A's rows are unchanged.

use contracts::{
    EntryResult, NodeSelector, Page, PublishEntry, PublishResult, TreeCommit, TreeReader,
    TreeService,
};
use sc_ids::FileHash;
use uuid::Uuid;

use crate::mutation_support::Tree;
use crate::publish_support::{actor, existing, hash_of, new_file};
use crate::reader_support::reader;
use crate::revision_support::device_meta;

const DEVICE: &str = "laptop";

/// The ids of A that B probes, or random ids of the same shape.
struct Targets {
    folder: Uuid,
    file: Uuid,
    deleted_file: Uuid,
    current_revision: Uuid,
    replaced_revision: Uuid,
    hashes: Vec<FileHash>,
}

impl Targets {
    fn random() -> Targets {
        Targets {
            folder: Uuid::new_v4(),
            file: Uuid::new_v4(),
            deleted_file: Uuid::new_v4(),
            current_revision: Uuid::new_v4(),
            replaced_revision: Uuid::new_v4(),
            hashes: vec![hash_of(Uuid::new_v4()), hash_of(Uuid::new_v4())],
        }
    }
}

/// A's tree: `docs/` with `sub/`, `a.txt` replaced once (r1, then r2 from `laptop`) and a deleted `old.txt`.
async fn rich_tree(tree: &Tree) -> Targets {
    let account = tree.account;
    let folder = tree
        .service
        .create_folder(account, tree.root, "docs")
        .await
        .expect("docs")
        .id;
    tree.service
        .create_folder(account, folder, "sub")
        .await
        .expect("sub");
    let blobs = tree.blobs(3).await;
    let first = new_file(folder, "a.txt", blobs[0]);
    let first_hash = first.revision.content_hash;
    let (file, _) = publish(tree, account, first).await;
    let (deleted_file, _) = publish(tree, account, new_file(folder, "old.txt", blobs[1])).await;
    let replaced_revision = revision_of(tree, file).await;
    let mut edit = existing(file, Some(replaced_revision), blobs[2]);
    edit.revision.client_meta = Some(device_meta(DEVICE));
    let edit_hash = edit.revision.content_hash;
    let (_, current_revision) = publish(tree, account, edit).await;
    tree.service
        .delete_node(account, deleted_file, None)
        .await
        .expect("delete old.txt");
    Targets {
        folder,
        file,
        deleted_file,
        current_revision,
        replaced_revision,
        hashes: vec![first_hash, edit_hash],
    }
}

async fn revision_of(tree: &Tree, node: Uuid) -> Uuid {
    let mut admin = tree.fixture.db.admin().await;
    let revision = sqlx::query_scalar("SELECT current_revision_id FROM nodes WHERE id = $1")
        .bind(node)
        .fetch_one(&mut admin)
        .await
        .expect("current revision");
    crate::mutation_support::close(admin).await;
    revision
}

/// One published entry: its node and revision.
async fn publish(tree: &Tree, account: Uuid, entry: PublishEntry) -> (Uuid, Uuid) {
    match tree
        .service
        .publish(actor(account), vec![entry], Vec::new(), sc_testdb::START)
        .await
        .expect("publish")
    {
        PublishResult::Published(results) => match results.as_slice() {
            [
                EntryResult::Published {
                    node_id,
                    revision_id,
                    ..
                },
            ] => (*node_id, *revision_id),
            other => panic!("expected one published entry, got {other:?}"),
        },
        PublishResult::Replayed(_) => panic!("unexpected replay"),
    }
}

/// Every answer B gets for `targets`, as text, in a fixed order. `own` is a folder of B that B tries to move.
async fn answers(tree: &Tree, b: Uuid, own: Uuid, blob: Uuid, t: &Targets) -> Vec<String> {
    let reader = reader(tree, 1000);
    let service = &tree.service;
    let page = Page {
        offset: 0,
        limit: 0,
    };
    let mut out = vec![
        format!("{:?}", reader.get_node(b, NodeSelector::Id(t.file)).await),
        format!("{:?}", reader.get_node(b, NodeSelector::Id(t.folder)).await),
        format!("{:?}", reader.list_children(b, t.folder, None, 0).await),
        format!("{:?}", reader.find_live_node(b, t.folder, "a.txt").await),
        format!("{:?}", reader.resolve_live_file(b, t.file).await),
        format!("{:?}", reader.resolve_live_file(b, t.deleted_file).await),
        format!("{:?}", reader.get_revision(b, t.current_revision).await),
        format!("{:?}", reader.get_revision(b, t.replaced_revision).await),
        format!(
            "{:?}",
            reader.is_current_or_retained(b, t.current_revision).await
        ),
        format!(
            "{:?}",
            reader.is_current_or_retained(b, t.replaced_revision).await
        ),
        format!("{:?}", reader.lookup_revisions(b, &t.hashes).await),
        format!("{:?}", service.find_revisions(b, Some(DEVICE), page).await),
        format!("{:?}", service.find_revisions(b, None, page).await),
        format!("{:?}", service.create_folder(b, t.folder, "x").await),
        format!(
            "{:?}",
            service.update_node(b, t.file, Some("y.txt"), None).await
        ),
        format!(
            "{:?}",
            service.update_node(b, t.folder, None, Some(own)).await
        ),
        format!(
            "{:?}",
            service.update_node(b, own, None, Some(t.folder)).await
        ),
        format!("{:?}", service.delete_node(b, t.file, None).await),
        format!("{:?}", service.delete_node(b, t.folder, None).await),
    ];
    let entries = vec![
        existing(t.file, Some(t.current_revision), blob),
        existing(t.file, None, blob),
        new_file(t.folder, "z.txt", blob),
    ];
    let published = service
        .publish(actor(b), entries, Vec::new(), sc_testdb::START)
        .await;
    out.push(match published {
        Ok(PublishResult::Published(results)) => format!("{results:?}"),
        other => format!("{other:?}"),
    });
    drop(reader);
    out
}

#[tokio::test]
async fn another_account_gets_for_every_operation_exactly_what_random_ids_give_and_changes_nothing()
{
    // Arrange
    let tree = Tree::new().await;
    let targets = rich_tree(&tree).await;
    let (b, b_root) = tree.add_account().await;
    let own = tree
        .service
        .create_folder(b, b_root, "mine")
        .await
        .expect("B's folder")
        .id;
    let blob = tree.blobs_of(b, 1, "committed").await[0];
    let a_before = tree.rows_of(tree.account).await;

    // Act
    let foreign = answers(&tree, b, own, blob, &targets).await;
    let random = answers(&tree, b, own, blob, &Targets::random()).await;

    // Assert
    assert_eq!(foreign, random);
    let expected = [
        "Err(NotFound)",
        "Err(NotFound)",
        "Err(NotFound)",
        "Ok(None)",
        "Ok(Gone)",
        "Ok(Gone)",
        "Err(NotFound)",
        "Err(NotFound)",
        "Ok(false)",
        "Ok(false)",
        "Ok([])",
        "Ok([])",
        "Ok([])",
        "Err(NotFound)",
        "Err(NotFound)",
        "Err(NotFound)",
        "Err(NotFound)",
        "Err(NotFound)",
        "Err(NotFound)",
        "[Failed(NotFound), Failed(NotFound), Failed(ParentNotFound)]",
    ];
    assert_eq!(foreign, expected);
    assert_eq!(tree.rows_of(tree.account).await, a_before);
    let own_view = reader(&tree, 1000);
    let a_matches = own_view
        .lookup_revisions(tree.account, &targets.hashes)
        .await
        .expect("A's own lookup");
    let a_device = tree
        .service
        .find_revisions(
            tree.account,
            Some(DEVICE),
            Page {
                offset: 0,
                limit: 0,
            },
        )
        .await
        .expect("A's own device query");
    assert!(!a_matches.is_empty(), "the probed hashes are A's");
    assert_eq!(a_device.len(), 1, "the probed device is A's");
    drop(own_view);
    tree.finish().await;
}
