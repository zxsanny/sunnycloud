//! AZ-3659 (E-22/p07), flow F8 and F12 step 1 through the real `sunnycloud serve` binary on PostgreSQL 18 and a blob
//! volume over HTTP: a stale edit kept as a conflict copy named for the committing device (AC-1), edit beats delete
//! and the stale delete (AC-2), the name rules of the commit and node routes with the NFC/NFD pair of G-1 (AC-3),
//! the delete's tombstone, usage and GC row in one change (AC-4), the production callers (AC-5), a move racing an
//! edit (AC-70) and the usage under 1,000 concurrent commits, edits and deletes (AC-74).
//!
//! Every edit and new file is a staged small file committed by `commits:batch`; a second device has its own label,
//! so a conflict-copy name shows which device committed. No duration of these flows is waited for: retention is
//! only read back from the GC row's `due_at`.

mod pipeline_support;
mod root_support;
mod serve_support;

use std::collections::BTreeSet;
use std::sync::Barrier;
use std::thread;

use pipeline_support::tree::{
    Done, Op, commit_on, copy_names, error_of, feed_nodes, names, patch_on, uuid_of,
};
use pipeline_support::wiring::{Caller, missing_callers};
use pipeline_support::{File, Pipeline, content, now_micros};
use serde_json::json;
use uuid::Uuid;

const DEVICE_B: &str = "desk-b";

#[tokio::test(flavor = "multi_thread")]
async fn ac1_a_stale_edit_becomes_current_and_the_superseded_revision_moves_to_a_conflict_copy() {
    // Arrange: x at R1, edited by device A to R2; device B still holds R1.
    let pipeline = Pipeline::start(&[]).await;
    let device_b = pipeline.device_named(DEVICE_B).await;
    let folder = pipeline.folder("ac1");
    let r1 = pipeline.commit_as(&pipeline.device, folder, "x", &content(1, 4096), None);
    let x = uuid_of(&r1, "node_id");
    let r1 = uuid_of(&r1, "revision_id");
    let r2 = pipeline.commit_as(&pipeline.device, folder, "x", &content(2, 5000), Some(r1));
    let r2 = uuid_of(&r2, "revision_id");
    let cursor = pipeline.feed_cursor();
    let before = now_micros();

    // Act
    let stale = pipeline.commit_as(&device_b, folder, "x", &content(3, 6000), Some(r1));
    let after = now_micros();

    // Assert
    assert_eq!(stale["status"], "committed", "{stale}");
    assert_eq!(uuid_of(&stale, "node_id"), x);
    let copy = uuid_of(&stale, "conflict_copy");
    assert_eq!(
        pipeline.node(x)["current_revision"],
        stale["revision_id"],
        "the stale commit is current"
    );
    let copy_view = pipeline.node(copy);
    assert_eq!(
        copy_view["current_revision"],
        json!(r2),
        "R2 moved, same blob"
    );
    assert_eq!(copy_view["parent_id"], json!(folder));
    let expected = copy_names("x", "", DEVICE_B, before, after);
    let copy_name = copy_view["name"].as_str().expect("a name").to_owned();
    assert!(
        expected.contains(&copy_name),
        "{copy_name} not in {expected:?}"
    );
    assert_eq!(pipeline.download(copy), content(2, 5000));
    let fed = feed_nodes(&pipeline.changes_since(&cursor));
    assert_eq!(fed, BTreeSet::from([x, copy]));
    pipeline.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn ac2_an_edit_revives_a_deleted_file_a_stale_delete_is_edited_since_and_a_repeated_delete_succeeds()
 {
    // Arrange: a tombstoned file t; a file e edited since device B's base.
    let pipeline = Pipeline::start(&[]).await;
    let device_b = pipeline.device_named(DEVICE_B).await;
    let folder = pipeline.folder("ac2");
    let t = pipeline.commit_as(&pipeline.device, folder, "t.txt", &content(1, 3000), None);
    let (t_node, t_rev) = (uuid_of(&t, "node_id"), uuid_of(&t, "revision_id"));
    assert_eq!(pipeline.delete_with_base(t_node, Some(t_rev)).status, 204);
    let e = pipeline.commit_as(&pipeline.device, folder, "e.txt", &content(2, 3000), None);
    let (e_node, e_r1) = (uuid_of(&e, "node_id"), uuid_of(&e, "revision_id"));
    let e_r2 = pipeline.commit_as(
        &pipeline.device,
        folder,
        "e.txt",
        &content(3, 3100),
        Some(e_r1),
    );
    let e_r2 = uuid_of(&e_r2, "revision_id");

    // Act
    let revived = pipeline.commit_as(
        &pipeline.device,
        folder,
        "t.txt",
        &content(4, 3200),
        Some(t_rev),
    );
    let stale_delete = pipeline.call(
        "DELETE",
        &format!("/api/v1/nodes/{e_node}?base_revision={e_r1}"),
        &[],
        &[],
    );
    let stale_by_b = serve_support::http::call(
        pipeline.stack.ports.public,
        "DELETE",
        &format!("/api/v1/nodes/{e_node}?base_revision={e_r1}"),
        &device_b,
        &[],
        &[],
    );
    let deleted = pipeline.delete_with_base(e_node, Some(e_r2));
    let repeated = pipeline.delete_with_base(e_node, Some(e_r2));

    // Assert
    assert_eq!(revived["status"], "committed", "{revived}");
    assert_eq!(uuid_of(&revived, "node_id"), t_node);
    assert!(revived["conflict_copy"].is_null(), "{revived}");
    let t_view = pipeline.node(t_node);
    assert_eq!(t_view["state"], "live");
    assert_eq!(t_view["current_revision"], revived["revision_id"]);
    assert_eq!(
        names(&pipeline.children(folder)),
        BTreeSet::from(["t.txt".to_owned()])
    );
    assert_eq!(error_of(&stale_delete), (409, "edited_since".to_owned()));
    assert_eq!(error_of(&stale_by_b), (409, "edited_since".to_owned()));
    assert_eq!(deleted.status, 204, "{deleted:?}");
    assert_eq!(repeated.status, 204, "{repeated:?}");
    assert_eq!(pipeline.node(e_node)["state"], "tombstone");
    pipeline.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn ac3_a_case_collision_gets_the_server_name_on_commit_and_name_conflict_or_cycle_on_the_node_routes()
 {
    // Arrange: folder d holding A.txt, a folder b, and p/c for the cycle.
    let pipeline = Pipeline::start(&[]).await;
    let d = pipeline.folder("ac3");
    let upper = pipeline.commit_as(&pipeline.device, d, "A.txt", &content(1, 2000), None);
    let upper = uuid_of(&upper, "node_id");
    let b = pipeline.subfolder(d, "b");
    let p = pipeline.subfolder(d, "p");
    let c = pipeline.subfolder(p, "c");
    let before = now_micros();

    // Act
    let lower = pipeline.commit_as(&pipeline.device, d, "a.txt", &content(2, 2100), None);
    let after = now_micros();
    let folder_create = pipeline.create_folder(d, "a.TXT");
    let rename_onto = pipeline.patch_node(b, &json!({"name": "A.txt"}));
    let case_only = pipeline.patch_node(upper, &json!({"name": "a.txt"}));
    let cycle = pipeline.patch_node(p, &json!({"parent_id": c}));

    // Assert
    assert_eq!(lower["status"], "committed", "{lower}");
    let lower_node = uuid_of(&lower, "node_id");
    assert_eq!(uuid_of(&lower, "conflict_copy"), lower_node, "{lower}");
    let server_name = pipeline.node(lower_node)["name"]
        .as_str()
        .expect("name")
        .to_owned();
    let expected = copy_names("a", ".txt", "laptop", before, after);
    assert!(
        expected.contains(&server_name),
        "{server_name} not in {expected:?}"
    );
    assert_eq!(error_of(&folder_create), (409, "name_conflict".to_owned()));
    assert_eq!(error_of(&rename_onto), (409, "name_conflict".to_owned()));
    assert_eq!(case_only.status, 200, "{case_only:?}");
    let renamed = case_only.json();
    assert_eq!(
        (uuid_of(&renamed, "node_id"), renamed["name"].clone()),
        (upper, json!("a.txt"))
    );
    assert_eq!(error_of(&cycle), (409, "cycle".to_owned()));
    assert_eq!(pipeline.node(p)["parent_id"], json!(d));
    let listed = names(&pipeline.children(d));
    let expected: BTreeSet<String> = ["a.txt", "b", "p", server_name.as_str()]
        .into_iter()
        .map(str::to_owned)
        .collect();
    assert_eq!(listed, expected);
    pipeline.finish().await;
}

/// G-1 (resolved 2026-11-11 by the owner, `tests/spec-gaps.md`): an NFC and an NFD spelling are one name, the
/// stored name keeps the first writer's spelling.
#[tokio::test(flavor = "multi_thread")]
async fn ac3_g1_an_nfd_spelling_of_a_stored_nfc_name_is_the_same_name_on_both_entry_points() {
    // Arrange
    let (nfc, nfd) = ("caf\u{e9}.md", "cafe\u{301}.md");
    let pipeline = Pipeline::start(&[]).await;
    let d = pipeline.folder("g1");
    let first = pipeline.commit_as(&pipeline.device, d, nfc, &content(1, 1000), None);
    let first = uuid_of(&first, "node_id");
    let b = pipeline.subfolder(d, "b");
    let before = now_micros();

    // Act
    let second = pipeline.commit_as(&pipeline.device, d, nfd, &content(2, 1100), None);
    let after = now_micros();
    let rename_onto = pipeline.patch_node(b, &json!({"name": nfd}));
    let folder_create = pipeline.create_folder(d, nfd);

    // Assert
    assert_eq!(
        pipeline.node(first)["name"],
        nfc,
        "the first spelling is kept"
    );
    let second_node = uuid_of(&second, "node_id");
    assert_eq!(uuid_of(&second, "conflict_copy"), second_node, "{second}");
    let server_name = pipeline.node(second_node)["name"]
        .as_str()
        .expect("name")
        .to_owned();
    let expected = copy_names("cafe\u{301}", ".md", "laptop", before, after);
    assert!(
        expected.contains(&server_name),
        "{server_name:?} not in {expected:?}"
    );
    assert_eq!(error_of(&rename_onto), (409, "name_conflict".to_owned()));
    assert_eq!(error_of(&folder_create), (409, "name_conflict".to_owned()));
    pipeline.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn ac4_a_delete_with_its_current_base_tombstones_releases_usage_and_queues_the_blob_after_retention()
 {
    // Arrange: retention 3 days, so the GC row's due time shows the configured value.
    const RETENTION_MICROS: i64 = 3 * 86_400_000_000;
    let pipeline = Pipeline::start(&[("retention", "3d")]).await;
    let folder = pipeline.folder("ac4");
    let size = 300_000;
    let file = pipeline.commit_as(
        &pipeline.device,
        folder,
        "gone.bin",
        &content(1, size),
        None,
    );
    let (node, revision) = (uuid_of(&file, "node_id"), uuid_of(&file, "revision_id"));
    let (used_before, _) = pipeline.quota();
    let cursor = pipeline.feed_cursor();
    let before = now_micros();

    // Act
    let deleted = pipeline.delete_with_base(node, Some(revision));
    let after = now_micros();

    // Assert
    assert_eq!(deleted.status, 204, "{deleted:?}");
    let fed = pipeline.changes_since(&cursor);
    assert_eq!(fed.len(), 1, "{fed:?}");
    assert_eq!(
        (uuid_of(&fed[0], "node_id"), fed[0]["state"].clone()),
        (node, json!("tombstone"))
    );
    let (used_after, _) = pipeline.quota();
    assert_eq!(
        used_before - used_after,
        i64::try_from(size).expect("small")
    );
    let (reason, due) = pipeline.gc_row(revision).await.expect("a gc row");
    assert_eq!(reason, "deleted");
    assert!(
        (before + RETENTION_MICROS..=after + RETENTION_MICROS).contains(&due),
        "due {due} not in [{}, {}]",
        before + RETENTION_MICROS,
        after + RETENTION_MICROS
    );
    pipeline.finish().await;
}

/// The F8 and F12 step 1 joins: each operation, the non-test file that calls or binds it, and its text.
const CALLERS: &[Caller] = &[
    (
        "TreeService::{create_folder, update_node, delete_node} from the node routes",
        "components/11_http_api/src/registry/tree/handlers.rs",
        &[
            ".create_folder(user.account, request.parent_id, &request.name)",
            ".update_node(user.account, id, request.name.as_deref(), request.parent_id)",
            ".delete_node(user.account, id, base_revision)",
        ],
    ),
    (
        "the committing device's label in the commit actor",
        "components/11_http_api/src/registry/transfer/commits.rs",
        &["device_label: user.principal.label.clone()"],
    ),
    (
        "the conflict path of TreeCommit::publish (copy, created-as-copy, revival)",
        "components/02_tree/src/publish.rs",
        &[
            "insert_as_copy(tx, &file, at.naming).await?",
            "return self.revive(tx, at, node_id, &file, ancestors).await",
            ".keep_superseded(tx, at, node_id, &file, superseded)",
            "self.stamp.stamp(tx, account, touched)",
        ],
    ),
    (
        "QuotaLedger::adjust_used, the GcQueue schedule and FeedStamp::stamp of every node mutation",
        "components/02_tree/src/service.rs",
        &[
            ".adjust_used(tx, account, -current.size)",
            ".schedule_in_tx(tx, current.blob_id, GcReason::Deleted, Due::AfterRetention)",
            "let seq = self.stamp(tx, account, id).await?",
            "view.seq = self.stamp(tx, account, node_id).await?",
            "self.stamp(tx, account, node_id).await.map(Some)",
            ".stamp(tx, account, TouchedNodes(vec![node]))",
        ],
    ),
    (
        "the tree service bound to the real quota ledger, GC queue and feed stamp",
        "server/src/root/domain.rs",
        &[
            "let service = Arc::new(PgTreeService::new(",
            "quota.ledger.clone(),",
            "blobs.gc.clone(),",
            "feed.stamp.clone(),",
        ],
    ),
    (
        "the node routes bound to the tree service",
        "server/src/root/api.rs",
        &["tree: tree.service.clone()"],
    ),
];

#[test]
fn ac5_every_f8_operation_has_a_production_caller() {
    // Arrange
    let callers = CALLERS;

    // Act
    let missing = missing_callers(callers);

    // Assert
    assert!(missing.is_empty(), "{missing:#?}");
}

const ROUNDS: usize = 20;

#[tokio::test(flavor = "multi_thread")]
async fn ac70_a_move_of_the_folder_racing_an_edit_of_its_file_keeps_both_with_no_conflict_copy() {
    // Arrange
    let pipeline = Pipeline::start(&[]).await;
    let device_b = pipeline.device_named(DEVICE_B).await;
    let public = pipeline.stack.ports.public;
    for round in 0..ROUNDS {
        let f = pipeline.folder(&format!("f-{round}"));
        let g = pipeline.folder(&format!("g-{round}"));
        let seed = u64::try_from(round).expect("small") * 2;
        let x = pipeline.commit_as(&pipeline.device, f, "x.txt", &content(seed + 1, 4000), None);
        let (x_node, base) = (uuid_of(&x, "node_id"), uuid_of(&x, "revision_id"));
        let edit = File::new("x.txt", 4100, seed + 2);
        assert_eq!(pipeline.stage_as(&device_b, &edit).status, 200);
        let cursor = pipeline.feed_cursor();
        let start = Barrier::new(2);

        // Act: A moves f into g while B commits its edit of x, both released by one barrier.
        let (moved, edited) = thread::scope(|scope| {
            let mover = scope.spawn(|| {
                start.wait();
                patch_on(public, &pipeline.device, f, &json!({"parent_id": g}))
            });
            let editor = scope.spawn(|| {
                start.wait();
                commit_on(public, &device_b, &edit, f, Some(base))
            });
            (mover.join().expect("mover"), editor.join().expect("editor"))
        });

        // Assert
        assert_eq!(moved.status, 200, "round {round}: {moved:?}");
        assert_eq!(edited.status, 200, "round {round}: {edited:?}");
        let edited = &edited.json()["entries"][0];
        assert_eq!(edited["status"], "committed", "round {round}: {edited}");
        assert_eq!(uuid_of(edited, "node_id"), x_node, "round {round}");
        assert!(edited["conflict_copy"].is_null(), "round {round}: {edited}");
        assert_eq!(pipeline.node(f)["parent_id"], json!(g), "round {round}");
        let x_view = pipeline.node(x_node);
        assert_eq!(x_view["parent_id"], json!(f), "round {round}");
        assert_eq!(
            x_view["current_revision"], edited["revision_id"],
            "round {round}"
        );
        assert_eq!(
            pipeline.children(f).len(),
            1,
            "round {round}: a conflict copy"
        );
        assert!(
            pipeline.download(x_node) == edit.bytes,
            "round {round}: content"
        );
        let fed = feed_nodes(&pipeline.changes_since(&cursor));
        assert_eq!(fed, BTreeSet::from([f, x_node]), "round {round}");
    }
    pipeline.finish().await;
}

const SEEDS: usize = 400;
const STREAMS: usize = 16;

fn mix_size(index: usize) -> usize {
    64 * 1024 + index * 7919 % (128 * 1024)
}

#[tokio::test(flavor = "multi_thread")]
async fn ac74_usage_stays_exact_and_within_quota_under_1000_concurrent_commits_edits_and_deletes() {
    // Arrange: 400 committed files; a quota 24 MiB above their total, so some new files are refused.
    let mut pipeline = Pipeline::start(&[]).await;
    let folder = pipeline.folder("ac74");
    let seeds: Vec<File> = (0..SEEDS)
        .map(|index| {
            File::new(
                &format!("seed-{index:03}"),
                mix_size(index),
                index as u64 + 1,
            )
        })
        .collect();
    let mut current = Vec::with_capacity(SEEDS);
    for chunk in seeds.chunks(200) {
        assert!(
            pipeline
                .stage_all(chunk)
                .await
                .iter()
                .all(|staged| staged.status == 200)
        );
        let committed =
            pipeline.commit_batch(Uuid::new_v4(), &pipeline_support::entries(chunk, folder));
        for result in committed.json()["entries"].as_array().expect("entries") {
            assert_eq!(result["status"], "committed", "{result}");
            current.push((uuid_of(result, "node_id"), uuid_of(result, "revision_id")));
        }
    }
    let limit = pipeline.used_bytes().await + 24 * 1024 * 1024;
    pipeline.set_limit(limit).await;
    let ops: Vec<Op> = (0..1000)
        .map(|index| {
            let seed = 10_000 + index as u64;
            match index % 5 {
                3 => {
                    let (_, base) = current[index / 5];
                    let name = &seeds[index / 5].name;
                    Op::Edit(File::new(name, mix_size(index + 3), seed), base)
                }
                4 => {
                    let (node, base) = current[200 + index / 5];
                    Op::Delete(node, base)
                }
                _ => Op::New(File::new(&format!("new-{index:04}"), mix_size(index), seed)),
            }
        })
        .collect();

    // Act
    let mix = pipeline.run_mix(folder, ops, STREAMS).await;

    // Assert
    let refused: Vec<&Done> = mix.done.iter().filter(|done| **done != Done::Ok).collect();
    assert!(!refused.is_empty(), "the quota was never reached");
    assert!(
        refused
            .iter()
            .all(|done| **done == Done::Refused(507, "quota_exceeded".to_owned())),
        "{refused:?}"
    );
    let highest = mix.samples.iter().copied().max().expect("samples");
    assert!(highest <= limit, "usage {highest} above the quota {limit}");
    let live: i64 = pipeline
        .children(folder)
        .iter()
        .map(|node| node["size"].as_i64().expect("a size"))
        .sum();
    assert_eq!(pipeline.used_bytes().await, live);
    assert_eq!(pipeline.reserved_bytes().await, 0);
    let drift = pipeline.quota_drift_findings();
    let matches = ("ok", "stored usage matches the files for every account");
    assert_eq!(drift, [(matches.0.to_owned(), matches.1.to_owned())]);
    pipeline.finish().await;
}
