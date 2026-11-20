//! `GET /api/v1/realtime` (`realtime.md`) through the real `sunnycloud serve` binary on PostgreSQL 18: live and
//! resumed `Change` frames of real commits, two sockets of one account, the Origin rule on cookie sessions, the
//! per-account cap and its coupling to the hold cap, the resets, revocation, malformed input and SIGTERM.
//! The timing limits and the slow consumer are `11_http_api`'s `it realtime` tests.

mod pipeline_support;
mod realtime_support;
mod root_support;
mod serve_support;

use std::time::Duration;

use pipeline_support::feed::{Page, split};
use pipeline_support::{File, Pipeline, WITHIN, entries};
use realtime_support::{Frame, Socket, ping, subscribe};
use serve_support::http::call;
use tokio_tungstenite::tungstenite::protocol::Message;
use uuid::Uuid;

/// A socket of the pipeline's account, subscribed at its feed head.
async fn live(pipeline: &Pipeline) -> Socket {
    let (epoch, seq) = split(&pipeline.feed_cursor());
    Socket::subscribed(
        pipeline.stack.ports.public,
        &pipeline.account.token,
        epoch,
        seq,
    )
    .await
}

#[tokio::test(flavor = "multi_thread")]
async fn rt01_a_commit_arrives_as_a_change_frame_equal_to_the_node_and_to_the_changes_item() {
    // Arrange
    let pipeline = Pipeline::start(&[]).await;
    let folder = pipeline.folder("rt01");
    let before = pipeline.feed_cursor();
    let mut socket = live(&pipeline).await;

    // Act
    let (node, revision) = pipeline.publish_small(folder, &File::new("one.txt", 5000, 1), None);
    let changes = socket.changes_until_end().await;
    let page = Page::of(&pipeline.changes(&pipeline.device, &before, None, None));

    // Assert
    let change = changes.last().expect("a change");
    let item = page
        .items
        .iter()
        .find(|item| item["node_id"] == node.to_string())
        .expect("the item");
    let json = pipeline.node(node);
    assert_eq!(change.node_id, node);
    assert_eq!(change.revision_id, Some(revision));
    assert_eq!((change.name.as_str(), change.size), ("one.txt", Some(5000)));
    assert_eq!(change.parent_id, Some(folder));
    assert!(!change.folder && !change.deleted && change.end);
    assert_eq!(change.size, json["size"].as_u64());
    assert_eq!(change.seq, item["seq"].as_u64().expect("seq"));
    assert_eq!(
        change.content_hash.as_deref(),
        item["content_hash"].as_str()
    );
    assert_eq!(change.name, item["name"].as_str().expect("name"));
    pipeline.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn rt02_a_resumed_socket_gets_the_missed_commits_in_order_and_nothing_older() {
    // Arrange
    let pipeline = Pipeline::start(&[]).await;
    let folder = pipeline.folder("rt02");
    let mut first = live(&pipeline).await;
    pipeline.publish_small(folder, &File::new("a.txt", 100, 1), None);
    let seen = first.changes_until_end().await;
    let resume = seen.last().expect("a change");
    let (epoch, seq) = (resume.epoch, resume.seq);
    drop(first);
    pipeline.publish_small(folder, &File::new("b.txt", 100, 2), None);
    pipeline.publish_small(folder, &File::new("c.txt", 100, 3), None);

    // Act
    let mut second = Socket::subscribed(
        pipeline.stack.ports.public,
        &pipeline.account.token,
        epoch,
        seq,
    )
    .await;
    let b = second.changes_until_end().await;
    let c = second.changes_until_end().await;

    // Assert
    let names: Vec<_> = b
        .iter()
        .chain(&c)
        .map(|change| change.name.as_str())
        .collect();
    assert_eq!(names, ["b.txt", "c.txt"]);
    assert!(b[0].seq < c[0].seq && b[0].seq > seq);
    pipeline.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn rt03_a_batch_commit_is_one_seq_and_only_its_last_frame_ends_it() {
    // Arrange
    let pipeline = Pipeline::start(&[]).await;
    let folder = pipeline.folder("rt03");
    let mut socket = live(&pipeline).await;
    let files: Vec<File> = (0..3)
        .map(|i| File::new(&format!("f{i}.txt"), 50 + i, 10 + i as u64))
        .collect();
    for answer in pipeline.stage_all(&files).await {
        assert_eq!(answer.status, 200, "{answer:?}");
    }

    // Act
    let committed = pipeline.commit_batch(Uuid::new_v4(), &entries(&files, folder));
    let batch = socket.changes_until_end().await;

    // Assert
    assert_eq!(committed.status, 200, "{committed:?}");
    assert!(batch.len() >= 3, "{batch:?}");
    assert!(batch.iter().all(|change| change.seq == batch[0].seq));
    assert!(batch[..batch.len() - 1].iter().all(|change| !change.end));
    pipeline.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn rt04_both_sockets_of_an_account_get_the_frame_and_another_account_gets_none() {
    // Arrange
    let pipeline = Pipeline::start(&[]).await;
    let folder = pipeline.folder("rt04");
    let mut one = live(&pipeline).await;
    let mut two = live(&pipeline).await;
    let stranger = pipeline.stack.account().await;
    let (epoch, seq) = split(&pipeline.head_as(&stranger.token));
    let mut other =
        Socket::subscribed(pipeline.stack.ports.public, &stranger.token, epoch, seq).await;

    // Act
    pipeline.publish_small(folder, &File::new("shared.txt", 10, 1), None);
    let (a, b) = (one.changes_until_end().await, two.changes_until_end().await);
    let silence = tokio::time::timeout(Duration::from_secs(1), other.next()).await;

    // Assert
    assert_eq!(a.last().expect("a").name, "shared.txt");
    assert_eq!(a, b);
    assert!(silence.is_err(), "the other account received {silence:?}");
    pipeline.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn rt05_a_cookie_handshake_needs_the_public_origin() {
    // Arrange
    let pipeline = Pipeline::start(&[]).await;
    let public = pipeline.stack.ports.public;
    let token = &pipeline.account.token;
    let cookie = ("Cookie", format!("{}={token}", realtime_support::COOKIE));
    let wrong = [
        vec![cookie.clone()],
        vec![
            cookie.clone(),
            ("Origin", "https://evil.example".to_owned()),
        ],
        vec![cookie.clone(), ("Origin", "null".to_owned())],
        vec![
            cookie.clone(),
            ("Origin", "https://app.example.org".to_owned()),
        ],
    ];

    // Act
    let mut refused = Vec::new();
    for headers in &wrong {
        refused.push(Socket::open(public, headers).await.err());
    }
    let right = Socket::open(public, &Socket::web_headers(token)).await;

    // Assert
    for answer in refused {
        let answer = answer.expect("refused");
        assert_eq!(
            (answer.status, answer.code()),
            (403, "csrf_rejected".to_owned())
        );
    }
    assert!(right.is_ok());
    pipeline.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn rt06_revoking_the_session_closes_its_sockets_with_4401_within_5_s() {
    // Arrange
    let pipeline = Pipeline::start(&[]).await;
    let mut socket = live(&pipeline).await;
    tokio::time::sleep(Duration::from_millis(300)).await;

    // Act
    let started = std::time::Instant::now();
    let signed_out = call(
        pipeline.stack.ports.public,
        "DELETE",
        "/api/v1/sessions/current",
        &pipeline.account.token,
        &[],
        &[],
    )
    .status;
    let code = socket.close_code().await;

    // Assert
    assert!((200..300).contains(&signed_out), "{signed_out}");
    assert_eq!(code, Some(4401));
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "{:?}",
        started.elapsed()
    );
    pipeline.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn rt07_the_ninth_socket_of_an_account_is_429_until_one_closes() {
    // Arrange
    let pipeline = Pipeline::start(&[]).await;
    let public = pipeline.stack.ports.public;
    let token = pipeline.account.token.clone();
    let mut open = Vec::new();
    for _ in 0..8 {
        open.push(Socket::subscribed(public, &token, 1, 0).await);
    }

    // Act
    let ninth = Socket::open(public, &Socket::web_headers(&token))
        .await
        .err();
    drop(open.pop());
    let mut retried = None;
    for _ in 0..100 {
        retried = Socket::open(public, &Socket::web_headers(&token))
            .await
            .ok();
        if retried.is_some() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    // Assert
    let ninth = ninth.expect("refused");
    assert_eq!(
        (ninth.status, ninth.code()),
        (429, "rate_limited".to_owned())
    );
    assert!(retried.is_some());
    pipeline.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn rt07b_a_hold_cap_below_the_socket_cap_closes_the_socket_past_it_with_1013() {
    // Arrange: each subscribed socket parks one hold of its session; `max_holds_per_principal` is 2.
    let pipeline = Pipeline::start(&[("max_holds_per_principal", "2")]).await;
    let public = pipeline.stack.ports.public;
    let token = pipeline.account.token.clone();
    let (epoch, seq) = split(&pipeline.feed_cursor());
    let _one = Socket::subscribed(public, &token, epoch, seq).await;
    let _two = Socket::subscribed(public, &token, epoch, seq).await;
    tokio::time::sleep(Duration::from_millis(500)).await;

    // Act
    let mut third = Socket::subscribed(public, &token, epoch, seq).await;
    let code = third.close_code().await;

    // Assert
    assert_eq!(code, Some(1013));
    pipeline.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn rt08_an_epoch_a_seq_ahead_and_a_seq_below_the_horizon_are_resets_and_a_new_subscribe_works()
 {
    // Arrange
    let pipeline = Pipeline::start(&[]).await;
    let folder = pipeline.folder("rt08");
    pipeline.publish_small(folder, &File::new("one.txt", 10, 1), None);
    pipeline.publish_small(folder, &File::new("two.txt", 10, 2), None);
    let (epoch, head) = split(&pipeline.feed_cursor());
    pipeline.raise_horizon_to_head().await;
    let public = pipeline.stack.ports.public;
    let mut socket = Socket::web(public, &pipeline.account.token).await;
    socket.frame().await;

    // Act
    let mut resets = Vec::new();
    for (e, s) in [(epoch + 1, head), (epoch, head + 1), (epoch, 0)] {
        socket.send(&subscribe(e, s)).await;
        resets.push(socket.frame().await);
    }
    socket.send(&subscribe(epoch, head)).await;
    pipeline.publish_small(folder, &File::new("three.txt", 10, 3), None);
    let after = socket.changes_until_end().await;

    // Assert
    let reset = |reason| Frame::Reset {
        reason,
        epoch,
        seq: head,
    };
    assert_eq!(resets, [reset(1), reset(3), reset(2)]);
    assert_eq!(after.last().expect("a change").name, "three.txt");
    pipeline.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn rt09_an_unknown_frame_closes_1002_text_1003_and_a_long_message_1009_while_others_go_on() {
    // Arrange
    let pipeline = Pipeline::start(&[]).await;
    let public = pipeline.stack.ports.public;
    let token = pipeline.account.token.clone();
    let mut keeper = Socket::web(public, &token).await;
    keeper.frame().await;
    let mut unknown = Socket::web(public, &token).await;
    let mut text = Socket::web(public, &token).await;
    let mut long = Socket::web(public, &token).await;
    for socket in [&mut unknown, &mut text, &mut long] {
        socket.frame().await;
    }

    // Act
    unknown.send(&[0x7e, 0]).await;
    text.send_raw(Message::Text("hi".into())).await;
    long.send(&vec![0x01; 4096]).await;
    let codes = [
        unknown.close_code().await,
        text.close_code().await,
        long.close_code().await,
    ];
    keeper.send(&ping(3)).await;

    // Assert
    assert_eq!(codes, [Some(1002), Some(1003), Some(1009)]);
    assert_eq!(keeper.frame().await, Frame::Pong { nonce: 3 });
    pipeline.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn rt14_sigterm_closes_the_sockets_with_1001_before_the_grace_ends() {
    // Arrange
    let pipeline = Pipeline::start(&[]).await;
    let mut socket = live(&pipeline).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    let pipeline_support::Pipeline {
        stack, mut server, ..
    } = pipeline;

    // Act
    let signalled = server.signal("TERM");
    let code = socket.close_code().await;
    let exited = server.wait_exit(WITHIN);

    // Assert
    assert_eq!(code, Some(1001));
    assert!(signalled.elapsed() < Duration::from_secs(5));
    assert_eq!(exited.code, Some(0), "{}", exited.stderr_text());
    stack.finish().await;
}
