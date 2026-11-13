//! AZ-3716 AC-1 (batched hash lookup), AC-3 (revision reads), AC-73 (blob paths for doctor) and AC-5 (live usage)
//! on the reader as the API role (IT-02-15, IT-02-17, IT-02-18).

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use contracts::{LiveUsageSource, TreeReader, TreeReaderError};
use sc_ids::{FileHash, Hash32};
use sqlx::Arguments;
use sqlx::postgres::PgArguments;
use sunnycloud_tree::{LOOKUP_REVISIONS_SQL, MAX_LOOKUP_HASHES};
use uuid::Uuid;

use crate::mutation_support::Tree;
use crate::publish_support::{existing, hash_of, new_file};
use crate::reader_support::{publish_file, reader};
use crate::revision_support::{
    DELETED, DEVICE_FILES, REPLACED, dataset, meta_of, plan, publish_all,
};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_hash_batch_matches_current_revisions_of_live_files_only_and_1001_hashes_are_too_many() {
    // Arrange
    let tree = Tree::new().await;
    let data = dataset(&tree).await;
    let live = 0..800;
    let mut hashes: Vec<FileHash> = data.files[live.clone()].iter().map(|f| f.hash).collect();
    hashes.extend(data.files[REPLACED].iter().map(|f| f.hash));
    hashes.extend(data.files[DELETED].iter().map(|f| f.hash));
    let too_many: Vec<FileHash> = (0..=MAX_LOOKUP_HASHES)
        .map(|_| hash_of(Uuid::new_v4()))
        .collect();
    let reader = reader(&tree, 1000);

    // Act
    let matches = reader
        .lookup_revisions(tree.account, &hashes)
        .await
        .expect("lookup");
    let refused = reader.lookup_revisions(tree.account, &too_many).await;

    // Assert
    assert_eq!(hashes.len(), MAX_LOOKUP_HASHES);
    let found: HashMap<Uuid, (FileHash, Uuid, Option<String>)> = matches
        .into_iter()
        .map(|m| (m.revision_id, (m.hash, m.node_id, m.client_meta)))
        .collect();
    let expected: HashMap<Uuid, (FileHash, Uuid, Option<String>)> = live
        .map(|index| {
            let file = data.files[index];
            (file.revision, (file.hash, file.node, meta_of(index)))
        })
        .collect();
    assert_eq!(found, expected);
    assert_eq!(
        found.values().filter(|(_, _, meta)| meta.is_some()).count(),
        DEVICE_FILES + 50
    );
    assert!(matches!(refused, Err(TreeReaderError::BatchTooLarge)));
    drop(reader);
    tree.finish().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_replacement_hash_matches_and_another_accounts_hash_matches_nothing() {
    // Arrange
    let tree = Tree::new().await;
    let data = dataset(&tree).await;
    let (other, other_root) = tree.add_account().await;
    let (_, foreign_revision, _) = publish_file(&tree, other, other_root, "theirs.txt").await;
    let replacement = data.replacements[0];
    let reader = reader(&tree, 1000);

    // Act
    let mine = reader
        .lookup_revisions(tree.account, &[replacement.hash])
        .await
        .expect("lookup");
    let theirs_seen_by_me = reader
        .lookup_revisions(tree.account, &[hash_of(foreign_revision)])
        .await
        .expect("lookup");

    // Assert
    assert_eq!(
        mine.iter()
            .map(|m| (m.node_id, m.revision_id))
            .collect::<Vec<_>>(),
        [(replacement.node, replacement.revision)]
    );
    assert_eq!(theirs_seen_by_me, []);
    drop(reader);
    tree.finish().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_hash_lookup_reads_the_account_hash_index_as_the_api_role() {
    // Arrange
    let tree = Tree::new().await;
    let data = dataset(&tree).await;
    let hashes: Vec<Vec<u8>> = data.files[..10]
        .iter()
        .map(|f| f.hash.as_bytes().to_vec())
        .collect();
    let mut args = PgArguments::default();
    args.add(tree.account).expect("bind account");
    args.add(hashes).expect("bind hashes");

    // Act
    let plan = plan(&tree, LOOKUP_REVISIONS_SQL, args).await;

    // Assert
    assert!(
        plan.contains("revisions_account_id_content_hash_idx"),
        "{plan}"
    );
    assert!(!plan.contains("Seq Scan on revisions"), "{plan}");
    tree.finish().await;
}

#[tokio::test]
async fn revision_reads_answer_for_current_and_retained_and_not_for_collected_or_foreign() {
    // Arrange
    let tree = Tree::new().await;
    let (account, root) = (tree.account, tree.root);
    let (node, retained, _) = publish_file(&tree, account, root, "kept.txt").await;
    let blob = tree.blobs(1).await[0];
    let current = publish_all(&tree, vec![existing(node, Some(retained), blob)]).await[0];
    let (collected_node, collected, _) = publish_file(&tree, account, root, "gone.txt").await;
    let blob = tree.blobs(1).await[0];
    publish_all(&tree, vec![existing(collected_node, Some(collected), blob)]).await;
    tree.exec(&format!("DELETE FROM revisions WHERE id = '{collected}'"))
        .await;
    let (other, other_root) = tree.add_account().await;
    let (_, foreign, _) = publish_file(&tree, other, other_root, "theirs.txt").await;
    let reader = reader(&tree, 1000);

    // Act
    let mut views = Vec::new();
    let mut held = Vec::new();
    for revision in [current.revision, retained, collected, foreign] {
        views.push(reader.get_revision(account, revision).await);
        held.push(
            reader
                .is_current_or_retained(account, revision)
                .await
                .expect("lease check"),
        );
    }

    // Assert
    let blocklist = Some(vec![Hash32::from_bytes([3; 32])]);
    let read = |index: usize| match &views[index] {
        Ok(view) => (
            view.id,
            view.node_id,
            view.content_hash,
            view.blocklist.clone(),
        ),
        Err(error) => panic!("revision {index}: {error:?}"),
    };
    assert_eq!(
        read(0),
        (current.revision, node, current.hash, blocklist.clone())
    );
    assert_eq!(read(1), (retained, node, hash_of(retained), blocklist));
    assert!(matches!(views[2], Err(TreeReaderError::NotFound)));
    assert!(matches!(views[3], Err(TreeReaderError::NotFound)));
    assert_eq!(held, [true, true, false, false]);
    drop(reader);
    tree.finish().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn doctor_gets_full_paths_of_live_blobs_none_for_a_deleted_file_and_nothing_for_a_random_id()
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
    let b = tree
        .service
        .create_folder(account, a, "b")
        .await
        .expect("b")
        .id;
    let c = tree
        .service
        .create_folder(account, b, "c")
        .await
        .expect("c")
        .id;
    let mut live = Vec::new();
    for (parent, name, path) in [
        (root, "top.txt", "top.txt"),
        (a, "one.txt", "a/one.txt"),
        (b, "two.txt", "a/b/two.txt"),
        (c, "three.txt", "a/b/c/three.txt"),
    ] {
        let (_, _, blob) = publish_file(&tree, account, parent, name).await;
        live.push((blob, vec![path.to_owned()]));
    }
    let (other, other_root) = tree.add_account().await;
    let (_, _, foreign_blob) = publish_file(&tree, other, other_root, "Theirs.txt").await;
    live.push((foreign_blob, vec!["Theirs.txt".to_owned()]));
    let (deleted, _, deleted_blob) = publish_file(&tree, account, c, "deleted.txt").await;
    tree.service
        .delete_node(account, deleted, None)
        .await
        .expect("delete");
    let random = Uuid::new_v4();
    let mut ids: Vec<Uuid> = live.iter().map(|(blob, _)| *blob).collect();
    ids.extend([deleted_blob, random]);
    let reader = reader(&tree, 1000);

    // Act
    let described = reader.describe_blobs(&ids).await.expect("describe");

    // Assert
    let mut expected: HashMap<Uuid, Vec<String>> = live.into_iter().collect();
    expected.insert(deleted_blob, Vec::new());
    assert_eq!(described, expected);
    assert!(!described.contains_key(&random));
    drop(reader);
    tree.finish().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn doctor_describes_1000_nested_blobs_within_200_ms() {
    // Arrange
    let tree = Tree::new().await;
    let mut parent = tree.root;
    for depth in 0..5 {
        parent = tree
            .service
            .create_folder(tree.account, parent, &format!("level {depth}"))
            .await
            .expect("folder")
            .id;
    }
    let blobs = tree.blobs(1000).await;
    let entries = blobs
        .iter()
        .enumerate()
        .map(|(index, blob)| new_file(parent, &format!("f{index:04}"), *blob))
        .collect();
    publish_all(&tree, entries).await;
    tree.exec("ANALYZE nodes").await;
    tree.exec("ANALYZE revisions").await;
    let reader = reader(&tree, 1000);
    reader
        .describe_blobs(&blobs[..1])
        .await
        .expect("warm the pool");

    // Act
    let started = Instant::now();
    let described = reader.describe_blobs(&blobs).await.expect("describe");
    let elapsed = started.elapsed();

    // Assert
    assert_eq!(described.len(), 1000);
    let paths: HashSet<&str> = described.values().flatten().map(String::as_str).collect();
    assert_eq!(paths.len(), 1000);
    assert!(paths.contains("level 0/level 1/level 2/level 3/level 4/f0999"));
    assert!(elapsed < Duration::from_millis(200), "{elapsed:?}");
    drop(reader);
    tree.finish().await;
}

#[tokio::test]
async fn live_bytes_sums_the_current_revisions_of_live_files() {
    // Arrange
    let tree = Tree::new().await;
    let blobs = tree.blobs(4).await;
    let entries = [(100, "kept"), (250, "replaced"), (400, "deleted")]
        .into_iter()
        .zip(&blobs)
        .map(|((size, name), blob)| {
            let mut entry = new_file(tree.root, name, *blob);
            entry.revision.size = size;
            entry
        })
        .collect();
    let files = publish_all(&tree, entries).await;
    let mut replacement = existing(files[1].node, Some(files[1].revision), blobs[3]);
    replacement.revision.size = 700;
    publish_all(&tree, vec![replacement]).await;
    tree.service
        .delete_node(tree.account, files[2].node, None)
        .await
        .expect("delete");
    let folder = tree
        .service
        .create_folder(tree.account, tree.root, "empty")
        .await
        .expect("folder");
    let (other, other_root) = tree.add_account().await;
    publish_file(&tree, other, other_root, "theirs.txt").await;
    let reader = reader(&tree, 1000);

    // Act
    let mine = reader.live_bytes(tree.account).await.expect("live bytes");
    let random = reader.live_bytes(Uuid::new_v4()).await.expect("live bytes");

    // Assert
    assert_eq!(folder.size, None);
    assert_eq!(mine, 100 + 700);
    assert_eq!(random, 0);
    drop(reader);
    tree.finish().await;
}
