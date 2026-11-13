//! AZ-3716 AC-4 (path resolution, ST-02-3 part, with the owner decision of 2026-11-13 on a tombstoned last part)
//! and AC-2 (revisions of one device, paged, IT-02-18) on `TreeService` as the API role.

use std::collections::HashSet;
use std::time::Duration;

use contracts::{NodeState, Page, TreeService, TreeServiceError};
use sqlx::Arguments;
use sqlx::postgres::PgArguments;
use sunnycloud_tree::{DEVICE_REVISIONS_SQL, MAX_REVISION_PAGE};
use uuid::Uuid;

use crate::mutation_support::Tree;
use crate::reader_support::publish_file;
use crate::revision_support::{DEVICE, DEVICE_FILES, dataset, plan};

#[tokio::test]
async fn a_path_resolves_to_its_live_node_and_traversal_missing_empty_and_foreign_paths_are_not_found()
 {
    // Arrange
    let tree = Tree::new().await;
    let (account, root) = (tree.account, tree.root);
    let a = tree
        .service
        .create_folder(account, root, "a")
        .await
        .expect("a")
        .id;
    let (file, _, _) = publish_file(&tree, account, a, "b.txt").await;
    let (other, _) = tree.add_account().await;
    let service = tree.service.as_ref();

    // Act
    let found = service.resolve_path(account, "a/b.txt").await;
    let folded = service.resolve_path(account, "A/B.TXT").await;
    let folder = service.resolve_path(account, "a").await;
    let mut refused = Vec::new();
    for path in [
        "a/missing.txt",
        "../x",
        "a//b",
        "a/./b.txt",
        "/a/b.txt",
        "a/b.txt/",
        "",
        "a/b.txt/x",
    ] {
        refused.push(service.resolve_path(account, path).await);
    }
    let foreign = service.resolve_path(other, "a/b.txt").await;

    // Assert
    assert_eq!(found.expect("a/b.txt"), file);
    assert_eq!(folded.expect("A/B.TXT"), file);
    assert_eq!(folder.expect("a"), a);
    for (index, result) in refused.into_iter().enumerate() {
        assert!(
            matches!(result, Err(TreeServiceError::NotFound)),
            "path {index}: {result:?}"
        );
    }
    assert!(matches!(foreign, Err(TreeServiceError::NotFound)));
    tree.finish().await;
}

#[tokio::test]
async fn a_tombstoned_last_part_is_not_found_for_ordinary_callers_and_found_with_its_state_for_restore()
 {
    // Arrange
    let clock = sc_testdb::clock_at_start();
    let tree = Tree::with_clock(clock.clone()).await;
    let (account, root) = (tree.account, tree.root);
    let a = tree
        .service
        .create_folder(account, root, "a")
        .await
        .expect("a")
        .id;
    let (older, _, _) = publish_file(&tree, account, a, "b.txt").await;
    tree.service
        .delete_node(account, older, None)
        .await
        .expect("delete the first b.txt");
    clock.tick(Duration::from_secs(1));
    let (newer, _, _) = publish_file(&tree, account, a, "b.txt").await;
    let service = tree.service.as_ref();
    let live = service
        .resolve_path_with_state(account, "a/b.txt")
        .await
        .expect("live b.txt");
    tree.service
        .delete_node(account, newer, None)
        .await
        .expect("delete the second b.txt");

    // Act
    let ordinary = service.resolve_path(account, "a/b.txt").await;
    let for_restore = service
        .resolve_path_with_state(account, "a/b.txt")
        .await
        .expect("tombstoned b.txt");
    tree.service
        .delete_node(account, a, None)
        .await
        .expect("delete a");
    let under_deleted_folder = service.resolve_path_with_state(account, "a/b.txt").await;
    let deleted_folder = service.resolve_path_with_state(account, "a").await;

    // Assert
    assert_eq!(live, (newer, NodeState::Live));
    assert!(matches!(ordinary, Err(TreeServiceError::NotFound)));
    assert_eq!(for_restore, (newer, NodeState::Tombstone));
    assert!(matches!(
        under_deleted_folder,
        Err(TreeServiceError::NotFound)
    ));
    assert_eq!(deleted_folder.expect("a"), (a, NodeState::Tombstone));
    tree.finish().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_revisions_of_one_device_page_to_the_end_without_duplicates_and_a_garbage_offset_is_refused()
 {
    // Arrange
    let tree = Tree::new().await;
    let data = dataset(&tree).await;
    let (other, other_root) = tree.add_account().await;
    publish_file(&tree, other, other_root, "theirs.txt").await;
    let service = tree.service.as_ref();
    let limit = 64;

    // Act
    let mut pages = Vec::new();
    loop {
        let offset = u64::try_from(pages.len() * limit as usize).expect("offset");
        let page = service
            .find_revisions(tree.account, Some(DEVICE), Page { offset, limit })
            .await
            .expect("page");
        let last = page.len() < limit as usize;
        pages.push(page);
        if last {
            break;
        }
    }
    let garbage = service
        .find_revisions(
            tree.account,
            Some(DEVICE),
            Page {
                offset: u64::MAX,
                limit,
            },
        )
        .await;
    let foreign = service
        .find_revisions(
            other,
            Some(DEVICE),
            Page {
                offset: 0,
                limit: 0,
            },
        )
        .await
        .expect("foreign");
    let unknown_device = service
        .find_revisions(
            tree.account,
            Some("toaster"),
            Page {
                offset: 0,
                limit: 0,
            },
        )
        .await
        .expect("unknown device");

    // Assert
    let refs: Vec<(Uuid, Uuid)> = pages
        .iter()
        .flatten()
        .map(|r| (r.revision_id, r.node_id))
        .collect();
    let unique: HashSet<(Uuid, Uuid)> = refs.iter().copied().collect();
    let expected: HashSet<(Uuid, Uuid)> = data.files[..DEVICE_FILES]
        .iter()
        .map(|f| (f.revision, f.node))
        .collect();
    assert_eq!(refs.len(), DEVICE_FILES);
    assert_eq!(unique, expected);
    assert_eq!(pages.len(), DEVICE_FILES.div_ceil(limit as usize));
    assert!(matches!(garbage, Err(TreeServiceError::InvalidCursor)));
    assert_eq!(foreign, []);
    assert_eq!(unknown_device, []);
    tree.finish().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn without_a_device_every_revision_is_listed_and_a_page_is_cut_to_the_maximum() {
    // Arrange
    let tree = Tree::new().await;
    dataset(&tree).await;
    let total = tree
        .count(&format!(
            "SELECT count(*) FROM revisions WHERE account_id = '{}'",
            tree.account
        ))
        .await;
    let service = tree.service.as_ref();

    // Act
    let first = service
        .find_revisions(
            tree.account,
            None,
            Page {
                offset: 0,
                limit: u32::MAX,
            },
        )
        .await
        .expect("first page");
    let rest = service
        .find_revisions(
            tree.account,
            None,
            Page {
                offset: u64::from(MAX_REVISION_PAGE),
                limit: 0,
            },
        )
        .await
        .expect("rest");

    // Assert
    assert_eq!(first.len(), MAX_REVISION_PAGE as usize);
    assert_eq!(i64::try_from(first.len() + rest.len()).expect("len"), total);
    tree.finish().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_device_filter_reads_the_client_device_index_as_the_api_role() {
    // Arrange
    let tree = Tree::new().await;
    dataset(&tree).await;
    let mut args = PgArguments::default();
    args.add(tree.account).expect("bind account");
    args.add(DEVICE).expect("bind device");
    args.add(0_i64).expect("bind offset");
    args.add(64_i64).expect("bind limit");

    // Act
    let plan = plan(&tree, DEVICE_REVISIONS_SQL, args).await;

    // Assert
    assert!(plan.contains("revisions_client_device_idx"), "{plan}");
    assert!(!plan.contains("Seq Scan"), "{plan}");
    tree.finish().await;
}
