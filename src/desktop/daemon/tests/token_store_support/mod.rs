//! IT-14-18 contexts (14 §5, spike AZ-3885). keyring 4.2.0 decides its store once per process, so each context
//! runs one ignored child test of this binary in its own process: a cleared environment, a throwaway home and
//! runtime dir, and a private `dbus-run-session` bus (or none). The user's keyring and bus are never reachable,
//! no display is set so no real prompt can appear, and the fake token travels on stdin only.

use std::fmt::Write as _;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tempfile::TempDir;
use tracing::field::{Field, Visit};
use tracing::span::{Attributes, Id, Record};
use tracing::subscriber::DefaultGuard;
use tracing::{Event, Level, Metadata, Subscriber};

/// Set only by the parent: a child test that does not see it does nothing.
pub const CHILD_ENV: &str = "SUNNYCLOUD_TOKEN_STORE_CHILD";
/// The account the children store a token for.
pub const ACCOUNT: &str = "0190d5a2-6b1c-7a00-8000-0000000000a1";
const KEYRING_PASSWORD: &str = "throwaway-test-keyring-password";
const CHILD_LIMIT: Duration = Duration::from_secs(60);

/// What the child's private session offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bus {
    /// No `DBUS_SESSION_BUS_ADDRESS` and no `$XDG_RUNTIME_DIR/bus`.
    None,
    /// A private bus that cannot activate any service: no Secret Service.
    WithoutSecretService,
    /// gnome-keyring unlocked with a throwaway password.
    KeyringUnlocked,
    /// gnome-keyring bus-activated on first use, never unlocked: no usable default keyring.
    KeyringNeverUnlocked,
    /// The login keyring locked; no prompt UI can start.
    KeyringLocked,
    /// The login keyring locked and an unlock prompt that nobody answers.
    KeyringPromptUnanswered,
}

pub struct ChildRun {
    pub ok: bool,
    pub output: String,
}

/// Runs the ignored child test `case` in its own isolated process and returns its result and output.
pub fn run_child(case: &str, bus: Bus, token: &str, state_home: StateHome) -> ChildRun {
    let dirs = TempDir::new().expect("temp dir for the child");
    let home = dirs.path().join("home");
    let runtime = dirs.path().join("run");
    for dir in [&home, &runtime] {
        std::fs::create_dir_all(dir).expect("child dirs");
        set_mode(dir, 0o700);
    }
    let exe = std::env::current_exe().expect("this test binary");
    let mut command = Command::new("env");
    command
        .arg("-i")
        .arg("PATH=/usr/bin:/bin")
        .arg(format!("HOME={}", home.display()))
        .arg(format!("XDG_RUNTIME_DIR={}", runtime.display()))
        .arg(format!("{CHILD_ENV}={case}"));
    if state_home == StateHome::Set {
        command.arg(format!(
            "XDG_STATE_HOME={}",
            dirs.path().join("state").display()
        ));
    }
    let child_test = format!("exec \"$0\" --exact {case} --ignored --nocapture --test-threads=1");
    match bus {
        Bus::None => command
            .arg(exe)
            .args(["--exact", case, "--ignored", "--nocapture"]),
        Bus::WithoutSecretService => {
            let config = bare_bus_config(dirs.path(), &runtime);
            command
                .arg("dbus-run-session")
                .arg(format!("--config-file={}", config.display()))
                .args(["--", "sh", "-c", &child_test])
                .arg(exe)
        }
        _ => {
            let script = format!("{}{child_test}", keyring_setup(bus));
            command
                .args(["dbus-run-session", "--", "sh", "-c", &script])
                .arg(exe)
                .arg(KEYRING_PASSWORD)
                .arg(prompter_path())
        }
    };
    run_with_stdin(command, token)
}

/// Whether the child gets `$XDG_STATE_HOME` (under its temp dir) or must use `~/.local/state`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StateHome {
    Set,
    Unset,
}

/// Shell steps before the child: `$1` is the keyring password, `$2` the silent prompter.
fn keyring_setup(bus: Bus) -> String {
    const UNLOCK: &str = "printf '%s' \"$1\" | gnome-keyring-daemon --unlock --components=secrets || exit 3; \
        printf 'x' | secret-tool store --label=setup setup-entry 1 || exit 3; ";
    const LOCK: &str = "gdbus call --session --dest org.freedesktop.secrets \
        --object-path /org/freedesktop/secrets --method org.freedesktop.Secret.Service.Lock \
        \"['/org/freedesktop/secrets/collection/login']\" || exit 3; ";
    const PROMPTER: &str = "python3 -I \"$2\" < /dev/null > \"$HOME/prompter.out\" & i=0; \
        while ! grep -q ready \"$HOME/prompter.out\" && [ $i -lt 100 ]; do sleep 0.1; i=$((i+1)); done; \
        grep -q ready \"$HOME/prompter.out\" || exit 4; ";
    match bus {
        Bus::KeyringUnlocked => UNLOCK.to_owned(),
        Bus::KeyringLocked => [UNLOCK, LOCK].concat(),
        Bus::KeyringPromptUnanswered => [UNLOCK, LOCK, PROMPTER].concat(),
        Bus::None | Bus::WithoutSecretService | Bus::KeyringNeverUnlocked => String::new(),
    }
}

fn prompter_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/silent_prompter.py")
}

/// A session bus with no service directories: nothing can be activated, so no Secret Service exists.
fn bare_bus_config(dir: &Path, runtime: &Path) -> PathBuf {
    let path = dir.join("bare-session.conf");
    let config = format!(
        "<!DOCTYPE busconfig PUBLIC \"-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN\" \
         \"http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd\">\n<busconfig><type>session</type>\
         <listen>unix:dir={}</listen><auth>EXTERNAL</auth><policy context=\"default\">\
         <allow send_destination=\"*\" eavesdrop=\"true\"/><allow eavesdrop=\"true\"/><allow own=\"*\"/>\
         </policy></busconfig>\n",
        runtime.display()
    );
    std::fs::write(&path, config).expect("bus config");
    path
}

fn run_with_stdin(mut command: Command, token: &str) -> ChildRun {
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start the isolated child");
    child
        .stdin
        .take()
        .expect("child stdin")
        .write_all(token.as_bytes())
        .expect("token on stdin");
    let started = Instant::now();
    loop {
        if let Some(status) = child.try_wait().expect("child status") {
            let mut output = String::new();
            for stream in [
                child.stdout.take().map(|s| Box::new(s) as Box<dyn Read>),
                child.stderr.take().map(|s| Box::new(s) as Box<dyn Read>),
            ]
            .into_iter()
            .flatten()
            {
                let mut text = String::new();
                let mut stream = stream;
                stream.read_to_string(&mut text).expect("child output");
                output.push_str(&text);
            }
            return ChildRun {
                ok: status.success(),
                output,
            };
        }
        if started.elapsed() > CHILD_LIMIT {
            child.kill().expect("kill the hung child");
            panic!("the isolated child did not finish within {CHILD_LIMIT:?}");
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

pub fn set_mode(path: &Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).expect("chmod");
}

/// Reads the fake token the parent wrote to stdin.
pub fn token_from_stdin() -> String {
    let mut token = String::new();
    std::io::stdin()
        .read_to_string(&mut token)
        .expect("token on stdin");
    token
}

/// The token string must not occur in this process's arguments or environment (NFT-SEC-34).
pub fn assert_not_in_process(token: &str) {
    for file in ["/proc/self/cmdline", "/proc/self/environ"] {
        let bytes = std::fs::read(file).expect("read /proc");
        let found = bytes
            .windows(token.len())
            .any(|window| window == token.as_bytes());
        assert!(!found, "the token occurs in {file}");
    }
}

/// Records every tracing event of the installing thread as `(level, "field=value ...")`.
#[derive(Clone, Default)]
pub struct Captured(Arc<Mutex<Vec<(Level, String)>>>);

impl Captured {
    pub fn install(&self) -> DefaultGuard {
        tracing::subscriber::set_default(Collector(self.clone()))
    }

    pub fn events(&self) -> Vec<(Level, String)> {
        self.0.lock().expect("capture lock").clone()
    }

    pub fn warnings(&self) -> Vec<String> {
        self.events()
            .into_iter()
            .filter(|(level, _)| *level == Level::WARN)
            .map(|(_, text)| text)
            .collect()
    }
}

struct Collector(Captured);

struct Fields(String);

impl Visit for Fields {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        write!(self.0, "{}={value:?} ", field.name()).expect("format field");
    }
}

impl Subscriber for Collector {
    fn enabled(&self, _: &Metadata<'_>) -> bool {
        true
    }

    fn new_span(&self, _: &Attributes<'_>) -> Id {
        Id::from_u64(1)
    }

    fn record(&self, _: &Id, _: &Record<'_>) {}

    fn record_follows_from(&self, _: &Id, _: &Id) {}

    fn event(&self, event: &Event<'_>) {
        let mut fields = Fields(String::new());
        event.record(&mut fields);
        (self.0.0.lock().expect("capture lock")).push((*event.metadata().level(), fields.0));
    }

    fn enter(&self, _: &Id) {}

    fn exit(&self, _: &Id) {}
}
