//! AC-2: the record file holds every received message as one JSON line, readable while FD runs and complete after
//! FD is killed; and the binary binds its socket as the real decoder does.

mod support;

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

use media_shared::decoder_protocol::Request;
use serde_json::json;
use sunnycloud_fake_decoder::{Config, FakeDecoder, Script};
use support::{
    Client, JobDir, WAIT, hello, kinds, probe, record_lines, render, self_check, wait_until,
};
use tempfile::TempDir;

struct Paths {
    dir: TempDir,
}

impl Paths {
    fn new() -> Self {
        Self {
            dir: tempfile::tempdir().expect("temporary folder"),
        }
    }

    fn socket(&self) -> PathBuf {
        self.dir.path().join("decoder.sock")
    }

    fn record(&self) -> PathBuf {
        self.dir.path().join("record.jsonl")
    }

    fn script(&self, script: &serde_json::Value) -> PathBuf {
        let path = self.dir.path().join("script.json");
        fs::write(&path, script.to_string()).expect("script file");
        path
    }
}

#[tokio::test]
async fn every_request_is_one_line_readable_while_fd_still_runs() {
    // Arrange
    let paths = Paths::new();
    let script = Script::parse(r#"{"render_still": {"default": "silent"}}"#).expect("parses");
    let decoder = FakeDecoder::serve(Config {
        script,
        record: Some(paths.record()),
        ..Config::new(&paths.socket())
    })
    .expect("FD binds");
    let job = JobDir::new("0193b2a4-job");
    let mut client = Client::connect(&paths.socket()).await;

    // Act
    let mut counts = Vec::new();
    for request in [hello(), self_check(), probe(&job)] {
        client.send(request).await;
        client.reply().await;
        counts.push(record_lines(&paths.record()).len());
    }
    client.send(render(&job, 1440)).await;
    client.send(Request::Cancel).await;
    wait_until("the cancel line", || {
        record_lines(&paths.record()).len() == 5
    })
    .await;

    // Assert
    assert_eq!(counts, [1, 2, 3]);
    let lines = record_lines(&paths.record());
    assert_eq!(
        kinds(&lines),
        ["hello", "self_check", "probe", "render_still", "cancel"]
    );
    assert_eq!(lines[0]["fields"], json!({"protocol": 2}));
    assert_eq!(
        lines[1]["fields"],
        json!({"forbidden_targets": ["db:5432"]})
    );
    assert_eq!(lines[3]["job"], "0193b2a4-job");
    assert_eq!(lines[3]["out_entries"], 0);
    assert_eq!(lines[3]["fields"]["max_edge"], 1440);
    assert_eq!(lines[3]["fields"]["job_dir"], job.dir());
    assert_eq!(lines[4]["fields"], json!({}));
    assert!(lines.iter().all(|line| line["conn"] == 1));
    let times: Vec<u64> = lines
        .iter()
        .map(|line| line["at_ms"].as_u64().expect("at_ms"))
        .collect();
    assert!(times.windows(2).all(|pair| pair[0] <= pair[1]), "{times:?}");
    assert_eq!(decoder.records().len(), 5);
}

#[tokio::test]
async fn an_existing_record_file_is_appended_to_not_truncated() {
    // Arrange
    let paths = Paths::new();
    fs::write(paths.record(), "{\"kind\":\"earlier\"}\n").expect("an earlier record");
    let _decoder = FakeDecoder::serve(Config {
        record: Some(paths.record()),
        ..Config::new(&paths.socket())
    })
    .expect("FD binds");
    let mut client = Client::connect(&paths.socket()).await;

    // Act
    client.send(hello()).await;
    client.reply().await;

    // Assert
    assert_eq!(kinds(&record_lines(&paths.record())), ["earlier", "hello"]);
}

/// The built binary; stdin closed, stderr kept for the failure message.
fn spawn(paths: &Paths, script: &serde_json::Value) -> Child {
    Command::new(env!("CARGO_BIN_EXE_fake-decoder"))
        .arg("--socket")
        .arg(paths.socket())
        .arg("--script")
        .arg(paths.script(script))
        .arg("--record")
        .arg(paths.record())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the binary starts")
}

/// Polls until a connect to the socket succeeds.
async fn connect(child: &mut Child, socket: &Path) -> Client {
    let give_up = Instant::now() + WAIT;
    while std::os::unix::net::UnixStream::connect(socket).is_err() {
        let exited = child.try_wait().expect("the child's state");
        assert!(exited.is_none(), "FD exited before it listened: {exited:?}");
        assert!(Instant::now() < give_up, "FD never listened");
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    Client::connect(socket).await
}

/// Polls until the child exits; kills it and fails after [`WAIT`].
async fn exited(mut child: Child) -> Output {
    let give_up = Instant::now() + WAIT;
    while child.try_wait().expect("the child's state").is_none() {
        if Instant::now() >= give_up {
            child.kill().expect("the child can be killed");
            panic!("FD did not exit: {:?}", child.wait_with_output());
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    child.wait_with_output().expect("the child is reaped")
}

#[tokio::test]
async fn the_record_holds_every_request_after_fd_is_killed() {
    // Arrange
    let paths = Paths::new();
    let mut child = spawn(&paths, &json!({"render_still": {"default": "silent"}}));
    let job = JobDir::new("job-1");
    let mut client = connect(&mut child, &paths.socket()).await;
    for request in [hello(), self_check(), probe(&job)] {
        client.send(request).await;
        client.reply().await;
    }
    client.send(render(&job, 320)).await;
    client.send(Request::Cancel).await;
    wait_until("the last line", || record_lines(&paths.record()).len() == 5).await;

    // Act
    child.kill().expect("SIGKILL");
    let output = exited(child).await;

    // Assert
    assert!(!output.status.success(), "{output:?}");
    assert_eq!(
        kinds(&record_lines(&paths.record())),
        ["hello", "self_check", "probe", "render_still", "cancel"]
    );
}

#[tokio::test]
async fn sigterm_ends_fd_with_exit_code_0() {
    // Arrange
    let paths = Paths::new();
    let mut child = spawn(&paths, &json!({}));
    connect(&mut child, &paths.socket()).await;

    // Act
    let killed = Command::new("kill")
        .args(["-TERM", &child.id().to_string()])
        .stdin(Stdio::null())
        .status()
        .expect("kill runs");
    let output = exited(child).await;

    // Assert
    assert!(killed.success());
    assert!(output.status.success(), "{output:?}");
    assert!(
        paths.record().exists(),
        "the record file is created at start"
    );
}

#[tokio::test]
async fn the_binary_replaces_a_stale_socket_and_sets_mode_0660() {
    // Arrange
    let paths = Paths::new();
    drop(UnixListener::bind(paths.socket()).expect("a stale socket"));
    // A child another test forks at this moment holds the listener until its `exec`: stale means refused.
    wait_until("the stale socket to refuse", || {
        std::os::unix::net::UnixStream::connect(paths.socket())
            .is_err_and(|error| error.kind() == std::io::ErrorKind::ConnectionRefused)
    })
    .await;
    let mut child = spawn(&paths, &json!({}));

    // Act
    let mut client = connect(&mut child, &paths.socket()).await;
    client.send(hello()).await;
    let reply = client.reply().await;
    child.kill().expect("SIGKILL");
    exited(child).await;

    // Assert
    assert!(reply.is_some());
    let mode = fs::metadata(paths.socket())
        .expect("socket")
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(mode, 0o660);
}

#[tokio::test]
async fn the_binary_refuses_a_file_that_is_not_a_socket_and_leaves_it() {
    // Arrange
    let paths = Paths::new();
    fs::write(paths.socket(), b"not a socket").expect("a plain file");

    // Act
    let output = exited(spawn(&paths, &json!({}))).await;

    // Assert
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert_eq!(
        fs::read(paths.socket()).expect("still there"),
        b"not a socket"
    );
}

#[tokio::test]
async fn the_binary_refuses_a_bad_script_with_exit_code_1() {
    // Arrange
    let paths = Paths::new();

    // Act
    let output = exited(spawn(&paths, &json!({"probe": {"default": "explode"}}))).await;

    // Assert
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("the script is not valid"),
        "{output:?}"
    );
    assert!(!paths.socket().exists(), "no socket without a valid script");
}

#[tokio::test]
async fn the_binary_refuses_a_missing_argument_with_exit_code_2() {
    // Act
    let output = exited(
        Command::new(env!("CARGO_BIN_EXE_fake-decoder"))
            .arg("--socket")
            .arg("/nonexistent/decoder.sock")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .expect("the binary starts"),
    )
    .await;

    // Assert
    assert_eq!(output.status.code(), Some(2), "{output:?}");
}
