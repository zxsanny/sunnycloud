//! AZ-3719 AC-3 (ST-02-3, AC-37): hostile names are only data. Each is stored verbatim as a file (`publish`) and
//! as a folder (`create_folder`), listed once, resolved by path; crafted cursors and device filters with quotes
//! give `InvalidCursor`, a literal match or nothing, never a SQL error or an extra row.

use contracts::{Page, TreeReaderError, TreeService};
use sc_names::{name_key, validate_name};

use crate::mutation_support::Tree;
use crate::publish_support::new_file;
use crate::reader_support::reader;
use crate::revision_support::device_meta;

fn hostile() -> Vec<String> {
    let mut names: Vec<String> = ["'; DROP TABLE nodes;--", "%", "_", "a\\b", "..%2f"]
        .map(str::to_owned)
        .to_vec();
    names.push("日".repeat(85));
    names
}

fn key(name: &str) -> String {
    name_key(&validate_name(name).expect("a valid name"))
        .as_str()
        .to_owned()
}

/// The names of `names` whose key sorts after `cursor`, in key order: what a literal cursor must give.
fn after(names: &[String], cursor: &str) -> Vec<String> {
    let mut keyed: Vec<(String, String)> =
        names.iter().map(|name| (key(name), name.clone())).collect();
    keyed.sort();
    keyed
        .into_iter()
        .filter(|(key, _)| key.as_str() > cursor)
        .map(|(_, name)| name)
        .collect()
}

#[tokio::test]
async fn hostile_names_are_stored_verbatim_listed_once_resolved_and_never_reach_the_sql() {
    // Arrange
    let tree = Tree::new().await;
    let account = tree.account;
    let files = tree.folder(tree.root, "files").await;
    let folders = tree.folder(tree.root, "folders").await;
    let names = hostile();
    assert_eq!(names[5].len(), 255);
    let blobs = tree.blobs(names.len()).await;
    let quoted_device = "o'brien\"; --";
    let entries = names
        .iter()
        .zip(&blobs)
        .map(|(name, blob)| {
            let mut entry = new_file(files, name, *blob);
            entry.revision.client_meta = Some(device_meta(&quoted_device.replace('"', "\\\"")));
            entry
        })
        .collect();
    let published = tree.publish_with(entries, Vec::new()).await;
    for name in &names {
        tree.service
            .create_folder(account, folders, name)
            .await
            .expect("hostile folder");
    }
    let reader = reader(&tree, 1000);

    // Act
    let mut listed = Vec::new();
    for folder in [files, folders] {
        let page = reader
            .list_children(account, folder, None, 0)
            .await
            .expect("listing");
        listed.push(
            page.items
                .into_iter()
                .map(|item| item.name)
                .collect::<Vec<_>>(),
        );
    }
    let mut resolved = Vec::new();
    for name in &names {
        for folder in ["files", "folders"] {
            resolved.push(
                tree.service
                    .resolve_path(account, &format!("{folder}/{name}"))
                    .await,
            );
        }
    }
    let mut refused = Vec::new();
    for path in ["../x", "a//b", "files/../folders", "files/./%"] {
        refused.push(format!(
            "{:?}",
            tree.service.resolve_path(account, path).await
        ));
    }
    let mut cursors = Vec::new();
    let crafted: Vec<String> = names
        .iter()
        .map(|name| key(name))
        .chain([
            "' OR '1'='1".to_owned(),
            "x'; --".to_owned(),
            "'".to_owned(),
        ])
        .collect();
    for cursor in &crafted {
        let page = reader.list_children(account, files, Some(cursor), 0).await;
        cursors.push((cursor.clone(), page));
    }
    let page = Page {
        offset: 0,
        limit: 0,
    };
    let by_quoted = tree
        .service
        .find_revisions(account, Some(quoted_device), page)
        .await
        .expect("quoted device filter");
    let by_injection = tree
        .service
        .find_revisions(account, Some("x' OR '1'='1"), page)
        .await
        .expect("injection device filter");

    // Assert
    let in_key_order = after(&names, "");
    assert_eq!(listed, [in_key_order.clone(), in_key_order]);
    assert!(
        resolved.iter().all(Result::is_ok),
        "every hostile name resolves: {resolved:?}"
    );
    assert_eq!(refused, ["Err(NotFound)"; 4]);
    for (cursor, page) in cursors {
        match page {
            Ok(page) => {
                let names_after: Vec<String> =
                    page.items.into_iter().map(|item| item.name).collect();
                assert_eq!(names_after, after(&names, &cursor), "cursor {cursor:?}");
            }
            Err(TreeReaderError::InvalidCursor) => {}
            Err(other) => panic!("cursor {cursor:?}: {other:?}"),
        }
    }
    assert_eq!(by_quoted.len(), names.len());
    assert!(by_injection.is_empty());
    assert_eq!(published.len(), names.len());
    let nodes = tree.nodes().await;
    assert_eq!(
        nodes,
        3 + 2 * names.len() as i64,
        "root, two folders and the hostile nodes"
    );
    let tables = tree
        .count("SELECT count(*) FROM pg_tables WHERE tablename IN ('nodes', 'revisions')")
        .await;
    assert_eq!(tables, 2);
    drop(reader);
    tree.finish().await;
}
