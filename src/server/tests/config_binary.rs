//! AC-2, AC-3, AC-4 and AC-5 of AZ-3790 against the real binary: exit 3, nothing listening, no database
//! connection, every problem named, no secret in the output.

mod config_support;

use std::net::{SocketAddr, TcpListener, TcpStream};
use std::process::{Command, Output, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use config_support::{PLANTED, Setup};

const WITHIN: Duration = Duration::from_secs(10);
const PROBE_TIMEOUT: Duration = Duration::from_millis(50);

/// A loopback port nobody listens on: the fake "database" the binary must never dial and the two listener
/// addresses it must never open before it refuses.
struct Watch {
    database: TcpListener,
    listeners: Vec<SocketAddr>,
}

impl Watch {
    fn new() -> Self {
        let database = TcpListener::bind("127.0.0.1:0").expect("a loopback port is free");
        database
            .set_nonblocking(true)
            .expect("the listener can be polled");
        let listeners = (0..2)
            .map(|_| {
                TcpListener::bind("127.0.0.1:0")
                    .expect("a loopback port is free")
                    .local_addr()
                    .expect("bound address")
            })
            .collect();
        Self {
            database,
            listeners,
        }
    }

    fn setup(&self) -> Setup {
        let port = self.database.local_addr().expect("bound address").port();
        let mut setup = Setup::valid(port);
        setup.set("internal_addr", &self.listeners[0].to_string());
        setup.set("ops_addr", &self.listeners[1].to_string());
        setup
    }

    fn database_was_dialled(&self) -> bool {
        self.database.accept().is_ok()
    }
}

struct Run {
    output: Output,
    elapsed: Duration,
    listener_seen: bool,
}

impl Run {
    fn stderr(&self) -> String {
        String::from_utf8(self.output.stderr.clone()).expect("UTF-8")
    }

    fn stdout(&self) -> String {
        String::from_utf8(self.output.stdout.clone()).expect("UTF-8")
    }

    fn code(&self) -> Option<i32> {
        self.output.status.code()
    }
}

/// Runs the binary with exactly `setup.env` while a thread dials the two listener addresses until it exits.
fn run(watch: &Watch, setup: &Setup, args: &[&str]) -> Run {
    let started = Instant::now();
    let child = Command::new(env!("CARGO_BIN_EXE_sunnycloud"))
        .args(args)
        .env_clear()
        .envs(&setup.env)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the binary starts");
    let done = Arc::new(AtomicBool::new(false));
    let seen = Arc::new(AtomicBool::new(false));
    let prober = {
        let (done, seen, addrs) = (
            Arc::clone(&done),
            Arc::clone(&seen),
            watch.listeners.clone(),
        );
        thread::spawn(move || {
            while !done.load(Ordering::Acquire) {
                for addr in &addrs {
                    if TcpStream::connect_timeout(addr, PROBE_TIMEOUT).is_ok() {
                        seen.store(true, Ordering::Release);
                    }
                }
            }
        })
    };
    let output = child.wait_with_output().expect("the binary exits");
    let elapsed = started.elapsed();
    done.store(true, Ordering::Release);
    prober.join().expect("the prober ends");
    Run {
        output,
        elapsed,
        listener_seen: seen.load(Ordering::Acquire),
    }
}

fn assert_refused_before_connecting(watch: &Watch, run: &Run) {
    assert_eq!(run.code(), Some(3), "{}", run.stderr());
    assert!(run.elapsed < WITHIN, "{:?}", run.elapsed);
    assert!(!run.listener_seen, "a listener opened before the refusal");
    assert!(!watch.database_was_dialled(), "the database was dialled");
    assert!(run.stdout().is_empty(), "{}", run.stdout());
    assert!(!run.stderr().contains(PLANTED), "{}", run.stderr());
}

#[test]
fn serve_refuses_an_unknown_variable_naming_it() {
    // Arrange
    let watch = Watch::new();
    let mut setup = watch.setup();
    setup.set("bogus", "1");

    // Act
    let run = run(&watch, &setup, &["serve"]);

    // Assert
    assert_refused_before_connecting(&watch, &run);
    assert!(
        run.stderr().contains("SUNNYCLOUD_BOGUS"),
        "{}",
        run.stderr()
    );
}

#[test]
fn serve_refuses_a_retention_above_ninety_days_naming_the_range() {
    // Arrange
    let watch = Watch::new();
    let mut setup = watch.setup();
    setup.set("retention", "91d");

    // Act
    let run = run(&watch, &setup, &["serve"]);

    // Assert
    assert_refused_before_connecting(&watch, &run);
    let stderr = run.stderr();
    assert!(
        stderr.contains("retention") && stderr.contains("0s to 90d"),
        "{stderr}"
    );
}

#[test]
fn serve_refuses_a_start_without_any_sign_in_provider() {
    // Arrange
    let watch = Watch::new();
    let mut setup = watch.setup();
    setup.remove("github_client_id");
    setup.without_secret("provider_github_client_secret");

    // Act
    let run = run(&watch, &setup, &["serve"]);

    // Assert
    assert_refused_before_connecting(&watch, &run);
    assert!(run.stderr().contains("providers"), "{}", run.stderr());
}

#[test]
fn serve_lists_three_problems_of_three_sections_in_one_refusal() {
    // Arrange
    let watch = Watch::new();
    let mut setup = watch.setup();
    setup.set("refresh_reuse_grace", "5m");
    setup.set("retention", "91d");
    setup.set("share_wrong_password_limit", "0");

    // Act
    let run = run(&watch, &setup, &["serve"]);

    // Assert
    assert_refused_before_connecting(&watch, &run);
    let stderr = run.stderr();
    assert!(stderr.contains("3 problems"), "{stderr}");
    for key in [
        "refresh_reuse_grace",
        "retention",
        "share_wrong_password_limit",
    ] {
        assert!(stderr.contains(key), "{key} missing from {stderr}");
    }
}

#[test]
fn migrate_without_the_migrator_secret_exits_3_naming_it_and_dials_nothing() {
    // Arrange: the API container's environment, which never mounts `db_migrator_url`.
    let watch = Watch::new();
    let setup = watch.setup();

    // Act
    let run = run(&watch, &setup, &["migrate"]);

    // Assert
    assert_refused_before_connecting(&watch, &run);
    assert!(run.stderr().contains("db_migrator_url"), "{}", run.stderr());
}

#[test]
fn a_valid_configuration_gets_past_the_configuration_stage() {
    // Arrange
    let watch = Watch::new();
    let mut setup = watch.setup();
    setup.secret(
        "db_migrator_url",
        "postgres://migrator@127.0.0.1:1/sunnycloud",
    );

    // Act
    let serve = run(&watch, &setup, &["serve"]);
    let migrate = run(&watch, &setup, &["migrate"]);

    // Assert: both stop at the command whose task has not landed (exit 2), not at configuration (exit 3).
    assert_eq!(serve.code(), Some(2), "{}", serve.stderr());
    assert_eq!(migrate.code(), Some(2), "{}", migrate.stderr());
}

#[test]
fn a_refusal_with_a_planted_database_url_prints_it_nowhere() {
    // Arrange
    let watch = Watch::new();
    let mut setup = watch.setup();
    setup.set("retention", "91d");

    // Act
    let run = run(&watch, &setup, &["--json", "serve"]);

    // Assert
    assert_refused_before_connecting(&watch, &run);
    assert!(!run.stdout().contains(PLANTED) && !run.stderr().contains(PLANTED));
}

#[test]
fn commands_that_use_no_provider_worker_token_or_master_key_get_past_configuration() {
    // Arrange: only the migrator secret, the one thing `migrate` reads from the API's table.
    let watch = Watch::new();
    let mut setup = Setup::empty();
    setup.secret(
        "db_migrator_url",
        "postgres://migrator@127.0.0.1:1/sunnycloud",
    );

    for args in [
        &["migrate"][..],
        &["admin", "setup-token"],
        &["key-check"],
        &["openapi"],
    ] {
        // Act
        let run = run(&watch, &setup, args);

        // Assert: the command whose task has not landed stops at "not implemented" (2), never at configuration (3).
        assert_eq!(run.code(), Some(2), "{args:?}: {}", run.stderr());
        assert!(!run.stderr().contains("invalid configuration"), "{args:?}");
        assert!(
            !run.stderr().contains("warning"),
            "{args:?}: {}",
            run.stderr()
        );
    }
    let version = run(&watch, &setup, &["version"]);
    assert_eq!(version.code(), Some(0), "{}", version.stderr());
    assert!(version.stderr().is_empty(), "{}", version.stderr());
}

#[test]
fn serve_names_every_missing_secret_in_one_refusal() {
    // Arrange
    let watch = Watch::new();
    let mut setup = watch.setup();
    for secret in [
        "db_api_url",
        "master_key_file",
        "share_grant_key",
        "worker_api_token",
    ] {
        setup.without_secret(secret);
    }

    // Act
    let run = run(&watch, &setup, &["serve"]);

    // Assert
    assert_eq!(run.code(), Some(3), "{}", run.stderr());
    for secret in [
        "db_api_url",
        "master_key_file",
        "share_grant_key",
        "worker_api_token",
    ] {
        assert!(
            run.stderr().contains(secret),
            "{secret} missing: {}",
            run.stderr()
        );
    }
}
