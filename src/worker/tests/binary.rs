//! The real `sunnycloud-worker` process: AC-2 (logs), AC-3 (ops endpoints), AC-4 (healthcheck) and the exit codes.

use std::process::Output;

use support::{Running, TOKEN, TokenFile, busy_port, env_for, free_port, http_get, worker_command};

mod support;

const MARKER: &str = "Zq7fK2mXw9PaB4nTc8RvL1sYh6JdE3uGo5NiA0xCkMbWtQyV";

fn text(bytes: &[u8]) -> String {
    String::from_utf8(bytes.to_vec()).expect("UTF-8")
}

fn describe(output: &Output) -> String {
    format!(
        "exit {:?}\nstdout: {}\nstderr: {}",
        output.status.code(),
        text(&output.stdout),
        text(&output.stderr)
    )
}

#[test]
fn ac3_the_ops_endpoints_answer_on_the_configured_address() {
    let worker = Running::start(TOKEN);

    let (healthz, _) = http_get(worker.addr, "/healthz");
    let (readyz, ready_body) = http_get(worker.addr, "/readyz");
    let (metrics, metrics_body) = http_get(worker.addr, "/metrics");

    assert_eq!((healthz, readyz, metrics), (200, 200, 200));
    assert!(ready_body.contains(r#""status":"ok""#), "{ready_body}");
    assert!(
        metrics_body.contains("sunnycloud_build_info"),
        "{metrics_body}"
    );
}

#[test]
fn ac2_the_bearer_appears_in_no_output_of_a_running_worker() {
    let mut worker = Running::start(MARKER);
    let bodies: Vec<String> = ["/healthz", "/readyz", "/metrics"]
        .into_iter()
        .map(|path| http_get(worker.addr, path).1)
        .collect();

    let output = worker.stop();

    assert_eq!(output.status.code(), Some(0), "{}", describe(&output));
    let stdout = text(&output.stdout);
    assert!(stdout.contains("worker started"), "{stdout}");
    assert!(stdout.contains(r#""component":"worker""#), "{stdout}");
    for seen in bodies.iter().chain([&stdout, &text(&output.stderr)]) {
        assert!(!seen.contains(MARKER), "the bearer leaked: {seen}");
    }
}

#[test]
fn ac4_healthcheck_exits_0_for_a_running_worker_and_non_zero_for_a_stopped_one() {
    let mut worker = Running::start(TOKEN);
    let running = worker.healthcheck();

    let stopped_output = worker.stop();
    let stopped = worker.healthcheck();

    assert_eq!(
        running.status.code(),
        Some(0),
        "{} / worker: {}",
        describe(&running),
        describe(&stopped_output)
    );
    assert_eq!(stopped_output.status.code(), Some(0));
    assert_eq!(stopped.status.code(), Some(1), "{}", describe(&stopped));
    assert!(text(&stopped.stderr).contains("not ready"));
}

#[test]
fn sigterm_stops_the_worker_cleanly_and_closes_the_ops_port() {
    let mut worker = Running::start(TOKEN);
    let addr = worker.addr;

    let output = worker.stop();

    assert_eq!(output.status.code(), Some(0), "{}", describe(&output));
    assert!(std::net::TcpStream::connect(addr).is_err());
}

#[test]
fn ac1_the_worker_refuses_to_start_naming_every_problem() {
    let token = TokenFile::new(TOKEN);
    let mut env = env_for(free_port(), &token);
    env.remove("SUNNYCLOUD_WORKER_API_TOKEN_FILE");
    env.insert(
        "SUNNYCLOUD_WORKER_STATUS_INTERVAL".to_owned(),
        "soon".to_owned(),
    );

    let output = worker_command(&env, &[]).output().expect("starts");

    assert_eq!(output.status.code(), Some(3), "{}", describe(&output));
    let stderr = text(&output.stderr);
    assert!(stderr.contains("worker_status_interval"), "{stderr}");
    assert!(stderr.contains("worker_api_token"), "{stderr}");
    assert!(output.stdout.is_empty());
}

#[test]
fn an_unknown_argument_exits_3() {
    let token = TokenFile::new(TOKEN);
    let env = env_for(free_port(), &token);

    let output = worker_command(&env, &["serve"]).output().expect("starts");

    assert_eq!(output.status.code(), Some(3), "{}", describe(&output));
    assert!(text(&output.stderr).contains("usage:"));
}

#[test]
fn a_busy_ops_port_is_a_runtime_failure() {
    let blocker = busy_port();
    let port = blocker.local_addr().expect("address").port();
    let token = TokenFile::new(TOKEN);
    let env = env_for(port, &token);

    let output = worker_command(&env, &[]).output().expect("starts");

    assert_eq!(output.status.code(), Some(2), "{}", describe(&output));
    assert!(text(&output.stderr).contains("cannot bind the ops listener"));
}
