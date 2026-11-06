//! AC-5: a trusted endpoint that sends hostile answers.

mod common;

use std::time::{Duration, Instant};

use ipc_schema::ControlError;
use sunnycloud_settings::daemon_client::{ClientError, ConnectionState};
use sunnycloud_settings_testkit::script::{Answer, Hostile, Script};
use sunnycloud_settings_testkit::stub::MARKUP;
use tracing::Level;

use common::{Captured, WAIT, session_with};

const TIMEOUT: Duration = Duration::from_millis(500);

fn hostile(method: &str, hostile: Hostile) -> Script {
    Script::default().with_answers(method, vec![Answer::Hostile(hostile)])
}

#[tokio::test]
async fn a_10_mb_frame_is_rejected_and_the_client_reconnects_and_stays_usable() {
    let captured = Captured::default();
    let _guard = captured.install();
    let s = session_with(hostile("status", Hostile::oversize_10mb()), |c| {
        c.request_timeout = Duration::from_secs(5);
    })
    .await;
    let started = Instant::now();

    let error = s.client.status().await.unwrap_err();

    assert!(matches!(error, ClientError::Protocol(_)), "got {error:?}");
    assert!(started.elapsed() < Duration::from_secs(5));
    assert_eq!(captured.count(Level::ERROR, "protocol violation"), 1);
    let mut state = s.client.connection();
    tokio::time::timeout(WAIT, state.wait_for(|st| *st == ConnectionState::Connected))
        .await
        .unwrap()
        .unwrap();
    assert!(s.client.status().await.is_ok());
    assert_eq!(s.stub.count("status"), 2);
}

#[tokio::test]
async fn invalid_json_is_logged_and_dropped_without_closing_the_connection() {
    let captured = Captured::default();
    let _guard = captured.install();
    let s = session_with(hostile("status", Hostile::InvalidJson), |c| {
        c.request_timeout = TIMEOUT;
    })
    .await;
    let started = Instant::now();

    let error = s.client.status().await.unwrap_err();

    assert_eq!(error, ClientError::Timeout);
    assert!(started.elapsed() < TIMEOUT * 4);
    assert_eq!(captured.count(Level::ERROR, "protocol violation"), 1);
    assert_eq!(*s.client.connection().borrow(), ConnectionState::Connected);
    assert!(s.client.status().await.is_ok());
}

#[tokio::test]
async fn an_answer_with_a_wrong_id_reaches_no_caller() {
    let captured = Captured::default();
    let _guard = captured.install();
    let s = session_with(hostile("status", Hostile::WrongId), |c| {
        c.request_timeout = TIMEOUT;
    })
    .await;

    let error = s.client.status().await.unwrap_err();

    assert_eq!(error, ClientError::Timeout);
    assert_eq!(captured.count(Level::ERROR, "protocol violation"), 1);
    assert!(s.client.status().await.is_ok());
}

#[tokio::test]
async fn markup_in_error_text_is_passed_on_unchanged_as_plain_data() {
    let captured = Captured::default();
    let _guard = captured.install();
    let s = session_with(hostile("set_server", Hostile::MarkupError), |_| {}).await;

    let error = s
        .client
        .set_server("https://x".to_owned())
        .await
        .unwrap_err();

    assert_eq!(MARKUP, "<b>x</b>{@html}<script>");
    assert_eq!(
        error,
        ClientError::Control(ControlError::UpgradeRequired {
            message: MARKUP.to_owned()
        })
    );
    assert_eq!(captured.count(Level::ERROR, "protocol violation"), 0);
}

#[tokio::test]
async fn a_result_of_the_wrong_shape_is_a_protocol_error_not_a_panic() {
    let script = Script::default().with_answers(
        "status",
        vec![Answer::Hostile(Hostile::Raw(
            r#"{"jsonrpc":"2.0","id":1,"result":{"not":"a status"}}"#.to_owned(),
        ))],
    );
    let captured = Captured::default();
    let _guard = captured.install();
    let s = session_with(script, |_| {}).await;

    let error = s.client.status().await.unwrap_err();

    assert!(matches!(error, ClientError::Protocol(_)), "got {error:?}");
    assert_eq!(captured.count(Level::ERROR, "protocol violation"), 1);
}
