//! AZ-4334: the test clock of `serve`. With feature `test-clock` the real binary moves its time through the `:9091`
//! listener and a scheduler decision follows it (AC-1), and the process holds one `OffsetClock` (AC-2). A default
//! build has no test-clock listener, key or route (AC-3).

mod root_support;
mod serve_support;

use std::time::Duration;

use serve_support::http::{Response, call};

const WITHIN: Duration = Duration::from_secs(10);
const OFFSET_PATH: &str = "/internal/v1/test/clock-offset";
/// The worker token of the root's configuration (`config_support`).
const WORKER_TOKEN: &str = "worker-token";

/// `GET /internal/v1/test/clock-offset` with the worker bearer.
fn read_offset(stack: &serve_support::Stack) -> Response {
    let headers = [("X-Worker-Id", "clock-reader".to_owned())];
    call(
        stack.ports.internal,
        "GET",
        OFFSET_PATH,
        WORKER_TOKEN,
        &headers,
        b"",
    )
}

#[cfg(feature = "test-clock")]
mod test_build {
    use std::sync::Arc;
    use std::thread;
    use std::time::Instant;

    use sc_clock::{Clock, OffsetClock};
    use serde_json::{Value, json};
    use sqlx::Connection;
    use uuid::Uuid;

    use super::serve_support::Stack;
    use super::serve_support::http::{call, janitor_cycles, send};
    use super::*;
    use crate::root_support::address;

    const TWO_HOURS: u64 = 7200;
    const POLL_EVERY: Duration = Duration::from_millis(200);

    fn json_type() -> (&'static str, String) {
        ("Content-Type", "application/json".to_owned())
    }

    fn offset_json(seconds: u64) -> Value {
        json!({"offset_seconds": seconds})
    }

    fn reaper_cycles(stack: &Stack) -> u64 {
        janitor_cycles(stack.ports.ops, "session_reaper")
    }

    /// Polls `done` every 200 ms; panics past [`WITHIN`].
    fn wait_until(mut done: impl FnMut() -> bool) {
        let deadline = Instant::now() + WITHIN;
        while !done() {
            assert!(Instant::now() < deadline, "not reached within {WITHIN:?}");
            thread::sleep(POLL_EVERY);
        }
    }

    async fn session_rows(stack: &Stack, upload: Uuid) -> i64 {
        let mut admin = stack.root.db.admin().await;
        let rows = sqlx::query_scalar("SELECT count(*) FROM upload_sessions WHERE id = $1")
            .bind(upload)
            .fetch_one(&mut admin)
            .await
            .expect("count the upload session");
        admin.close().await.expect("close admin connection");
        rows
    }

    #[tokio::test]
    async fn ac1_an_advance_on_the_test_clock_listener_lets_the_scheduler_reap_a_live_upload_session()
     {
        // Arrange
        let mut stack = Stack::new().await;
        stack
            .root
            .setup
            .set("upload_session_ttl", "1h")
            .set("session_reap_interval", "1s");
        let mut server = stack.spawn();
        server.wait_ready(stack.ports.ops, WITHIN);
        let account = stack.account().await;
        let device = stack.device_token(&account).await;
        let start = json!({
            "parent_id": account.root,
            "name": "clock.bin",
            "declared_size": 4 * 1024 * 1024,
            "client_key": format!("clock-{}", Uuid::new_v4().simple()),
        })
        .to_string();
        let started = call(
            stack.ports.public,
            "POST",
            "/api/v1/uploads",
            &device,
            &[json_type()],
            start.as_bytes(),
        );
        assert_eq!(started.status, 200, "{started:?}");
        let upload = Uuid::parse_str(started.json()["upload_id"].as_str().expect("upload id"))
            .expect("a uuid");
        let before = read_offset(&stack);
        // Two finished reaper cycles on real time leave a session with an hour to live alone.
        let cycles = reaper_cycles(&stack);
        wait_until(|| reaper_cycles(&stack) >= cycles + 2);
        let rows_on_real_time = session_rows(&stack, upload).await;
        let body = json!({"advance_seconds": TWO_HOURS}).to_string();

        // Act
        let advanced = send(
            stack.ports.test_clock,
            "POST",
            "/test/clock",
            &[json_type()],
            body.as_bytes(),
        );
        let deadline = Instant::now() + WITHIN;
        let mut rows_after = session_rows(&stack, upload).await;
        while rows_after != 0 && Instant::now() < deadline {
            tokio::time::sleep(POLL_EVERY).await;
            rows_after = session_rows(&stack, upload).await;
        }
        let after = read_offset(&stack);

        // Assert
        assert_eq!((before.status, before.json()), (200, offset_json(0)));
        assert_eq!(rows_on_real_time, 1);
        assert_eq!(
            (advanced.status, advanced.json()),
            (200, offset_json(TWO_HOURS))
        );
        assert_eq!(rows_after, 0, "the reaper did not follow the moved clock");
        assert_eq!((after.status, after.json()), (200, offset_json(TWO_HOURS)));
        server.signal("TERM");
        let exited = server.wait_exit(Duration::from_secs(30));
        assert_eq!(exited.code, Some(0), "{}", exited.stderr_text());
        stack.finish().await;
    }

    #[tokio::test]
    async fn ac2_the_process_clock_every_component_reads_is_the_offset_clock_the_routes_move() {
        // Arrange
        let env = sunnycloud_server::process_env();

        // Act
        let shared: Arc<dyn Clock> = env.services.get::<dyn Clock>().await.expect("dyn Clock");
        let offset = env
            .services
            .get::<OffsetClock>()
            .await
            .expect("OffsetClock");
        let before = env.clock.now_utc();
        offset.advance_seconds(3600).expect("advance");
        let after = shared.now_utc();

        // Assert
        assert_eq!(address(&env.clock), address(&offset));
        assert_eq!(address(&shared), address(&offset));
        let moved = after
            .checked_sub(Duration::from_secs(3600))
            .expect("in range");
        assert!(moved >= before, "{before:?} then {after:?}");
    }
}

#[cfg(not(feature = "test-clock"))]
mod release_build {
    use std::net::TcpListener;

    use super::serve_support::Stack;
    use super::*;

    /// The address a `-test` build binds by default (`http_server.md`, test-clock listener row).
    const DEFAULT_TEST_CLOCK: &str = "0.0.0.0:9091";

    #[tokio::test]
    async fn ac3_serve_starts_with_the_test_clock_port_taken_and_the_clock_offset_route_is_404() {
        // Arrange
        let stack = Stack::new().await;
        // Taken by this test or already by another process (`AddrInUse`): a build that bound it would refuse the
        // start. Any other bind failure would leave the premise unproven, so it fails the test.
        let held = match TcpListener::bind(DEFAULT_TEST_CLOCK) {
            Ok(listener) => Some(listener),
            Err(error) if error.kind() == std::io::ErrorKind::AddrInUse => None,
            Err(error) => panic!("the default test-clock address cannot be held: {error}"),
        };
        let mut server = stack.spawn();

        // Act
        server.wait_ready(stack.ports.ops, WITHIN);
        let read = read_offset(&stack);

        // Assert
        assert_eq!(read.status, 404, "{read:?}");
        drop(held);
        server.signal("TERM");
        let exited = server.wait_exit(Duration::from_secs(30));
        assert_eq!(exited.code, Some(0), "{}", exited.stderr_text());
        stack.finish().await;
    }

    #[tokio::test]
    async fn ac3_a_test_clock_address_is_an_unknown_key_and_no_listener_opens() {
        // Arrange
        let mut stack = Stack::new().await;
        let test_clock = stack.ports.test_clock.to_string();
        stack.root.setup.set("test_clock_addr", &test_clock);

        // Act
        let refused = stack.refused();

        // Assert
        assert_eq!(
            refused.exited.code,
            Some(3),
            "{}",
            refused.exited.stderr_text()
        );
        assert!(!refused.accepted, "a listener opened before the refusal");
        assert!(
            refused
                .exited
                .stderr_text()
                .contains("SUNNYCLOUD_TEST_CLOCK_ADDR"),
            "{}",
            refused.exited.stderr_text()
        );
        stack.finish().await;
    }
}
