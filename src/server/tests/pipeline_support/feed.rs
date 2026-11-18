//! Flow F7 on the [`Pipeline`]: change-feed reads and held polls over HTTP as any principal, the proof that a
//! poll is parked (the hold cap of its principal answers 429 to a probe), the pool gauge of the ops listener, the
//! superuser arrangement of a purge horizon, and a count of a construction across the non-test sources.

use std::fs;
use std::path::Path;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use serde_json::Value;
use sqlx::Connection;

use super::wiring::production_text;
use super::{POLL_EVERY, Pipeline, WITHIN};
use crate::serve_support::http::{Response, call};

/// A held poll on its own connection: its answer and when it arrived.
pub type Held = JoinHandle<(Response, Instant)>;

/// One page of `GET /api/v1/changes`.
pub struct Page {
    pub items: Vec<Value>,
    pub next_cursor: String,
    pub has_more: bool,
}

impl Page {
    pub fn of(answer: &Response) -> Page {
        assert_eq!(answer.status, 200, "{answer:?}");
        let page = answer.json();
        Page {
            items: page["items"].as_array().expect("items").clone(),
            next_cursor: page["next_cursor"]
                .as_str()
                .expect("a next cursor")
                .to_owned(),
            has_more: page["has_more"].as_bool().expect("has_more"),
        }
    }
}

impl Pipeline {
    /// `GET /api/v1/changes` as `token`, with `wait` seconds and `limit` when given.
    pub fn changes(
        &self,
        token: &str,
        cursor: &str,
        wait: Option<u64>,
        limit: Option<u32>,
    ) -> Response {
        let mut path = format!("/api/v1/changes?cursor={cursor}");
        if let Some(wait) = wait {
            path.push_str(&format!("&wait={wait}"));
        }
        if let Some(limit) = limit {
            path.push_str(&format!("&limit={limit}"));
        }
        call(self.public(), "GET", &path, token, &[], &[])
    }

    /// `GET /api/v1/changes/head` as `token`: the cursor `E.S`.
    pub fn head_as(&self, token: &str) -> String {
        let head = call(
            self.public(),
            "GET",
            "/api/v1/changes/head",
            token,
            &[],
            &[],
        );
        assert_eq!(head.status, 200, "{head:?}");
        head.json()["cursor"].as_str().expect("a cursor").to_owned()
    }

    /// A poll as `token` from `cursor`, parked on the server when this returns.
    ///
    /// Needs `max_holds_per_principal` at 1: a probe of the same principal that is refused 429 proves that one
    /// hold of that principal is parked, and probes run one at a time, so that hold is the poll. A probe that
    /// got in first parks for its own 1 s wait; the poll it refused is sent again, each time with a longer head
    /// start before the first probe, so a loaded host cannot keep the probe ahead of the poll.
    pub fn park(&self, token: &str, cursor: &str, wait: u64) -> Held {
        let deadline = Instant::now() + WITHIN;
        let mut head_start = POLL_EVERY;
        loop {
            let poll = self.spawn_poll(token, cursor, wait);
            thread::sleep(head_start);
            head_start *= 2;
            loop {
                assert!(Instant::now() < deadline, "the poll never parked");
                let probe = self.changes(token, cursor, Some(1), None);
                if probe.status == 429 {
                    assert!(!poll.is_finished(), "the poll answered while parked");
                    return poll;
                }
                assert_eq!(probe.status, 200, "{probe:?}");
                if poll.is_finished() {
                    let (refused, _) = poll.join().expect("the poll thread ends");
                    assert_eq!(refused.status, 429, "{refused:?}");
                    break;
                }
                thread::sleep(POLL_EVERY);
            }
        }
    }

    fn spawn_poll(&self, token: &str, cursor: &str, wait: u64) -> Held {
        let path = format!("/api/v1/changes?cursor={cursor}&wait={wait}");
        let (public, token) = (self.public(), token.to_owned());
        thread::spawn(move || {
            let answer = call(public, "GET", &path, &token, &[], &[]);
            (answer, Instant::now())
        })
    }

    /// Every page from `cursor` to the head as `token`, `limit` items a page; the items and the last cursor.
    pub fn read_to_head(&self, token: &str, cursor: &str, limit: u32) -> (Vec<Value>, String) {
        let (mut items, mut cursor) = (Vec::new(), cursor.to_owned());
        loop {
            let page = Page::of(&self.changes(token, &cursor, None, Some(limit)));
            items.extend(page.items);
            cursor = page.next_cursor;
            if !page.has_more {
                return (items, cursor);
            }
        }
    }

    /// `sunnycloud_db_pool_connections{state="in_use"}` of the ops listener's `/metrics`.
    pub fn pool_in_use(&self) -> u64 {
        let scraped = call(self.stack.ports.ops, "GET", "/metrics", "", &[], &[]);
        assert_eq!(scraped.status, 200, "{scraped:?}");
        let text = String::from_utf8(scraped.body).expect("metrics are UTF-8");
        text.lines()
            .find(|line| line.starts_with("sunnycloud_db_pool_connections{state=\"in_use\"}"))
            .and_then(|line| line.split_whitespace().last())
            .and_then(|value| value.parse::<f64>().ok())
            .map(|value| value as u64)
            .unwrap_or_else(|| panic!("no in-use gauge in:\n{text}"))
    }

    /// Superuser SQL: the purge horizon of the account raised to its head, as F12's tombstone purge raises it.
    pub async fn raise_horizon_to_head(&self) {
        let mut admin = self.stack.root.db.admin().await;
        sqlx::query("UPDATE account_feeds SET purge_horizon = seq WHERE account_id = $1")
            .bind(self.account.id)
            .execute(&mut admin)
            .await
            .expect("raise the purge horizon");
        admin.close().await.expect("close admin connection");
    }
}

/// `E` and `S` of the cursor `E.S`.
pub fn split(cursor: &str) -> (u64, u64) {
    let (epoch, seq) = cursor.split_once('.').expect("a cursor `E.S`");
    (
        epoch.parse().expect("an epoch"),
        seq.parse().expect("a seq"),
    )
}

/// Waits for a held poll's answer; panics past `within`.
pub fn answer(poll: Held, within: Duration) -> (Response, Instant) {
    let deadline = Instant::now() + within;
    while !poll.is_finished() {
        assert!(Instant::now() < deadline, "the poll did not answer");
        thread::sleep(Duration::from_millis(20));
    }
    poll.join().expect("the poll thread ends")
}

/// The files (relative to the workspace) whose production text contains `needle`, once per occurrence.
pub fn production_occurrences(roots: &[&str], needle: &str) -> Vec<String> {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the server crate sits inside the workspace");
    let mut found = Vec::new();
    for root in roots {
        visit(workspace, &workspace.join(root), needle, &mut found);
    }
    found
}

fn visit(workspace: &Path, dir: &Path, needle: &str, found: &mut Vec<String>) {
    let entries = fs::read_dir(dir).unwrap_or_else(|error| panic!("{}: {error}", dir.display()));
    for entry in entries {
        let path = entry.expect("a directory entry").path();
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("");
        if path.is_dir() {
            if !matches!(name, "tests" | "target" | "benches") {
                visit(workspace, &path, needle, found);
            }
            continue;
        }
        if !name.ends_with(".rs") {
            continue;
        }
        let production = production_text(&path);
        let relative = path.strip_prefix(workspace).unwrap_or(&path).display();
        for _ in production.matches(needle) {
            found.push(relative.to_string());
        }
    }
}
