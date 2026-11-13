//! AZ-3707 AC-1 (complete ordered paging), AC-2 (limits and listing errors) and AC-3 (the page statement is an
//! index scan plus primary-key lookups as the API role with row-level security on), on DS-02 (IT-02-16). PT-02-1
//! (listing time) needs REF-SRV and is ignored elsewhere (`NOT-RUN(HW-REF)`).

use std::collections::HashSet;
use std::time::{Duration, Instant};

use contracts::{ListingPage, TreeReaderError};
use sc_db::Scope;
use sc_names::{name_key, validate_name};
use sqlx::AssertSqlSafe;
use sunnycloud_tree::{LISTING_PAGE_SQL, PgTreeReader};
use uuid::Uuid;

use crate::mutation_support::Tree;
use crate::reader_support::{DS02_ENTRIES, ds02, reader};

/// Every page from the start, following `next` with the default limit.
async fn all_pages(reader: &PgTreeReader, account: Uuid, folder: Uuid) -> Vec<ListingPage> {
    let mut pages: Vec<ListingPage> = Vec::new();
    loop {
        let after = pages.last().and_then(|page| page.next.clone());
        if !pages.is_empty() && after.is_none() {
            return pages;
        }
        let page = reader
            .list_children(account, folder, after.as_deref(), 0)
            .await
            .expect("page");
        pages.push(page);
    }
}

fn key_of(name: &str) -> String {
    name_key(&validate_name(name).expect("a listed name is valid"))
        .as_str()
        .to_owned()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ds02_lists_in_ten_default_pages_of_unique_names_in_name_key_order() {
    // Arrange
    let tree = Tree::new().await;
    let (folder, _) = ds02(&tree).await;
    let reader = reader(&tree, 1000);

    // Act
    let pages = all_pages(&reader, tree.account, folder).await;

    // Assert
    let keys: Vec<String> = pages
        .iter()
        .flat_map(|page| page.items.iter().map(|item| key_of(&item.name)))
        .collect();
    let unique: HashSet<&String> = keys.iter().collect();
    assert_eq!(pages.len(), 10);
    assert_eq!(keys.len(), DS02_ENTRIES);
    assert_eq!(unique.len(), DS02_ENTRIES);
    assert!(
        keys.windows(2).all(|pair| pair[0] < pair[1]),
        "name_key order"
    );
    assert!(pages[..9].iter().all(|page| page.items.len() == 1000
        && page.next.as_deref() == page.items.last().map(|item| key_of(&item.name)).as_deref()));
    assert_eq!(pages[9].next, None);
    drop(reader);
    tree.finish().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ds02_clamps_the_limit_and_refuses_a_garbage_cursor_a_file_and_a_missing_id() {
    // Arrange
    let tree = Tree::new().await;
    let (folder, file) = ds02(&tree).await;
    let reader = reader(&tree, 1500);
    let account = tree.account;

    // Act
    let clamped = reader
        .list_children(account, folder, None, 5000)
        .await
        .expect("clamped page");
    let garbage = reader
        .list_children(account, folder, Some("not/a key"), 0)
        .await;
    let a_file = reader.list_children(account, file, None, 0).await;
    let missing = reader.list_children(account, Uuid::new_v4(), None, 0).await;

    // Assert
    assert_eq!(clamped.items.len(), 1500);
    assert!(clamped.next.is_some());
    assert!(matches!(garbage, Err(TreeReaderError::InvalidCursor)));
    assert!(matches!(a_file, Err(TreeReaderError::NotAFolder)));
    assert!(matches!(missing, Err(TreeReaderError::NotFound)));
    drop(reader);
    tree.finish().await;
}

/// The plan of the page statement as the reader runs it: the API role, the account scope, row-level security.
async fn page_plan(tree: &Tree, folder: Uuid, after: Option<&str>) -> (String, String, bool) {
    let explain = format!("EXPLAIN (COSTS OFF) {LISTING_PAGE_SQL}");
    tree.fixture
        .api
        .run(Scope::Account(tree.account), async |tx| {
            let role: String = sqlx::query_scalar("SELECT current_user::text")
                .fetch_one(tx.conn())
                .await?;
            let row_security: bool = sqlx::query_scalar("SELECT row_security_active('nodes')")
                .fetch_one(tx.conn())
                .await?;
            let lines: Vec<String> = sqlx::query_scalar(AssertSqlSafe(explain))
                .bind(folder)
                .bind(tree.account)
                .bind(after)
                .bind(1001_i64)
                .fetch_all(tx.conn())
                .await?;
            Ok::<_, sqlx::Error>((lines.join("\n"), role, row_security))
        })
        .await
        .expect("explain")
}

fn assert_index_plan(plan: &str) {
    assert!(
        plan.contains("Index Scan using nodes_parent_name_key on nodes n"),
        "{plan}"
    );
    assert!(plan.contains("Nested Loop Left Join"), "{plan}");
    assert!(
        plan.contains("Index Scan using revisions_pkey on revisions r"),
        "{plan}"
    );
    assert!(!plan.contains("Seq Scan"), "{plan}");
    assert!(!plan.contains("Sort"), "{plan}");
    assert!(!plan.contains("Bitmap"), "{plan}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ds02_pages_read_the_name_index_and_revisions_by_primary_key_under_row_level_security() {
    // Arrange
    let tree = Tree::new().await;
    let (folder, _) = ds02(&tree).await;
    let middle = name_key(&validate_name("straße 05003").expect("valid"));

    // Act
    let (first, role, row_security) = page_plan(&tree, folder, None).await;
    let (later, _, _) = page_plan(&tree, folder, Some(middle.as_str())).await;

    // Assert
    assert_eq!(role, sc_testdb::API);
    assert!(row_security, "row-level security is on for the API role");
    assert_index_plan(&first);
    assert_index_plan(&later);
    tree.finish().await;
}

const LISTINGS: usize = 20;

fn p95(mut times: Vec<Duration>) -> Duration {
    times.sort_unstable();
    times[(times.len() * 95 / 100).min(times.len() - 1)]
}

/// PT-02-1 on REF-SRV only (`cargo test -- --ignored`): one 1000-entry page p95 at most 80 ms, the whole
/// 10,000-entry listing p95 at most 300 ms (targets 40 ms and 200 ms).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "NOT-RUN(HW-REF): PT-02-1 listing time is measured on REF-SRV only"]
async fn pt_02_1_ds02_full_listing_and_single_pages_meet_the_latency_bounds() {
    // Arrange
    let tree = Tree::new().await;
    let (folder, _) = ds02(&tree).await;
    let reader = reader(&tree, 1000);
    all_pages(&reader, tree.account, folder).await;

    // Act
    let (mut listings, mut pages) = (Vec::new(), Vec::new());
    for _ in 0..LISTINGS {
        let begun = Instant::now();
        let mut after: Option<String> = None;
        loop {
            let page_begun = Instant::now();
            let page = reader
                .list_children(tree.account, folder, after.as_deref(), 0)
                .await
                .expect("page");
            pages.push(page_begun.elapsed());
            after = page.next;
            if after.is_none() {
                break;
            }
        }
        listings.push(begun.elapsed());
    }
    let (listing_p95, page_p95) = (p95(listings), p95(pages));

    // Assert
    println!("PT-02-1: full listing p95 {listing_p95:?}, page p95 {page_p95:?}");
    assert!(listing_p95 <= Duration::from_millis(300), "{listing_p95:?}");
    assert!(page_p95 <= Duration::from_millis(80), "{page_p95:?}");
    drop(reader);
    tree.finish().await;
}
