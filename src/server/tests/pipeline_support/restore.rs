//! AZ-3666 database restore and resync pipeline: helpers of `pipeline_db_restore.rs`.
//!
//! The dump tools are the real PostgreSQL 18 client. `SC_PG_TOOLS`, when set, names a directory holding `pg_dump`
//! and `pg_restore` (CI: `/usr/lib/postgresql/18/bin`, installed by `scripts/ci/install-tools.sh pg-client-18`);
//! unset, both are taken from `PATH`. The tools reach the server through the libpq variables `PGHOST`, `PGPORT`,
//! `PGUSER`, `PGPASSWORD` and `PGDATABASE`, which the tests set from `DATABASE_URL` (no percent-encoding in it);
//! the dump goes to stdout and the restore reads stdin, so a wrapper that runs the tools elsewhere (a container on
//! a laptop without a client) needs no shared file. A tool that is missing or not major version 18 fails the test
//! with that reason; nothing is skipped.

use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::Value;
use sqlx::{AssertSqlSafe, Connection, PgConnection};
use uuid::Uuid;

use super::{File, Pipeline, WITHIN};

/// One run of `sunnycloud --json ...`.
#[derive(Debug)]
pub struct Run {
    pub code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

impl Run {
    /// The one JSON envelope `--json` prints.
    pub fn envelope(&self) -> Value {
        serde_json::from_str(&self.stdout).unwrap_or_else(|error| {
            panic!(
                "no JSON envelope ({error}): {}\n{}",
                self.stdout, self.stderr
            )
        })
    }

    /// `(check, level, message)` of every finding in the envelope.
    pub fn findings(&self) -> Vec<(String, String, String)> {
        let envelope = self.envelope();
        envelope["data"]["findings"]
            .as_array()
            .unwrap_or_else(|| panic!("no findings in {envelope}"))
            .iter()
            .map(|item| {
                let text = |key: &str| item[key].as_str().unwrap_or_default().to_owned();
                (text("check"), text("level"), text("message"))
            })
            .collect()
    }
}

/// The superuser connection of `DATABASE_URL` as libpq variables.
fn libpq_env() -> BTreeMap<&'static str, String> {
    let url =
        std::env::var("DATABASE_URL").expect("DATABASE_URL names the PostgreSQL 18 superuser");
    let rest = url
        .strip_prefix("postgres://")
        .or_else(|| url.strip_prefix("postgresql://"))
        .unwrap_or_else(|| panic!("DATABASE_URL is not a postgres URL"));
    let (user_info, location) = rest.rsplit_once('@').expect("DATABASE_URL has a user");
    let (user, password) = user_info.split_once(':').unwrap_or((user_info, ""));
    let (host_port, database) = location.split_once('/').unwrap_or((location, "postgres"));
    let (host, port) = host_port.rsplit_once(':').unwrap_or((host_port, "5432"));
    let database = database.split('?').next().unwrap_or("postgres");
    BTreeMap::from([
        ("PGHOST", host.to_owned()),
        ("PGPORT", port.to_owned()),
        ("PGUSER", user.to_owned()),
        ("PGPASSWORD", password.to_owned()),
        ("PGDATABASE", database.to_owned()),
    ])
}

/// `name` from `SC_PG_TOOLS`, else from `PATH`.
fn pg_tool(name: &str) -> Command {
    let program = match std::env::var_os("SC_PG_TOOLS") {
        Some(dir) => PathBuf::from(dir).join(name),
        None => PathBuf::from(name),
    };
    let mut command = Command::new(program);
    command.envs(libpq_env());
    command
}

/// Runs `command` with `input` on stdin; its stdout, or a panic naming the tool and its stderr.
fn run_tool(name: &str, mut command: Command, input: &[u8]) -> Vec<u8> {
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap_or_else(|error| {
            panic!(
                "{name} cannot start ({error}): install the PostgreSQL 18 client (CI: scripts/ci/install-tools.sh \
                 pg-client-18) or set SC_PG_TOOLS to a directory holding pg_dump and pg_restore"
            )
        });
    let mut stdin = child.stdin.take().expect("stdin is piped");
    stdin.write_all(input).expect("write the tool's stdin");
    drop(stdin);
    let output = child.wait_with_output().expect("the tool ends");
    assert!(
        output.status.success(),
        "{name} failed with {}: {}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    output.stdout
}

/// Panics unless `name --version` reports PostgreSQL 18.
fn require_18(name: &str) {
    let mut command = pg_tool(name);
    command.arg("--version");
    let version = String::from_utf8(run_tool(name, command, &[])).expect("UTF-8 version");
    assert!(
        version.contains("(PostgreSQL) 18."),
        "{name} must be the PostgreSQL 18 client, found `{}`",
        version.trim()
    );
}

/// `pg_dump --format=custom` of `database`.
pub fn pg_dump(database: &str) -> Vec<u8> {
    require_18("pg_dump");
    let mut command = pg_tool("pg_dump");
    command.args(["--format=custom", &format!("--dbname={database}")]);
    run_tool("pg_dump", command, &[])
}

/// `pg_restore` of `dump` into the empty `database`, stopping at the first error, in one transaction.
fn pg_restore(database: &str, dump: &[u8]) {
    require_18("pg_restore");
    let mut command = pg_tool("pg_restore");
    command.args([
        "--exit-on-error",
        "--single-transaction",
        &format!("--dbname={database}"),
    ]);
    run_tool("pg_restore", command, dump);
}

/// Superuser SQL on the maintenance database of `DATABASE_URL`.
async fn maintenance(sql: String) {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL");
    let mut conn = PgConnection::connect(&url)
        .await
        .expect("connect as the superuser");
    sqlx::raw_sql(AssertSqlSafe(sql.clone()))
        .execute(&mut conn)
        .await
        .unwrap_or_else(|error| panic!("{sql}: {error}"));
    conn.close().await.expect("close the superuser connection");
}

/// Percent-encodes everything but unreserved characters, for a query value.
fn query_value(text: &str) -> String {
    text.bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                char::from(byte).to_string()
            }
            other => format!("%{other:02X}"),
        })
        .collect()
}

/// The production text of a workspace source (before its `#[cfg(test)]` module).
pub fn production_source(file: &str) -> String {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the server crate sits inside the workspace");
    let text =
        fs::read_to_string(workspace.join(file)).unwrap_or_else(|error| panic!("{file}: {error}"));
    match text.find("#[cfg(test)]") {
        Some(at) => text[..at].to_owned(),
        None => text,
    }
}

impl Pipeline {
    /// `sunnycloud --json <args>` with the stack's environment.
    pub fn command(&self, args: &[&str]) -> Run {
        self.command_with(&self.stack.root.setup.env, args)
    }

    fn command_with(&self, env: &BTreeMap<String, String>, args: &[&str]) -> Run {
        let output = Command::new(env!("CARGO_BIN_EXE_sunnycloud"))
            .arg("--json")
            .args(args)
            .env_clear()
            .envs(env)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .expect("the binary runs");
        Run {
            code: output.status.code(),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        }
    }

    /// `sunnycloud --json doctor` with the provider pointed at an unreachable local issuer, so no check needs the
    /// internet; the serve environment itself is left as it is.
    pub fn doctor(&self) -> Run {
        let setup = &self.stack.root.setup;
        let secret = setup.dir.join("doctor_oidc_client_secret");
        fs::write(&secret, "oidc-secret").expect("write the OIDC secret");
        let mut env = setup.env.clone();
        env.remove("SUNNYCLOUD_GITHUB_CLIENT_ID");
        env.remove("SUNNYCLOUD_PROVIDER_GITHUB_CLIENT_SECRET_FILE");
        for (key, value) in [
            ("SUNNYCLOUD_OIDC_ISSUER", "http://127.0.0.1:1".to_owned()),
            ("SUNNYCLOUD_OIDC_CLIENT_ID", "client".to_owned()),
            ("SUNNYCLOUD_ALLOW_PLAIN_HTTP_DEV", "true".to_owned()),
            (
                "SUNNYCLOUD_PROVIDER_OIDC_CLIENT_SECRET_FILE",
                secret.display().to_string(),
            ),
        ] {
            env.insert(key.to_owned(), value);
        }
        self.command_with(&env, &["doctor"])
    }

    /// SIGTERM; the binary exits 0.
    pub fn stop(&mut self) {
        self.server.signal("TERM");
        let exited = self.server.wait_exit(WITHIN);
        assert_eq!(exited.code, Some(0), "{}", exited.stderr_text());
    }

    /// A new `serve` on the same database and volume, waited for until ready.
    pub fn restart(&mut self) {
        self.server = self.stack.spawn();
        self.server.wait_ready(self.stack.ports.ops, WITHIN);
    }

    /// `pg_dump` of the running install's database.
    pub fn dump(&self) -> Vec<u8> {
        pg_dump(&self.stack.root.db.name)
    }

    /// The restore drill's database part: the database is wiped (dropped and created empty) and `dump` restored.
    pub async fn restore(&self, dump: &[u8]) {
        let name = &self.stack.root.db.name;
        maintenance(format!("DROP DATABASE {name} WITH (FORCE)")).await;
        maintenance(format!("CREATE DATABASE {name} TEMPLATE template0")).await;
        pg_restore(name, dump);
    }

    /// The `.epoch` marker on the blob volume, as text.
    pub fn marker(&self) -> String {
        let path = self.stack.blob_root().join(".epoch");
        fs::read_to_string(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
    }

    /// `<blob_root>/.keycheck`.
    pub fn key_check_file(&self) -> PathBuf {
        self.stack.blob_root().join(".keycheck")
    }

    /// The configured master key file.
    pub fn master_key_file(&self) -> PathBuf {
        self.stack.root.setup.dir.join("master.key")
    }

    /// The blob of `revision`.
    pub async fn blob_of(&self, revision: Uuid) -> Uuid {
        let mut admin = self.stack.root.db.admin().await;
        let blob: Uuid = sqlx::query_scalar("SELECT blob_id FROM revisions WHERE id = $1")
            .bind(revision)
            .fetch_one(&mut admin)
            .await
            .expect("read the revision's blob");
        admin.close().await.expect("close admin connection");
        blob
    }

    /// Every page of the children of `folder`, `limit` a page.
    pub fn pages(&self, folder: Uuid, limit: u32) -> Vec<Vec<Value>> {
        let mut pages = Vec::new();
        let mut after: Option<String> = None;
        loop {
            let mut path = format!("/api/v1/nodes/{folder}/children?limit={limit}");
            if let Some(key) = &after {
                path.push_str(&format!("&after={}", query_value(key)));
            }
            let listed = self.call("GET", &path, &[], &[]);
            assert_eq!(listed.status, 200, "{path}: {listed:?}");
            let page = listed.json();
            pages.push(page["items"].as_array().expect("items").clone());
            match page["next"].as_str() {
                Some(next) => after = Some(next.to_owned()),
                None => return pages,
            }
        }
    }

    /// Every node under the account's root, folder by folder in pages of `limit`, in the order listed.
    pub fn walk(&self, limit: u32) -> Vec<Value> {
        let (mut found, mut folders) = (Vec::new(), vec![self.account.root]);
        while let Some(folder) = folders.pop() {
            for item in self.pages(folder, limit).into_iter().flatten() {
                if item["kind"] == "folder" {
                    folders.push(super::uuid_of(&item, "node_id"));
                }
                found.push(item);
            }
        }
        found
    }
}

/// One published file of the restore scenario.
pub struct Published {
    pub path: String,
    pub node: Uuid,
    pub revision: Uuid,
    pub blob: Uuid,
    pub bytes: Vec<u8>,
}

/// The install after the restore drill, before any bump.
pub struct Restored {
    pub pipeline: Pipeline,
    /// Every node of the dump besides the root.
    pub nodes: Vec<Uuid>,
    /// The files whose blob files are gone from the volume.
    pub missing: Vec<Published>,
    /// The files whose blob files are on the volume.
    pub kept: Vec<Published>,
    /// The device's cursor when the install was last served, after the dump.
    pub old_cursor: String,
    /// The feed epoch of the dump.
    pub dump_epoch: u64,
    /// The `.epoch` marker the restored database finds, newer than the dump.
    pub newer_marker: u64,
}

/// F14 step 1 on a real install: a dump at T0 of a folder tree with eight files; after it, three files deleted
/// and their blobs collected (`retention` 0 and `admin gc-run`), two files added, and one `admin epoch-bump`, so
/// the volume's marker is newer than the dump; then the database wiped and the dump restored with `serve` stopped.
pub async fn restored() -> Restored {
    let mut pipeline = Pipeline::start(&[("retention", "0s")]).await;
    let docs = pipeline.folder("docs");
    let deep = pipeline.subfolder(docs, "deep");
    let mut published = Vec::new();
    for index in 0..8u64 {
        let (parent, prefix) = if index % 2 == 0 {
            (docs, "docs")
        } else {
            (deep, "docs/deep")
        };
        let file = File::new(
            &format!("file-{index}.txt"),
            3000 + index as usize,
            index + 70,
        );
        let (node, revision) = pipeline.publish_small(parent, &file, None);
        published.push(Published {
            path: format!("{prefix}/{}", file.name),
            node,
            revision,
            blob: pipeline.blob_of(revision).await,
            bytes: file.bytes,
        });
    }
    let dump_cursor = pipeline.feed_cursor();
    let dump = pipeline.dump();
    let missing: Vec<Published> = published.drain(..3).collect();
    for file in &missing {
        let deleted = pipeline.delete_file(file.node);
        assert!((200..300).contains(&deleted.status), "{deleted:?}");
    }
    for index in 0..2u64 {
        let file = File::new(&format!("late-{index}.txt"), 2000, index + 90);
        pipeline.publish_small(docs, &file, None);
    }
    let collected = pipeline.command(&["admin", "gc-run"]);
    assert_eq!(collected.code, Some(0), "{collected:?}");
    for file in &missing {
        let gone = pipeline.blob_file(file.blob);
        assert!(!gone.exists(), "{} kept", gone.display());
    }
    let old_cursor = pipeline.feed_cursor();
    pipeline.stop();
    let bumped = pipeline.command(&["admin", "epoch-bump", "--yes"]);
    assert_eq!(bumped.code, Some(0), "{bumped:?}");
    pipeline.restore(&dump).await;
    let mut nodes = vec![docs, deep];
    nodes.extend(missing.iter().chain(&published).map(|file| file.node));
    Restored {
        nodes,
        missing,
        kept: published,
        old_cursor,
        dump_epoch: super::feed::split(&dump_cursor).0,
        newer_marker: pipeline.marker().trim().parse().expect("a marker"),
        pipeline,
    }
}
