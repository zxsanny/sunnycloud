//! AZ-3658 (E-22/p06), flow F7 through the real `sunnycloud serve` binary on PostgreSQL 18 over HTTP: a commit
//! wakes a held poll (AC-1), an empty page after a short wait with no connection held (AC-2), the three resync
//! reasons (AC-3), revocation (AC-4) and SIGTERM (AC-5) ending held polls, the production callers and the one
//! notifier (AC-6), and 50 concurrent commits read back gapless and once each (AC-69).
//!
//! Every poll asserted as held is first proven parked: `max_holds_per_principal` is 1, so a probe of the same
//! principal refused 429 shows the hold is on the server. AC-1 sets `feed_safety_recheck` above its 5 s bound, so
//! only the notifier can deliver the change in time (Risk 1). AC-4 keeps the production `longpoll_auth_recheck`
//! and `session_cache_ttl` (2 s each), which the 5 s bound is about.

mod pipeline_support;
mod root_support;
mod serve_support;

use std::collections::BTreeSet;
use std::thread;
use std::time::{Duration, Instant};

use pipeline_support::download::id;
use pipeline_support::feed::{Page, answer, production_occurrences, split};
use pipeline_support::large::LargeFile;
use pipeline_support::wiring::{Caller, missing_callers};
use pipeline_support::{File, Pipeline, WITHIN, entry};
use serde_json::Value;
use serve_support::http::call;
use uuid::Uuid;

const ONE_HOLD: (&str, &str) = ("max_holds_per_principal", "1");
const BOUND: Duration = Duration::from_secs(5);
const HOLD_SECONDS: u64 = 25;
/// Long enough for six polls to be parked and the pool to be read while they are.
const PARKED_SECONDS: u64 = 15;

#[tokio::test(flavor = "multi_thread")]
async fn ac1_a_commit_by_device_a_wakes_device_b_within_5_s_with_the_latest_state_and_the_new_head()
{
    // Arrange
    let pipeline = Pipeline::start(&[ONE_HOLD, ("feed_safety_recheck", "30s")]).await;
    let folder = pipeline.folder("ac1");
    let device_b = pipeline.stack.device_token(&pipeline.account).await;
    let cursor = pipeline.head_as(&device_b);
    let poll = pipeline.park(&device_b, &cursor, HOLD_SECONDS);
    let file = File::new("from-a.txt", 5000, 101);

    // Act
    let sent = Instant::now();
    let (node, revision) = pipeline.publish_small(folder, &file, None);
    let (woken, at) = answer(poll, WITHIN);

    // Assert
    let page = Page::of(&woken);
    assert!(
        at.duration_since(sent) < BOUND,
        "woken after {:?}",
        at.duration_since(sent)
    );
    assert_eq!(page.items.len(), 1, "{:?}", page.items);
    let item = &page.items[0];
    let current = pipeline.node(node);
    assert_eq!(id(item, "node_id"), node);
    assert_eq!(id(item, "revision_id"), revision);
    assert_eq!(item["state"], "live");
    assert_eq!(item["name"], file.name.as_str());
    assert_eq!(item["size"].as_u64(), Some(file.bytes.len() as u64));
    assert_eq!(item["content_hash"], current["content_hash"]);
    assert_eq!(page.next_cursor, pipeline.head_as(&device_b));
    assert_eq!(
        split(&page.next_cursor).1,
        item["seq"].as_u64().expect("a seq")
    );
    pipeline.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn ac2_with_no_change_a_short_wait_ends_in_an_empty_page_and_parked_polls_hold_no_connection()
{
    // Arrange: a pool of 4 connections, six parked polls of six devices; no auth recheck within the wait.
    let pipeline = Pipeline::start(&[
        ONE_HOLD,
        ("db_pool_max", "4"),
        ("longpoll_auth_recheck", "60s"),
        ("feed_safety_recheck", "60s"),
    ])
    .await;
    let cursor = pipeline.feed_cursor();
    let in_use_idle = pipeline.pool_in_use();
    let mut devices = Vec::new();
    for _ in 0..6 {
        devices.push(pipeline.stack.device_token(&pipeline.account).await);
    }
    let parked: Vec<_> = devices
        .iter()
        .map(|device| pipeline.park(device, &cursor, PARKED_SECONDS))
        .collect();

    // Act
    let in_use_parked = pipeline.pool_in_use();
    let head_while_parked = pipeline.head_as(&pipeline.device);
    let asked = Instant::now();
    let short = pipeline.changes(&pipeline.device, &cursor, Some(2), None);
    let waited = asked.elapsed();

    // Assert
    eprintln!(
        "AC-2: pool in use {in_use_idle} idle, {in_use_parked} with six polls parked (pool max 4)"
    );
    assert!(
        in_use_parked <= in_use_idle,
        "{in_use_parked} in use parked, {in_use_idle} idle"
    );
    assert_eq!(head_while_parked, cursor);
    let page = Page::of(&short);
    assert!(page.items.is_empty(), "{:?}", page.items);
    assert_eq!(page.next_cursor, cursor);
    assert!(!page.has_more);
    assert!(waited >= Duration::from_secs(2), "{waited:?}");
    assert!(waited < Duration::from_secs(2) + BOUND, "{waited:?}");
    for poll in parked {
        let (answered, _) = answer(poll, Duration::from_secs(PARKED_SECONDS) + WITHIN);
        assert_eq!(Page::of(&answered).next_cursor, cursor);
    }
    pipeline.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn ac3_an_unknown_epoch_a_seq_above_head_and_a_seq_below_the_horizon_are_410_with_their_reason()
 {
    // Arrange: two commits, then the purge horizon raised to the head.
    let pipeline = Pipeline::start(&[]).await;
    let folder = pipeline.folder("ac3");
    pipeline.publish_small(folder, &File::new("one.txt", 100, 1), None);
    pipeline.publish_small(folder, &File::new("two.txt", 100, 2), None);
    let (epoch, head) = split(&pipeline.feed_cursor());
    pipeline.raise_horizon_to_head().await;
    let cursors = [
        (format!("{}.{head}", epoch + 1), "epoch_changed"),
        (format!("{epoch}.{}", head + 1), "cursor_ahead"),
        (format!("{epoch}.0"), "below_purge_horizon"),
    ];

    // Act
    let answers: Vec<_> = cursors
        .iter()
        .map(|(cursor, _)| pipeline.changes(&pipeline.device, cursor, Some(1), None))
        .collect();

    // Assert
    for (answered, (cursor, reason)) in answers.iter().zip(&cursors) {
        assert_eq!(answered.status, 410, "{cursor}: {answered:?}");
        let problem = answered.json();
        assert_eq!(problem["code"], "resync_required", "{problem}");
        assert_eq!(problem["reason"], *reason, "{cursor}: {problem}");
    }
    pipeline.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn ac4_revoking_the_session_and_the_device_ends_their_held_polls_with_401_within_5_s() {
    // Arrange
    let pipeline = Pipeline::start(&[ONE_HOLD]).await;
    let session = pipeline.account.token.clone();
    let device_b = pipeline.stack.device_token(&pipeline.account).await;
    let cursor = pipeline.feed_cursor();
    let by_session = pipeline.park(&session, &cursor, HOLD_SECONDS);
    let by_device = pipeline.park(&device_b, &cursor, HOLD_SECONDS);
    let public = pipeline.stack.ports.public;

    // Act
    let revoked = Instant::now();
    let signed_out = call(
        public,
        "DELETE",
        "/api/v1/sessions/current",
        &session,
        &[],
        &[],
    );
    let device_out = call(public, "DELETE", "/api/v1/devices/me", &device_b, &[], &[]);
    let (session_answer, session_at) = answer(by_session, WITHIN);
    let (device_answer, device_at) = answer(by_device, WITHIN);

    // Assert
    assert!((200..300).contains(&signed_out.status), "{signed_out:?}");
    assert!((200..300).contains(&device_out.status), "{device_out:?}");
    for (answered, at) in [(&session_answer, session_at), (&device_answer, device_at)] {
        assert_eq!(answered.status, 401, "{answered:?}");
        assert_eq!(answered.json()["code"], "unauthenticated");
        let after = at.duration_since(revoked);
        assert!(after < BOUND, "401 after {after:?}");
    }
    pipeline.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn ac5_sigterm_returns_held_polls_as_empty_pages_with_their_cursors_before_the_grace_ends() {
    // Arrange: `shutdown_grace` is the stack's 3 s.
    let pipeline = Pipeline::start(&[ONE_HOLD]).await;
    let device_b = pipeline.stack.device_token(&pipeline.account).await;
    let cursor = pipeline.feed_cursor();
    let polls = [
        pipeline.park(&pipeline.account.token, &cursor, HOLD_SECONDS),
        pipeline.park(&pipeline.device, &cursor, HOLD_SECONDS),
        pipeline.park(&device_b, &cursor, HOLD_SECONDS),
    ];
    let Pipeline {
        stack, mut server, ..
    } = pipeline;

    // Act
    let signalled = server.signal("TERM");
    let answers: Vec<_> = polls.into_iter().map(|poll| answer(poll, WITHIN)).collect();
    let exited = server.wait_exit(WITHIN);

    // Assert
    for (answered, at) in &answers {
        let page = Page::of(answered);
        assert!(page.items.is_empty(), "{:?}", page.items);
        assert_eq!(page.next_cursor, cursor);
        let after = at.duration_since(signalled);
        assert!(
            after < Duration::from_secs(3),
            "answered {after:?} after SIGTERM"
        );
    }
    assert_eq!(exited.code, Some(0), "{}", exited.stderr_text());
    stack.finish().await;
}

/// 25 one-block uploads ready to commit and 25 staged small files, each in `folder`.
async fn prepared_commits(pipeline: &Pipeline, folder: Uuid) -> Vec<(String, Vec<u8>)> {
    let mut requests = Vec::new();
    for index in 0..25 {
        let file = LargeFile::new(&format!("large-{index:02}.bin"), 300 * 1024, 200 + index);
        let upload = pipeline.started(folder, &file);
        assert_eq!(pipeline.put_block(upload, &file, 0).status, 204);
        requests.push((
            "/api/v1/commits".to_owned(),
            Pipeline::commit_body(upload, folder, &file),
        ));
    }
    let staged: Vec<File> = (0..25)
        .map(|index| File::new(&format!("small-{index:02}.txt"), 2000, 300 + index))
        .collect();
    for answered in pipeline.stage_all(&staged).await {
        assert_eq!(answered.status, 200, "{answered:?}");
    }
    for file in &staged {
        let body = serde_json::json!({
            "commit_id": Uuid::new_v4(),
            "entries": [entry(file.key, folder, &file.name)],
        });
        requests.push((
            "/api/v1/commits:batch".to_owned(),
            body.to_string().into_bytes(),
        ));
    }
    requests
}

/// The node of a commit answer of either route.
fn committed_node(answered: &Value) -> (Uuid, Uuid) {
    let result = answered["entries"].get(0).unwrap_or(answered);
    (id(result, "node_id"), id(result, "revision_id"))
}

#[tokio::test(flavor = "multi_thread")]
async fn ac69_fifty_concurrent_commits_read_back_from_cursor_0_once_each_gapless_up_to_the_head() {
    // Arrange: large and batch commits interleaved, all sent at once.
    let pipeline = Pipeline::start(&[("max_open_sessions", "64")]).await;
    let folder = pipeline.folder("ac69");
    let mut requests = prepared_commits(&pipeline, folder).await;
    let batch = requests.split_off(25);
    let mixed: Vec<_> = requests
        .into_iter()
        .zip(batch)
        .flat_map(|(a, b)| [a, b])
        .collect();
    let (_, seq_before) = split(&pipeline.feed_cursor());
    let (public, device) = (pipeline.stack.ports.public, pipeline.device.clone());

    // Act
    let committing: Vec<_> = mixed
        .into_iter()
        .map(|(path, body)| {
            let device = device.clone();
            thread::spawn(move || {
                let json = [("Content-Type", "application/json".to_owned())];
                call(public, "POST", &path, &device, &json, &body)
            })
        })
        .collect();
    let committed: Vec<(Uuid, Uuid)> = committing
        .into_iter()
        .map(|commit| {
            let answered = commit.join().expect("the commit thread ends");
            assert_eq!(answered.status, 200, "{answered:?}");
            committed_node(&answered.json())
        })
        .collect();
    let new_device = pipeline.stack.device_token(&pipeline.account).await;
    let head = pipeline.head_as(&new_device);
    let (epoch, _) = split(&head);
    let (items, last) = pipeline.read_to_head(&new_device, &format!("{epoch}.0"), 7);

    // Assert
    assert_eq!(last, head);
    assert_eq!(split(&head).1, seq_before + 50);
    let seqs: Vec<u64> = items
        .iter()
        .map(|item| item["seq"].as_u64().expect("a seq"))
        .collect();
    assert!(seqs.windows(2).all(|pair| pair[0] < pair[1]), "{seqs:?}");
    let nodes: Vec<Uuid> = items.iter().map(|item| id(item, "node_id")).collect();
    assert_eq!(
        nodes.iter().collect::<BTreeSet<_>>().len(),
        nodes.len(),
        "a node repeats"
    );
    let committed_seqs: BTreeSet<u64> = committed
        .iter()
        .map(|(node, revision)| {
            let item = items
                .iter()
                .find(|item| id(item, "node_id") == *node)
                .unwrap_or_else(|| panic!("{node} missing from the feed"));
            assert_eq!(id(item, "revision_id"), *revision);
            assert_eq!(item["state"], "live");
            item["seq"].as_u64().expect("a seq")
        })
        .collect();
    let expected: BTreeSet<u64> = (seq_before + 1..=seq_before + 50).collect();
    assert_eq!(committed_seqs, expected);
    pipeline.finish().await;
}

/// The F7 joins: each operation, the non-test file that calls or registers it, and what that file must contain.
const CALLERS: &[Caller] = &[
    (
        "ChangeFeed::{read, head} from the changes routes, with 01's PrincipalRevalidator passed by 11",
        "components/11_http_api/src/registry/feed/handlers.rs",
        &[
            "&user.principal,",
            "services.principal_revalidator.as_ref(),",
            ".head(user.account)",
        ],
    ),
    (
        "FeedStamp::stamp last in the transaction, FeedNotifier::notify after COMMIT",
        "components/02_tree/src/publish.rs",
        &[
            "self.stamp.stamp(tx, account, touched)",
            "self.notifier.notify(account, seq)",
        ],
    ),
    (
        "the shutdown signal from `serve` to 11, which wakes the held reads",
        "server/src/serve/mod.rs",
        &["_ = terminate.recv() => Stop::Terminate,", ".serve(async {"],
    ),
    (
        "11 cancels the feed token after the listeners stop",
        "components/11_http_api/src/server.rs",
        &["self.feed_shutdown.cancel();"],
    ),
    (
        "one notifier shared by 05's ChangeFeed and 02's TreeService, and the shutdown token of 05",
        "server/src/root/domain.rs",
        &[
            "let notifier = InProcessNotifier::new();",
            "Arc::clone(&notifier),",
            "shutdown.feed.clone(),",
            "feed.notifier.clone(),",
        ],
    ),
    (
        "the routes bound to the feed, the revalidator and the shutdown token",
        "server/src/root/api.rs",
        &[
            "principal_revalidator: identity.authenticator.clone()",
            "change_feed: feed.change_feed.clone()",
            "feed_shutdown: shutdown.feed.clone()",
        ],
    ),
];

#[test]
fn ac6_every_f7_operation_has_a_production_caller_and_one_notifier_is_constructed() {
    // Arrange
    let roots = ["components", "server/src", "shared"];

    // Act
    let missing = missing_callers(CALLERS);
    let constructions = production_occurrences(&roots, "InProcessNotifier::new()");

    // Assert
    assert!(missing.is_empty(), "{missing:#?}");
    assert_eq!(constructions, ["server/src/root/domain.rs"]);
}
