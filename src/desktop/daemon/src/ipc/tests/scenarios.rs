//! Acceptance scenarios that run unchanged over an in-memory stream and a real socket.

use std::io::ErrorKind;
use std::time::Duration;

use ipc_schema::*;
use serde_json::json;
use tokio::io::{AsyncRead, AsyncWrite};

use super::fake::{Client, FakeDaemon, TOKEN, error_code};

fn pair(id: i64) -> PairIdParams {
    PairIdParams {
        pair_id: PairId(id),
    }
}

fn node() -> NodeId {
    NodeId::parse("0190f3b0-7c2e-7a41-9d5e-2b6c8a1f4e30").unwrap()
}

/// One call of every method of `DaemonControl`, `subscribe` first.
fn every_method() -> Vec<MethodCall> {
    let entries = ListEntriesParams {
        pair_id: None,
        page: None,
    };
    vec![
        MethodCall::Subscribe(NoParams {}),
        MethodCall::Status(NoParams {}),
        MethodCall::SetServer(SetServerParams {
            url: "https://cloud.example".to_owned(),
        }),
        MethodCall::ListServerFolders(ListServerFoldersParams {
            parent: None,
            page: None,
        }),
        MethodCall::ListPairs(NoParams {}),
        MethodCall::AddPair(PairSpec {
            target: PairTarget::NodeId { node_id: node() },
            local_root: "/home/u/Other".to_owned(),
            rules_text: String::new(),
        }),
        MethodCall::UpdatePair(UpdatePairParams {
            pair_id: PairId(1),
            patch: PairPatch {
                rules_text: Some("*.bak\n".to_owned()),
            },
        }),
        MethodCall::Pause(pair(1)),
        MethodCall::Resume(pair(1)),
        MethodCall::ListSkipped(entries.clone()),
        MethodCall::ListEscaped(entries),
        MethodCall::GetSettings(NoParams {}),
        MethodCall::SetSettings(SettingsPatch {
            bandwidth_limit: Some(500),
            hash_throttle: None,
            autostart: None,
            tray: Some(false),
        }),
        MethodCall::ResolveHold(ResolveHoldParams {
            pair_id: PairId(1),
            decision: HoldDecision::RestoreFromServer,
        }),
        MethodCall::LoginStart(LoginStartParams {
            device_name: "laptop".to_owned(),
        }),
        MethodCall::LoginStatus(NoParams {}),
        MethodCall::Logout(NoParams {}),
        MethodCall::RemovePair(pair(1)),
    ]
}

/// AC-4: every method, a pause and a resume, and a status change from the daemon side are
/// recorded; the token string occurs nowhere in what the server sent.
pub async fn token_never_on_the_wire<S>(client: &mut Client<S>, daemon: &FakeDaemon)
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let calls = every_method();
    assert_eq!(
        calls.len(),
        METHODS.len(),
        "the session must cover every method"
    );

    for call in calls {
        let name = call.name();
        client.call(call).await;
        if name == "login_status" {
            daemon.emit();
        }
    }
    client.wait_for_events(3).await;

    let wire = String::from_utf8(client.received.clone()).unwrap();
    assert!(
        wire.lines().count() >= 21,
        "responses and events were recorded"
    );
    assert_eq!(wire.matches(TOKEN).count(), 0);
    assert!(!wire.to_lowercase().contains("token"));
}

/// AC-5: bad JSON, an unknown method, a `PairSpec` with two targets and a 10 MiB line.
pub async fn malformed_input<S>(a: &mut Client<S>, b: &mut Client<S>)
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    a.send_raw(b"this is not json\n").await.unwrap();
    let parse = a.next_response().await.unwrap();
    assert_eq!(parse.id, None);
    assert_eq!(error_code(&parse), PARSE_ERROR_CODE);

    a.send_raw(br#"{"jsonrpc":"2.0","id":7,"method":"get_token"}"#)
        .await
        .unwrap();
    a.send_raw(b"\n").await.unwrap();
    let unknown = a.next_response().await.unwrap();
    assert_eq!(unknown.id, Some(RequestId::Number(7)));
    assert_eq!(error_code(&unknown), METHOD_NOT_FOUND_CODE);

    let two_targets = json!({
        "jsonrpc": "2.0", "id": 8, "method": "add_pair",
        "params": {
            "target": {
                "NodeId": {"node_id": node()},
                "Path": {"server_path": "/Docs", "create_if_missing": true}
            },
            "local_root": "/home/u/X", "rules_text": ""
        }
    });
    a.send_raw(format!("{two_targets}\n").as_bytes())
        .await
        .unwrap();
    let invalid = a.next_response().await.unwrap();
    assert_eq!(invalid.id, Some(RequestId::Number(8)));
    assert_eq!(error_code(&invalid), INVALID_PARAMS_CODE);

    // The connection survived all three.
    assert!(matches!(
        a.call(MethodCall::Status(NoParams {})).await.outcome,
        Outcome::Success { .. }
    ));

    let mut huge = vec![b'x'; 10 * 1024 * 1024];
    huge.push(b'\n');
    if let Err(error) = a.send_raw(&huge).await {
        assert!(
            matches!(
                error.kind(),
                ErrorKind::BrokenPipe | ErrorKind::ConnectionReset
            ),
            "unexpected write error {error:?}"
        );
    }
    // Closing the offending connection is allowed; it must end, not hang.
    while a.next_message().await.is_some() {}

    // Other connections keep working.
    let status = b.call(MethodCall::Status(NoParams {})).await;
    assert!(matches!(status.outcome, Outcome::Success { .. }));
}

/// AC-6: two subscribed connections both receive the pause and the resume event, each
/// with the full status.
pub async fn events_to_subscribers<S>(a: &mut Client<S>, b: &mut Client<S>)
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    a.call(MethodCall::Subscribe(NoParams {})).await;
    b.call(MethodCall::Subscribe(NoParams {})).await;

    a.call(MethodCall::Pause(pair(1))).await;
    a.call(MethodCall::Resume(pair(1))).await;

    a.wait_for_events(2).await;
    b.wait_for_events(2).await;
    for client in [&*a, &*b] {
        assert_eq!(client.events.len(), 2);
        let states: Vec<PairState> = client
            .events
            .iter()
            .map(|e| e.status.pairs[0].state)
            .collect();
        assert_eq!(states, [PairState::Paused, PairState::Active]);
        for event in &client.events {
            let full = &event.status;
            assert_eq!(full.server_url.as_deref(), Some("https://cloud.example"));
            assert_eq!(full.skipped_count, 2);
            assert_eq!(full.pairs[0].local_root, "/home/u/Sync");
        }
    }
}

/// Waits (bounded) until `count` subscribers are registered with the daemon.
pub async fn wait_for_subscribers(daemon: &FakeDaemon, count: usize) {
    for _ in 0..500 {
        if daemon.subscriber_count() >= count {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("{count} subscribers never registered");
}
