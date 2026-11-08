//! AC-4: the missing-blob inventory pages through the committed rows and reports the ones without a file.

use std::fs;

use contracts::{BlobInventory as _, InventoryError};
use sunnycloud_blobstore::{BlobInventory, blob_path};
use uuid::Uuid;

use crate::support::Rig;

const COMMITTED: usize = 50;
const MISSING: usize = 7;
const UPLOADING: usize = 2;

pub struct Volume {
    pub missing: Vec<Uuid>,
    pub present: Vec<Uuid>,
    pub uploading: Vec<Uuid>,
}

/// Rows written as the owner and files that are only present or absent: the inventory looks at the name.
pub async fn volume_with_gaps(rig: &Rig) -> Volume {
    let mut ids: Vec<Uuid> = (0..COMMITTED).map(|_| Uuid::new_v4()).collect();
    ids.sort();
    let uploading: Vec<Uuid> = (0..UPLOADING).map(|_| Uuid::new_v4()).collect();
    insert(rig, &ids, "committed").await;
    insert(rig, &uploading, "uploading").await;
    let missing: Vec<Uuid> = ids.iter().copied().step_by(7).take(MISSING).collect();
    for id in &ids {
        if !missing.contains(id) {
            put_file(rig, *id);
        }
    }
    let present = ids.into_iter().filter(|id| !missing.contains(id)).collect();
    Volume {
        missing,
        present,
        uploading,
    }
}

pub fn put_file(rig: &Rig, id: Uuid) {
    let path = blob_path(&rig.volume, id);
    fs::create_dir_all(path.parent().expect("blob folder")).expect("folders");
    fs::write(path, b"x").expect("blob file");
}

async fn insert(rig: &Rig, ids: &[Uuid], state: &str) {
    for id in ids {
        rig.db
            .admin_exec(&format!(
                "INSERT INTO blobs (id, account_id, state, size, wrapped_key, master_key_id, created_at) \
                 VALUES ('{id}', '{}', '{state}', {}, 'k', 1, 'epoch'::timestamptz)",
                rig.account,
                if state == "committed" { "10" } else { "NULL" }
            ))
            .await;
    }
}

pub fn inventory(rig: &Rig) -> BlobInventory {
    BlobInventory::new(&rig.config, rig.api.clone()).expect("inventory")
}

#[tokio::test(flavor = "multi_thread")]
async fn repeated_calls_cover_every_committed_row_and_return_exactly_the_missing_ids() {
    // Arrange
    let rig = Rig::new().await;
    let volume = volume_with_gaps(&rig).await;
    let inventory = inventory(&rig);

    // Act
    let mut found = Vec::new();
    let mut calls = 0;
    for _ in 0..COMMITTED.div_ceil(20) {
        calls += 1;
        found.extend(inventory.find_missing(20).await.expect("page"));
    }
    let wrapped = inventory.find_missing(20).await.expect("first page again");

    // Assert
    found.sort();
    let mut expected = volume.missing.clone();
    expected.sort();
    assert_eq!(calls, 3);
    assert_eq!(found, expected);
    assert!(found.iter().all(|id| !volume.uploading.contains(id)));
    assert!(volume.present.iter().all(|id| !found.contains(id)));
    assert!(
        wrapped.iter().all(|id| volume.missing.contains(id)),
        "after the last row the walk starts again"
    );
    drop(inventory);
    rig.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn scan_page_walks_in_key_order_and_ends_with_no_next_when_the_rows_run_out() {
    // Arrange
    let rig = Rig::new().await;
    let volume = volume_with_gaps(&rig).await;
    let inventory = inventory(&rig);

    // Act
    let mut after = None;
    let mut pages = Vec::new();
    loop {
        let page = inventory.scan_page(after, 25).await.expect("page");
        after = page.next;
        pages.push(page);
        if after.is_none() {
            break;
        }
    }

    // Assert
    let lengths: Vec<usize> = pages.iter().map(|page| page.missing.len()).collect();
    let all: Vec<Uuid> = pages.iter().flat_map(|page| page.missing.clone()).collect();
    assert_eq!(pages.len(), 3, "two full pages and the empty end page");
    assert_eq!(lengths.iter().sum::<usize>(), MISSING);
    assert!(all.windows(2).all(|pair| pair[0] < pair[1]));
    assert!(all.iter().all(|id| volume.missing.contains(id)));
    drop(inventory);
    rig.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_batch_of_zero_still_moves_forward() {
    // Arrange
    let rig = Rig::new().await;
    volume_with_gaps(&rig).await;
    let inventory = inventory(&rig);

    // Act
    let page = inventory.scan_page(None, 0).await.expect("page");

    // Assert
    assert!(page.next.is_some(), "one row was checked");
    drop(inventory);
    rig.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_file_that_cannot_be_looked_up_is_an_error_not_a_missing_blob_and_the_position_is_kept() {
    // Arrange
    let rig = Rig::new().await;
    let id = Uuid::new_v4();
    insert(&rig, &[id], "committed").await;
    let path = blob_path(&rig.volume, id);
    let prefix = path
        .parent()
        .and_then(|p| p.parent())
        .expect("prefix folder")
        .to_path_buf();
    fs::create_dir_all(prefix.parent().expect("blobs folder")).expect("folders");
    fs::write(&prefix, b"not a folder").expect("blocker");
    let inventory = inventory(&rig);

    // Act
    let blocked = inventory.find_missing(10).await;
    fs::remove_file(&prefix).expect("remove the blocker");
    let cleared = inventory.find_missing(10).await.expect("page");

    // Assert
    assert!(
        matches!(blocked, Err(InventoryError::Unexpected(_))),
        "{blocked:?}"
    );
    assert_eq!(cleared, vec![id]);
    drop(inventory);
    rig.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_missing_database_table_is_a_typed_error() {
    // Arrange
    let rig = Rig::new().await;
    let inventory = inventory(&rig);
    rig.db
        .admin_exec("ALTER TABLE blobs RENAME TO blobs_hidden")
        .await;

    // Act
    let result = inventory.find_missing(10).await;

    // Assert
    assert!(
        matches!(result, Err(InventoryError::Unexpected(_))),
        "{result:?}"
    );
    drop(inventory);
    rig.finish().await;
}
