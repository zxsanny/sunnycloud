//! AZ-3844 AC-2 (IT-08-13) and the lookup half of AC-3 (ST-08-4): a node or a live file resolves to the derived
//! blob of its content, `NotReady` while a job is pending, `NotFound` otherwise; never the original, and nothing
//! of another account. The files are published through the real 02 `publish` with the real media effect, so the
//! jobs are the planner's; the node lookup is the real `PgTreeReader`, the key material the real 04 catalog, and
//! the ready thumbnail is a WebP written and read back through 04.

use std::collections::HashMap;
use std::sync::Arc;

use contracts::{
    CommitEffect, DerivedKind, DerivedOpener, EntryResult, LiveFile, LiveFileLookup, PublishResult,
    TreeCommit,
};
use media_api::{DerivedFiles, JobPlanner};
use media_shared::policy::CURRENT_PARAMS_VERSION;
use sc_ids::FileHash;
use sqlx::Connection;
use sunnycloud_blobstore::BlobCatalog;
use sunnycloud_tree::{PgTreeReader, TreeConfig};
use uuid::Uuid;

use crate::blob_support::{Store, WEBP};
use crate::derived_support::{Outcome, outcome};
use crate::effect_support::{current, jobs};
use crate::publish_support::{actor, new_file, tree_service};
use crate::queue_support::{Queue, hash};
use crate::support::{DerivedRow, Fixture, blob, derived, load_settings};

const JPEG: Option<&str> = Some("image/jpeg");

/// (name, content, media type, vault): a photo with a thumbnail, a photo whose job is queued, an SVG, a vault
/// JPEG, a compliant video (poster made, transcode skipped) and a photo whose job died.
const FILES: [(&str, u32, Option<&str>, bool); 6] = [
    ("ready.jpg", 1, JPEG, false),
    ("queued.jpg", 2, JPEG, false),
    ("logo.svg", 3, Some("image/svg+xml"), false),
    ("secret.jpg", 4, JPEG, true),
    ("clip.mp4", 5, Some("video/mp4"), false),
    ("dead.jpg", 6, JPEG, false),
];

struct Seeded {
    q: Queue,
    store: Store,
    b: Uuid,
    root: Uuid,
    reader: Arc<PgTreeReader>,
    opener: DerivedFiles,
    nodes: HashMap<&'static str, Uuid>,
    ready_blob: Uuid,
}

impl Seeded {
    async fn finish(self) {
        let Seeded {
            q,
            store,
            reader,
            opener,
            ..
        } = self;
        drop((store, reader, opener));
        q.finish().await;
    }

    async fn live_file(&self, name: &str) -> LiveFile {
        match self
            .reader
            .resolve_live_file(self.q.account, self.nodes[name])
            .await
            .expect("resolve")
        {
            LiveFileLookup::Found(file) => file,
            LiveFileLookup::Gone => panic!("{name} is gone"),
        }
    }
}

/// Moves the account's `kind` job of `content` to `status` (finished with `outcome` when terminal).
async fn finish_job(
    q: &Queue,
    content: &FileHash,
    kind: &str,
    status: &str,
    outcome: Option<&str>,
) {
    let mut conn = q.fixture.owner_conn().await;
    let updated = sqlx::query(
        "UPDATE jobs SET status = $4, outcome = $5, finished_at = 'epoch'::timestamptz \
         WHERE account_id = $1 AND content_hash = $2 AND kind = $3",
    )
    .bind(q.account)
    .bind(content.as_bytes().as_slice())
    .bind(kind)
    .bind(status)
    .bind(outcome)
    .execute(&mut conn)
    .await
    .expect("finish job");
    assert_eq!(updated.rows_affected(), 1, "{kind} job of the content");
    conn.close().await.expect("close owner connection");
}

async fn derived_row(q: &Queue, content: &FileHash, kind: &str, blob_id: Uuid, fresh_blob: bool) {
    let mut conn = q.fixture.owner_conn().await;
    if fresh_blob {
        blob(&mut conn, blob_id, q.account).await;
    }
    derived(
        &mut conn,
        &DerivedRow {
            account: q.account,
            content_hash: content.as_bytes(),
            kind,
            params_version: 1,
            blob: blob_id,
        },
    )
    .await
    .expect("derived row");
    conn.close().await.expect("close owner connection");
}

async fn seed() -> Seeded {
    // The keyring starts on an empty volume, before any `blobs` row exists.
    let fixture = Fixture::new().await;
    let store = Store::new(&fixture).await;
    let q = Queue::with_fixture(fixture).await;
    let (b, _) = q.second_account().await;
    let (service, root) = tree_service(&q).await;
    let mut entries = Vec::new();
    for (name, n, media_type, vault) in FILES {
        entries.push(new_file(&q, root, name, hash(n), media_type, vault).await);
    }
    let effects: Vec<Arc<dyn CommitEffect>> = vec![Arc::new(current(&q))];
    let result = service
        .publish(actor(q.account), entries, effects, sc_testdb::START)
        .await
        .expect("publish");
    let PublishResult::Published(results) = result else {
        panic!("unexpected replay");
    };
    let nodes = FILES
        .iter()
        .zip(&results)
        .map(|((name, ..), result)| match result {
            EntryResult::Published { node_id, .. } => (*name, *node_id),
            other => panic!("{name} not published: {other:?}"),
        })
        .collect();
    drop(service);
    let ready_blob = store.put(&q.fixture, q.account, &WEBP).await;
    derived_row(&q, &hash(1), "thumb", ready_blob, false).await;
    finish_job(&q, &hash(1), "stills", "succeeded", Some("derived")).await;
    derived_row(&q, &hash(5), "thumb", Uuid::new_v4(), true).await;
    finish_job(&q, &hash(5), "poster", "succeeded", Some("derived")).await;
    let compliant = Some("skipped_compliant");
    finish_job(&q, &hash(5), "transcode", "succeeded", compliant).await;
    finish_job(&q, &hash(6), "stills", "dead", None).await;
    let settings = load_settings(&[]).expect("settings");
    let reader = Arc::new(PgTreeReader::new(
        q.fixture.api.clone(),
        &TreeConfig::from_settings(&settings),
    ));
    let opener = DerivedFiles::new(
        q.fixture.api.clone(),
        reader.clone(),
        Arc::new(BlobCatalog::new(q.fixture.api.clone())),
        JobPlanner::new(CURRENT_PARAMS_VERSION, 200),
    );
    Seeded {
        q,
        store,
        b,
        root,
        reader,
        opener,
        nodes,
        ready_blob,
    }
}

const CASES: [(&str, DerivedKind); 8] = [
    ("ready.jpg", DerivedKind::Thumb),
    ("ready.jpg", DerivedKind::Preview),
    ("queued.jpg", DerivedKind::Thumb),
    ("logo.svg", DerivedKind::Thumb),
    ("secret.jpg", DerivedKind::Thumb),
    ("clip.mp4", DerivedKind::Transcode),
    ("clip.mp4", DerivedKind::Thumb),
    ("dead.jpg", DerivedKind::Thumb),
];

/// The position of a case in `CASES`.
fn case(name: &str, kind: DerivedKind) -> usize {
    CASES
        .iter()
        .position(|listed| *listed == (name, kind))
        .expect("a listed case")
}

#[tokio::test]
async fn each_file_gives_its_derived_blob_not_ready_or_not_found_by_node_and_by_live_file() {
    // Arrange
    let s = seed().await;

    // Act
    let mut by_node = Vec::new();
    let mut by_file = Vec::new();
    for (name, kind) in CASES {
        let node = s.nodes[name];
        let file = s.live_file(name).await;
        let opened = s.opener.open_derived(s.q.account, node, kind).await;
        by_node.push(outcome(opened));
        let opened = s.opener.open_derived_for(s.q.account, &file, kind).await;
        by_file.push(outcome(opened));
    }
    let folder = outcome(
        s.opener
            .open_derived(s.q.account, s.root, DerivedKind::Thumb)
            .await,
    );

    // Assert
    let Outcome::Blob(ready) = &by_node[case("ready.jpg", DerivedKind::Thumb)] else {
        panic!("{by_node:?}");
    };
    assert_eq!(
        (ready.blob.blob_id, ready.media_type.as_str()),
        (s.ready_blob, "image/webp")
    );
    assert!(matches!(
        by_node[case("clip.mp4", DerivedKind::Thumb)],
        Outcome::Blob(_)
    ));
    assert_eq!(
        by_node[case("queued.jpg", DerivedKind::Thumb)],
        Outcome::NotReady
    );
    for (name, kind) in [
        ("ready.jpg", DerivedKind::Preview),
        ("logo.svg", DerivedKind::Thumb),
        ("secret.jpg", DerivedKind::Thumb),
        ("clip.mp4", DerivedKind::Transcode),
        ("dead.jpg", DerivedKind::Thumb),
    ] {
        assert_eq!(
            by_node[case(name, kind)],
            Outcome::NotFound,
            "{name} {kind:?}"
        );
    }
    assert_eq!(folder, Outcome::NotFound);
    assert_eq!(by_file, by_node);
    s.finish().await;
}

#[tokio::test]
async fn the_planner_made_no_job_for_the_svg_or_the_vault_file() {
    // Arrange
    let s = seed().await;

    // Act
    let svg = jobs(&s.q, &hash(3)).await;
    let vault = jobs(&s.q, &hash(4)).await;

    // Assert
    assert_eq!((svg, vault), (Vec::new(), Vec::new()));
    s.finish().await;
}

#[tokio::test]
async fn the_ready_thumbnail_reads_back_as_the_webp_written_and_not_as_the_original() {
    // Arrange
    let s = seed().await;
    let file = s.live_file("ready.jpg").await;

    // Act
    let derived = s
        .opener
        .open_derived(s.q.account, s.nodes["ready.jpg"], DerivedKind::Thumb)
        .await
        .expect("derived blob");
    let bytes = s.store.read(&derived.blob).await;

    // Assert
    assert_eq!(bytes, WEBP);
    assert_eq!(derived.media_type, "image/webp");
    assert_ne!(Some(derived.blob.blob_id), file.blob_ref);
    s.finish().await;
}

#[tokio::test]
async fn another_account_finds_nothing_of_a_by_node_or_by_live_file() {
    // Arrange
    let s = seed().await;

    // Act
    let mut answers = Vec::new();
    for (name, kind) in CASES {
        let file = s.live_file(name).await;
        answers.push(outcome(
            s.opener.open_derived(s.b, s.nodes[name], kind).await,
        ));
        answers.push(outcome(s.opener.open_derived_for(s.b, &file, kind).await));
    }

    // Assert
    assert!(
        answers.iter().all(|answer| *answer == Outcome::NotFound),
        "{answers:?}"
    );
    s.finish().await;
}
