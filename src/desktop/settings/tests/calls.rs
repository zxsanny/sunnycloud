//! AC-2 (framing and matching) and AC-6 (typed errors) against the stub daemon.

mod common;

use std::time::Instant;

use ipc_schema::{AccountView, ControlError, Settings};
use sunnycloud_settings::daemon_client::ClientError;
use sunnycloud_settings_testkit::script::{Answer, EventSchedule, Hostile, Script};

use common::{WAIT, session, status_with_skipped};

fn settings(bandwidth_limit: u64) -> Settings {
    Settings {
        bandwidth_limit,
        hash_throttle: 0.5,
        autostart: true,
        tray: false,
    }
}

#[tokio::test]
async fn three_concurrent_calls_get_their_own_answers_by_id_while_events_arrive_in_order() {
    let mut signed_in = status_with_skipped(0);
    signed_in.account = Some(AccountView {
        account_id: "acct-1".to_owned(),
    });
    let script = Script {
        status: Some(signed_in),
        settings: Some(settings(777)),
        events: Some(EventSchedule {
            interval_ms: 60,
            statuses: (1..=3).map(status_with_skipped).collect(),
        }),
        ..Script::default()
    }
    .with_answers(
        "get_settings",
        vec![Answer::Delay {
            ms: 300,
            then: Box::new(Answer::Default),
        }],
    )
    .with_answers(
        "list_pairs",
        vec![Answer::Delay {
            ms: 150,
            then: Box::new(Answer::Default),
        }],
    );
    let mut s = session(script).await;
    s.client.subscribe().await.expect("subscribe succeeds");
    let started = Instant::now();

    let slow = async {
        let result = s.client.get_settings().await;
        (result, started.elapsed())
    };
    let middle = async {
        let result = s.client.list_pairs().await;
        (result, started.elapsed())
    };
    let fast = async {
        let result = s.client.status().await;
        (result, started.elapsed())
    };
    let ((settings_result, slow_at), (pairs, middle_at), (status, fast_at)) =
        tokio::join!(slow, middle, fast);

    assert_eq!(settings_result.unwrap(), settings(777));
    assert!(pairs.unwrap().is_empty());
    assert_eq!(status.unwrap().skipped_count, 0);
    assert!(
        fast_at < middle_at && middle_at < slow_at,
        "answers must arrive out of request order: {fast_at:?} {middle_at:?} {slow_at:?}"
    );
    let mut counts = Vec::new();
    for _ in 0..3 {
        let event = tokio::time::timeout(WAIT, s.events.recv())
            .await
            .expect("an event arrives")
            .expect("the event stream is open");
        counts.push(event.status.skipped_count);
    }
    assert_eq!(counts, [1, 2, 3]);
}

#[tokio::test]
async fn events_are_delivered_between_answers_of_a_busy_connection() {
    let script = Script {
        events: Some(EventSchedule {
            interval_ms: 20,
            statuses: (1..=3).map(status_with_skipped).collect(),
        }),
        ..Script::default()
    }
    .with_answers(
        "get_settings",
        vec![Answer::Delay {
            ms: 200,
            then: Box::new(Answer::Default),
        }],
    );
    let mut s = session(script).await;
    s.client.subscribe().await.unwrap();

    let slow = s.client.get_settings().await;
    let first = s
        .events
        .try_recv()
        .expect("an event arrived during the call");

    assert!(slow.is_ok());
    assert_eq!(first.status.skipped_count, 1);
}

#[tokio::test]
async fn every_control_error_variant_arrives_typed_with_its_fields() {
    let variants = [
        ControlError::HttpsRequired,
        ControlError::ServerUnreachable,
        ControlError::UpgradeRequired {
            message: "update to 1.2".to_owned(),
        },
        ControlError::SignedIn,
        ControlError::NotSignedIn,
        ControlError::ServerNotSet,
        ControlError::UnknownFolder,
        ControlError::RootInvalid,
        ControlError::RootNested,
        ControlError::RulesInvalid { line: 3 },
        ControlError::CloudManagedRoot,
        ControlError::AlreadyPaired,
        ControlError::UnknownPair,
        ControlError::SettingInvalid {
            key: "bandwidth_limit".to_owned(),
        },
        ControlError::NoSuchHold,
        ControlError::Internal {
            correlation_id: "c-77".to_owned(),
        },
    ];
    let script = Script::default().with_answers(
        "status",
        variants.iter().cloned().map(Answer::Error).collect(),
    );
    let s = session(script).await;

    for expected in variants {
        let error = s.client.status().await.unwrap_err();

        assert_eq!(error, ClientError::Control(expected));
    }
}

#[tokio::test]
async fn an_unknown_error_kind_becomes_a_generic_error() {
    // The first call of a client has id 1 (documented), which the raw answer repeats.
    let answer = r#"{"jsonrpc":"2.0","id":1,"error":{"code":-32000,"message":"Future","data":{"kind":"FromTheFuture"}}}"#;
    let script = Script::default().with_answers(
        "status",
        vec![Answer::Hostile(Hostile::Raw(answer.to_owned()))],
    );
    let s = session(script).await;

    let error = s.client.status().await.unwrap_err();

    assert_eq!(
        error,
        ClientError::Rpc {
            code: -32000,
            message: "Future".to_owned()
        }
    );
}

#[tokio::test]
async fn a_scripted_error_does_not_break_the_next_call() {
    let script = Script::default().with_answers(
        "set_server",
        vec![Answer::Error(ControlError::HttpsRequired)],
    );
    let s = session(script).await;

    let refused = s.client.set_server("http://x".to_owned()).await;
    let accepted = s
        .client
        .set_server("https://cloud.example".to_owned())
        .await;

    assert_eq!(
        refused,
        Err(ClientError::Control(ControlError::HttpsRequired))
    );
    assert_eq!(accepted, Ok(()));
    assert_eq!(s.stub.count("set_server"), 2);
}

#[tokio::test]
async fn set_settings_returns_the_applied_values() {
    let s = session(Script::default()).await;
    let patch = ipc_schema::SettingsPatch {
        bandwidth_limit: Some(2048),
        hash_throttle: None,
        autostart: None,
        tray: None,
    };

    let applied = s.client.set_settings(patch).await.unwrap();

    assert_eq!(applied.bandwidth_limit, 2048);
    assert_eq!(s.client.get_settings().await.unwrap(), applied);
}
