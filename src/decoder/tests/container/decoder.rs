//! One decoder container started with the production flags of `deploy/compose/compose.dev.yml`, with
//! the socket and scratch directories on the host so that the harness plays the supervisor.
//!
//! The runner runs as `10003:10003` with the host group as a supplementary group and as
//! `decoder_gid`, so the socket it creates is reachable by the harness and the harness's job
//! directories are readable and writable by the runner through that one group.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use media_shared::decoder_protocol::{Request, Response};
use serde_json::Value;
use tempfile::TempDir;

use super::docker::{State, docker, docker_ok, processes_of, state_of};
use super::flags::assert_production_flags;
use super::host::{remove_tree, seccomp_profile};
use super::peer::Peer;

pub const READY: Duration = Duration::from_secs(30);
const GIB: u64 = 1 << 30;
/// Test config of the spec (`tests.md`, Blackbox Tests); a case may override a key.
const TEST_CONFIG: [(&str, &str); 4] = [
    ("SUNNYCLOUD_DECODER_STALL_KILL", "3s"),
    ("SUNNYCLOUD_DECODER_ALIVE_INTERVAL", "500ms"),
    ("SUNNYCLOUD_DECODER_WATCHDOG", "3s"),
    ("SUNNYCLOUD_DECODER_MEMORY_POLL_INTERVAL", "100ms"),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flavor {
    /// `decoder-interactive`: photo limits 2 CPU / 2 GiB, tmpfs scratch.
    Interactive,
    /// `decoder-general`: 4 CPU / 4 GiB, disk scratch.
    General,
}

impl Flavor {
    fn name(self) -> &'static str {
        match self {
            Flavor::Interactive => "interactive",
            Flavor::General => "general",
        }
    }

    pub fn cpus(self) -> u64 {
        match self {
            Flavor::Interactive => 2,
            Flavor::General => 4,
        }
    }

    pub fn memory_bytes(self) -> u64 {
        match self {
            Flavor::Interactive => 2 * GIB,
            Flavor::General => 4 * GIB,
        }
    }

    /// Where the scratch directory lives: a RAM-backed directory for the interactive decoder, the
    /// ordinary temporary directory (a disk in CI) for the general one.
    fn base(self) -> PathBuf {
        match self {
            Flavor::Interactive => PathBuf::from("/dev/shm"),
            Flavor::General => std::env::temp_dir(),
        }
    }
}

/// `$DECODER_IMAGE`: the release image under test.
pub fn release_image() -> String {
    std::env::var("DECODER_IMAGE")
        .unwrap_or_else(|_| panic!("needs DECODER_IMAGE (a decoder image) and docker"))
}

/// `$DECODER_TEST_IMAGE`, else `<DECODER_IMAGE>-test`: the `test-stubs` variant of the same image.
pub fn stub_image() -> String {
    std::env::var("DECODER_TEST_IMAGE").unwrap_or_else(|_| format!("{}-test", release_image()))
}

/// A job directory the harness made on the host; the runner sees it as `/scratch/<id>`.
#[derive(Debug, Clone)]
pub struct Job {
    pub id: String,
    pub path: PathBuf,
}

impl Job {
    /// The `job_dir` of a request.
    pub fn dir(&self) -> String {
        format!("/scratch/{}", self.id)
    }

    pub fn source(&self) -> PathBuf {
        self.path.join("in/source")
    }

    pub fn out(&self, name: &str) -> PathBuf {
        self.path.join("out").join(name)
    }

    pub fn work(&self) -> PathBuf {
        self.path.join("work")
    }
}

pub struct Decoder {
    pub name: String,
    pub image: String,
    pub flavor: Flavor,
    pub scratch: PathBuf,
    sock: PathBuf,
    gid: u32,
    jobs: Vec<Job>,
    next: u32,
    root: Option<TempDir>,
}

static CONTAINERS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

impl Decoder {
    /// Starts a container, inspects it against the production flags (AC-1: a mismatch panics
    /// before any case runs) and waits until it answers `hello`.
    pub fn start(image: &str, flavor: Flavor, env: &[(&str, &str)]) -> Self {
        let root = TempDir::new_in(flavor.base()).expect("scratch base directory");
        let (sock_dir, scratch) = (root.path().join("sock"), root.path().join("scratch"));
        for dir in [root.path(), &sock_dir, &scratch] {
            if dir != root.path() {
                fs::create_dir(dir).expect("create a volume directory");
            }
            fs::set_permissions(dir, fs::Permissions::from_mode(0o770)).expect("volume mode");
        }
        fs::create_dir(scratch.join("_stub")).expect("create _stub");
        fs::set_permissions(scratch.join("_stub"), fs::Permissions::from_mode(0o770))
            .expect("_stub mode");
        let gid = fs::metadata(root.path())
            .expect("stat the volume root")
            .gid();
        let number = CONTAINERS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let name = format!("sc-h3-{}-{number}", std::process::id());
        let decoder = Self {
            name,
            image: image.to_owned(),
            flavor,
            sock: sock_dir.join("decoder.sock"),
            scratch,
            gid,
            jobs: Vec::new(),
            next: 0,
            root: Some(root),
        };
        decoder.run(&sock_dir, env);
        assert_production_flags(
            &decoder.name,
            decoder.flavor.name(),
            decoder.flavor.cpus(),
            decoder.flavor.memory_bytes(),
        );
        decoder.wait_ready(READY);
        decoder
    }

    fn run(&self, sock_dir: &Path, env: &[(&str, &str)]) {
        let seccomp = format!("seccomp={}", seccomp_profile().display());
        let mut args: Vec<String> = [
            "run",
            "--detach",
            "--name",
            &self.name,
            "--label",
            "sunnycloud-suite=h3",
            "--network",
            "none",
            "--read-only",
            "--cap-drop",
            "ALL",
            "--security-opt",
            "no-new-privileges:true",
            "--security-opt",
            &seccomp,
            "--init",
            "--restart",
            "unless-stopped",
            "--pids-limit",
            &crate::flags::PIDS_LIMIT.to_string(),
            "--cpus",
            &self.flavor.cpus().to_string(),
            "--memory",
            &self.flavor.memory_bytes().to_string(),
            "--memory-swap",
            &self.flavor.memory_bytes().to_string(),
            "--user",
            "10003:10003",
            "--group-add",
            &self.gid.to_string(),
            "--volume",
            &format!("{}:/run/decoder", sock_dir.display()),
            "--volume",
            &format!("{}:/scratch", self.scratch.display()),
            "--env",
            &format!("SUNNYCLOUD_DECODER_GID={}", self.gid),
        ]
        .map(str::to_owned)
        .into();
        let mut config: Vec<(String, String)> = TEST_CONFIG
            .iter()
            .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
            .collect();
        for (key, value) in env {
            config.retain(|(known, _)| known != key);
            config.push(((*key).to_owned(), (*value).to_owned()));
        }
        for (key, value) in config {
            args.extend(["--env".into(), format!("{key}={value}")]);
        }
        args.push(self.image.clone());
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        docker_ok(&refs);
    }

    pub fn state(&self) -> State {
        state_of(&self.name)
    }

    /// A connection that has answered `hello`, within `within`.
    pub fn wait_ready(&self, within: Duration) -> Peer {
        let deadline = Instant::now() + within;
        loop {
            if let Some(peer) = self.try_hello() {
                return peer;
            }
            assert!(
                Instant::now() < deadline,
                "{} did not answer hello within {within:?}; logs:\n{}",
                self.name,
                self.logs()
            );
            thread::sleep(Duration::from_millis(100));
        }
    }

    fn try_hello(&self) -> Option<Peer> {
        let mut peer = Peer::new(UnixStream::connect(&self.sock).ok()?);
        peer.send(Request::Hello { protocol: 1 });
        match peer.try_response(Duration::from_secs(2)) {
            Some(Response::Capabilities(_)) => Some(peer),
            _ => None,
        }
    }

    /// A connection to the current runner; panics when none is listening.
    pub fn connect(&self) -> Peer {
        Peer::new(UnixStream::connect(&self.sock).expect("connect to the decoder socket"))
    }

    /// Waits for Docker to restart the container after `before` was taken, then for `hello`.
    pub fn wait_restarted(&self, before: &State, within: Duration) -> Peer {
        let deadline = Instant::now() + within;
        loop {
            let now = self.state();
            if now.running && now.started_at != before.started_at {
                return self.wait_ready(
                    deadline
                        .saturating_duration_since(Instant::now())
                        .max(Duration::from_secs(1)),
                );
            }
            assert!(
                Instant::now() < deadline,
                "{} was not restarted within {within:?}: {now:?}",
                self.name
            );
            thread::sleep(Duration::from_millis(100));
        }
    }

    pub fn logs(&self) -> String {
        let output = docker(&["logs", &self.name]);
        format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    }

    /// Sends `signal` (`USR1`, `STOP`, `CONT`) to the runner from inside the container. `docker kill` is not
    /// used: any `docker kill` marks the container as stopped by hand, and `restart: unless-stopped` then
    /// does not restart it (tests.md IT-10-18 names `docker kill --signal=USR1`; see the report).
    pub fn signal_runner(&self, signal: &str) {
        let find = "for p in /proc/[0-9]*; do read -r c < $p/comm; \
            if [ \"$c\" = decoder-runner ]; then echo ${p#/proc/}; fi; done";
        let pid = docker_ok(&["exec", &self.name, "sh", "-c", find]);
        docker_ok(&[
            "exec",
            &self.name,
            "sh",
            "-c",
            &format!("kill -s {signal} {pid}"),
        ]);
    }

    pub fn waiter(&self) -> super::docker::Waiter {
        super::docker::Waiter::start(&self.name)
    }

    /// A job directory with `in/` and `out/` and no input yet.
    pub fn empty_job(&mut self) -> Job {
        self.next += 1;
        let id = format!("job-{}", self.next);
        self.empty_job_named(&id)
    }

    /// The id a later job will have, for a stub that must know the next job's directory.
    pub fn reserve_id(&mut self) -> String {
        self.next += 1;
        format!("job-{}", self.next)
    }

    pub fn empty_job_named(&mut self, id: &str) -> Job {
        let path = self.scratch.join(id);
        fs::create_dir(&path).expect("create the job directory");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o770)).expect("job mode");
        for (name, mode) in [("in", 0o750), ("out", 0o770)] {
            fs::create_dir(path.join(name)).expect("create in/ or out/");
            fs::set_permissions(path.join(name), fs::Permissions::from_mode(mode))
                .expect("in/ or out/ mode");
        }
        let job = Job {
            id: id.to_owned(),
            path,
        };
        self.jobs.push(job.clone());
        job
    }

    /// A job whose `in/source` holds `content` (mode 0440, the shared group).
    pub fn job(&mut self, content: &[u8]) -> Job {
        let job = self.empty_job();
        write_source(&job, content);
        job
    }

    pub fn job_named(&mut self, id: &str, content: &[u8]) -> Job {
        let job = self.empty_job_named(id);
        write_source(&job, content);
        job
    }

    /// The JSON lines the stub wrote for a job that had no marker (argv, environment, limits).
    pub fn stub_log(&self, job: &Job) -> Vec<Value> {
        let path = self.scratch.join("_stub").join(format!("{}.jsonl", job.id));
        fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("no stub log {}: {error}", path.display()))
            .lines()
            .map(|line| serde_json::from_str(line).expect("a stub log line is json"))
            .collect()
    }

    /// The host pid of the runner process of this container (from `docker top`).
    pub fn runner_pid(&self) -> u32 {
        processes_of(&self.name)
            .unwrap_or_default()
            .iter()
            .find(|row| row.comm == "decoder-runner")
            .unwrap_or_else(|| panic!("no runner in {}", self.name))
            .pid
    }

    /// AC-7 at the end of a test group: the runner left no `work/` behind, the harness removes its job
    /// trees, nothing else is left on the scratch volume, and only the runner (and the init that
    /// forwards signals to it) runs in the container.
    pub fn finish(mut self) {
        let jobs = std::mem::take(&mut self.jobs);
        let state = self.state();
        assert!(
            state.running,
            "{} is not running at the end of the group",
            self.name
        );
        for job in &jobs {
            assert!(
                !job.work().exists(),
                "work/ of {} was left after the group",
                job.id
            );
        }
        for job in &jobs {
            remove_tree(&job.path, &self.image);
        }
        remove_tree(&self.scratch.join("_stub"), &self.image);
        let left: Vec<String> = fs::read_dir(&self.scratch)
            .expect("read the scratch volume")
            .filter_map(Result::ok)
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        assert!(
            left.is_empty(),
            "scratch is not empty after the group: {left:?}"
        );
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let rows = processes_of(&self.name).unwrap_or_default();
            let runners = rows
                .iter()
                .filter(|row| row.comm == "decoder-runner")
                .count();
            let strangers: Vec<&str> = rows
                .iter()
                .map(|row| row.comm.as_str())
                .filter(|comm| *comm != "decoder-runner" && *comm != "docker-init")
                .collect();
            if runners == 1 && strangers.is_empty() {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "processes after the group: {rows:?}"
            );
            thread::sleep(Duration::from_millis(100));
        }
    }
}

impl Drop for Decoder {
    fn drop(&mut self) {
        let removed = docker(&["rm", "--force", "--volumes", &self.name]);
        if !removed.status.success() {
            eprintln!(
                "cannot remove container {}: {}",
                self.name,
                String::from_utf8_lossy(&removed.stderr)
            );
        }
        if let Some(root) = self.root.take() {
            remove_tree(root.path(), &self.image);
        }
    }
}

fn write_source(job: &Job, content: &[u8]) {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o440)
        .open(job.source())
        .expect("create in/source");
    file.write_all(content).expect("write in/source");
}
