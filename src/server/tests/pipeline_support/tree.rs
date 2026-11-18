//! Flow F8 and F12 step 1 on the [`Pipeline`]: the node routes (folder create, rename and move, delete with a base
//! revision), a small-file commit as any device (new file or edit of a base revision), the feed read from a cursor,
//! the account's quota over HTTP, a second signed-in device with its own label, the blob GC row of a revision and
//! the `quota_drift` finding of `sunnycloud doctor`.

use std::collections::BTreeSet;
use std::net::SocketAddr;
use std::process::{Command, Stdio};
use std::time::Duration;

use hyper::body::Bytes;
use serde_json::{Value, json};
use sqlx::Connection;
use tokio::task::JoinSet;
use uuid::Uuid;

use super::h2::H2;
use super::{File, Pipeline, entry, json_type, now_micros, stage_on, staged_headers, staged_path};
use crate::serve_support::http::{Response, call};

/// How often the usage is sampled while concurrent operations run.
const SAMPLE_EVERY: Duration = Duration::from_millis(20);

/// One operation of the concurrent mix: a new file, an edit of a base revision, or a delete with its base.
pub enum Op {
    New(File),
    Edit(File, Uuid),
    Delete(Uuid, Uuid),
}

/// What an operation got: done, or refused with a status and a code (an entry error is a 200 with its code).
#[derive(Debug, PartialEq, Eq)]
pub enum Done {
    Ok,
    Refused(u16, String),
}

/// The usage samples and the result of every operation of [`Pipeline::run_mix`].
pub struct Mix {
    pub samples: Vec<i64>,
    pub done: Vec<Done>,
}

/// A node or revision id member of a JSON answer.
pub fn uuid_of(value: &Value, member: &str) -> Uuid {
    value[member]
        .as_str()
        .and_then(|id| Uuid::parse_str(id).ok())
        .unwrap_or_else(|| panic!("no `{member}` in {value}"))
}

/// The names of a listing.
pub fn names(listing: &[Value]) -> BTreeSet<String> {
    listing
        .iter()
        .map(|node| node["name"].as_str().expect("a name").to_owned())
        .collect()
}

/// The node ids of feed items.
pub fn feed_nodes(items: &[Value]) -> BTreeSet<Uuid> {
    items.iter().map(|item| uuid_of(item, "node_id")).collect()
}

/// The status and problem `code` of a refusal.
pub fn error_of(response: &Response) -> (u16, String) {
    let code = response.json()["code"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    (response.status, code)
}

/// `YYYY-MM-DD HH-MM` in UTC of Unix microseconds, the minute a conflict-copy name carries.
pub fn minute(micros: i64) -> String {
    let seconds = micros.div_euclid(1_000_000);
    let (days, second_of_day) = (seconds.div_euclid(86_400), seconds.rem_euclid(86_400));
    // Howard Hinnant's `civil_from_days`.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * shifted + 2) / 5 + 1;
    let month = if shifted < 10 {
        shifted + 3
    } else {
        shifted - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    let (hour, min) = (second_of_day / 3600, second_of_day % 3600 / 60);
    format!("{year:04}-{month:02}-{day:02} {hour:02}-{min:02}")
}

/// The conflict-copy names `stem (conflict copy <minute> <label>)ext` for every minute from `from` to `to`.
pub fn copy_names(stem: &str, ext: &str, label: &str, from: i64, to: i64) -> Vec<String> {
    let mut names: Vec<String> = [from, to]
        .iter()
        .map(|at| format!("{stem} (conflict copy {} {label}){ext}", minute(*at)))
        .collect();
    names.dedup();
    names
}

impl Pipeline {
    /// `POST /api/v1/nodes` of a folder `name` under `parent`, the answer as it comes.
    pub fn create_folder(&self, parent: Uuid, name: &str) -> Response {
        let body = json!({"parent_id": parent, "name": name}).to_string();
        self.call("POST", "/api/v1/nodes", &[json_type()], body.as_bytes())
    }

    /// A folder `name` under `parent`; panics unless it was created.
    pub fn subfolder(&self, parent: Uuid, name: &str) -> Uuid {
        let made = self.create_folder(parent, name);
        assert_eq!(made.status, 201, "{made:?}");
        uuid_of(&made.json(), "node_id")
    }

    /// `PATCH /api/v1/nodes/{node}` with `body` (`name`, `parent_id` or both).
    pub fn patch_node(&self, node: Uuid, body: &Value) -> Response {
        self.call(
            "PATCH",
            &format!("/api/v1/nodes/{node}"),
            &[json_type()],
            body.to_string().as_bytes(),
        )
    }

    /// `DELETE /api/v1/nodes/{node}`, with `?base_revision=` when given.
    pub fn delete_with_base(&self, node: Uuid, base: Option<Uuid>) -> Response {
        let path = match base {
            Some(base) => format!("/api/v1/nodes/{node}?base_revision={base}"),
            None => format!("/api/v1/nodes/{node}"),
        };
        self.call("DELETE", &path, &[], &[])
    }

    /// `PUT /api/v1/staged-uploads/{key}` of `file` as the device of `token`.
    pub fn stage_as(&self, token: &str, file: &File) -> Response {
        stage_on(self.stack.ports.public, token, file)
    }

    /// `bytes` staged and committed by the device of `token` as `name` under `parent`, as a new file or, with
    /// `base`, as an edit of that revision's file; the batch's one entry result.
    pub fn commit_as(
        &self,
        token: &str,
        parent: Uuid,
        name: &str,
        bytes: &[u8],
        base: Option<Uuid>,
    ) -> Value {
        let file = File {
            key: Uuid::new_v4(),
            name: name.to_owned(),
            bytes: bytes.to_vec(),
        };
        let staged = stage_on(self.stack.ports.public, token, &file);
        assert_eq!(staged.status, 200, "{staged:?}");
        self.commit_staged(token, &file, parent, base)
    }

    /// The commit of a file already staged by the device of `token`; the batch's one entry result.
    pub fn commit_staged(
        &self,
        token: &str,
        file: &File,
        parent: Uuid,
        base: Option<Uuid>,
    ) -> Value {
        let committed = commit_on(self.stack.ports.public, token, file, parent, base);
        assert_eq!(committed.status, 200, "{committed:?}");
        committed.json()["entries"][0].clone()
    }

    /// The feed items after `cursor` (no hold, up to 1000).
    pub fn changes_since(&self, cursor: &str) -> Vec<Value> {
        let page = self.call(
            "GET",
            &format!("/api/v1/changes?cursor={cursor}&wait=0&limit=1000"),
            &[],
            &[],
        );
        assert_eq!(page.status, 200, "{page:?}");
        let page = page.json();
        assert_eq!(page["has_more"], false, "{page}");
        page["items"].as_array().expect("items").clone()
    }

    /// `used` and `limit_bytes` of `GET /api/v1/accounts/me/quota`.
    pub fn quota(&self) -> (i64, Option<i64>) {
        let quota = self.call("GET", "/api/v1/accounts/me/quota", &[], &[]);
        assert_eq!(quota.status, 200, "{quota:?}");
        let quota = quota.json();
        (
            quota["used"].as_i64().expect("used"),
            quota["limit_bytes"].as_i64(),
        )
    }

    /// A second signed-in device of the account whose label is `label`, in 01's table as the serve harness
    /// stores a device token (SHA-256 of the text).
    pub async fn device_named(&self, label: &str) -> String {
        let token = format!("sc_dev_serve{}", Uuid::new_v4().simple());
        let mut admin = self.stack.root.db.admin().await;
        sqlx::query(
            "INSERT INTO device_tokens (id, account_id, token_hash, device_name, platform, created_at) \
             VALUES (gen_random_uuid(), $1, sha256(convert_to($2, 'UTF8')), $3, 'linux', \
             'epoch'::timestamptz + $4::bigint * interval '1 microsecond')",
        )
        .bind(self.account.id)
        .bind(&token)
        .bind(label)
        .bind(now_micros())
        .execute(&mut admin)
        .await
        .expect("insert device token");
        admin.close().await.expect("close admin connection");
        token
    }

    /// The `blob_gc` row of the blob of `revision`: its reason and `due_at` in Unix microseconds.
    pub async fn gc_row(&self, revision: Uuid) -> Option<(String, i64)> {
        let mut admin = self.stack.root.db.admin().await;
        let row: Option<(String, i64)> = sqlx::query_as(
            "SELECT g.reason, (extract(epoch FROM g.due_at) * 1000000)::bigint FROM blob_gc g \
             JOIN revisions r ON r.blob_id = g.blob_id WHERE r.id = $1",
        )
        .bind(revision)
        .fetch_optional(&mut admin)
        .await
        .expect("read the gc row");
        admin.close().await.expect("close admin connection");
        row
    }

    /// The account's stored `used_bytes` in 07a's ledger, read as one snapshot.
    pub async fn used_bytes(&self) -> i64 {
        let mut admin = self.stack.root.db.admin().await;
        let used: i64 = sqlx::query_scalar("SELECT used_bytes FROM quota WHERE account_id = $1")
            .bind(self.account.id)
            .fetch_one(&mut admin)
            .await
            .expect("the quota row");
        admin.close().await.expect("close admin connection");
        used
    }

    /// `(level, message)` of the `quota_drift` findings of `sunnycloud --json doctor` on this stack. The providers are
    /// pointed at an unreachable local issuer first, so no check depends on the internet.
    pub fn quota_drift_findings(&mut self) -> Vec<(String, String)> {
        self.stack
            .root
            .setup
            .remove("github_client_id")
            .without_secret("provider_github_client_secret")
            .set("oidc_issuer", "http://127.0.0.1:1")
            .set("oidc_client_id", "client")
            .set("allow_plain_http_dev", "true")
            .secret("provider_oidc_client_secret", "oidc-secret");
        let output = Command::new(env!("CARGO_BIN_EXE_sunnycloud"))
            .args(["--json", "doctor"])
            .env_clear()
            .envs(&self.stack.root.setup.env)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .expect("doctor runs");
        let stdout = String::from_utf8_lossy(&output.stdout);
        let envelope: Value = serde_json::from_str(&stdout).unwrap_or_else(|error| {
            panic!(
                "doctor printed no JSON envelope ({error}): {stdout}\n{}",
                String::from_utf8_lossy(&output.stderr)
            )
        });
        envelope["data"]["findings"]
            .as_array()
            .unwrap_or_else(|| panic!("no findings in {envelope}"))
            .iter()
            .filter(|finding| finding["check"] == "quota_drift")
            .map(|finding| {
                let text = |key: &str| finding[key].as_str().unwrap_or_default().to_owned();
                (text("level"), text("message"))
            })
            .collect()
    }
}

/// The `commits:batch` body of `file` as one entry, an edit of `base` when given.
fn batch_body(file: &File, parent: Uuid, base: Option<Uuid>) -> String {
    let mut item = entry(file.key, parent, &file.name);
    if let Some(base) = base {
        item["base_revision"] = json!(base);
    }
    json!({"commit_id": Uuid::new_v4(), "entries": [item]}).to_string()
}

/// `POST /api/v1/commits:batch` of a staged `file` on `public` as the device of `token`; usable off the test task.
pub fn commit_on(
    public: SocketAddr,
    token: &str,
    file: &File,
    parent: Uuid,
    base: Option<Uuid>,
) -> Response {
    let body = batch_body(file, parent, base);
    let path = "/api/v1/commits:batch";
    call(public, "POST", path, token, &[json_type()], body.as_bytes())
}

/// `PATCH /api/v1/nodes/{node}` on `public` as the device of `token`; usable off the test task.
pub fn patch_on(public: SocketAddr, token: &str, node: Uuid, body: &Value) -> Response {
    let path = format!("/api/v1/nodes/{node}");
    let body = body.to_string();
    call(
        public,
        "PATCH",
        &path,
        token,
        &[json_type()],
        body.as_bytes(),
    )
}

fn refused(response: &crate::serve_support::http::Response) -> Done {
    let code = serde_json::from_slice::<Value>(&response.body)
        .ok()
        .and_then(|body| body["code"].as_str().map(str::to_owned))
        .unwrap_or_default();
    Done::Refused(response.status, code)
}

/// One operation over the h2c connection: stage then commit, or delete.
async fn run_op(h2: H2, device: String, folder: Uuid, op: Op) -> Done {
    let (file, base) = match op {
        Op::Delete(node, base) => {
            let path = format!("/api/v1/nodes/{node}?base_revision={base}");
            let deleted = h2.call("DELETE", &path, &device, &[], Bytes::new()).await;
            return if deleted.status == 204 {
                Done::Ok
            } else {
                refused(&deleted)
            };
        }
        Op::New(file) => (file, None),
        Op::Edit(file, base) => (file, Some(base)),
    };
    let body = Bytes::from(file.bytes.clone());
    let staged = h2
        .call(
            "PUT",
            &staged_path(&file),
            &device,
            &staged_headers(&file),
            body,
        )
        .await;
    if staged.status != 200 {
        return refused(&staged);
    }
    let body = Bytes::from(batch_body(&file, folder, base));
    let path = "/api/v1/commits:batch";
    let committed = h2.call("POST", path, &device, &[json_type()], body).await;
    if committed.status != 200 {
        return refused(&committed);
    }
    let result = &committed.json()["entries"][0];
    match result["status"].as_str() {
        Some("committed") => Done::Ok,
        _ => Done::Refused(200, result["code"].as_str().unwrap_or_default().to_owned()),
    }
}

impl Pipeline {
    /// Sets the account's quota limit (arranged from outside, as the serve harness does).
    pub async fn set_limit(&self, bytes: i64) {
        let mut admin = self.stack.root.db.admin().await;
        sqlx::query("UPDATE quota SET limit_bytes = $2 WHERE account_id = $1")
            .bind(self.account.id)
            .bind(bytes)
            .execute(&mut admin)
            .await
            .expect("set the quota limit");
        admin.close().await.expect("close admin connection");
    }

    /// Runs `ops` as concurrent streams of one h2c connection, at most `streams` at once, and samples the stored
    /// usage every [`SAMPLE_EVERY`] while they run.
    pub async fn run_mix(&self, folder: Uuid, ops: Vec<Op>, streams: usize) -> Mix {
        let h2 = H2::connect(self.stack.ports.public).await;
        let mut admin = self.stack.root.db.admin().await;
        let (mut samples, mut done) = (Vec::new(), Vec::new());
        let mut pending = ops.into_iter();
        let mut running = JoinSet::new();
        loop {
            while running.len() < streams {
                let Some(op) = pending.next() else { break };
                running.spawn(run_op(h2.clone(), self.device.clone(), folder, op));
            }
            if running.is_empty() {
                break;
            }
            let used: i64 =
                sqlx::query_scalar("SELECT used_bytes FROM quota WHERE account_id = $1")
                    .bind(self.account.id)
                    .fetch_one(&mut admin)
                    .await
                    .expect("sample the usage");
            samples.push(used);
            if let Ok(Some(finished)) =
                tokio::time::timeout(SAMPLE_EVERY, running.join_next()).await
            {
                done.push(finished.expect("no operation panicked"));
            }
        }
        admin.close().await.expect("close admin connection");
        Mix { samples, done }
    }
}
