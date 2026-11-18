//! The vault routes against the real `sunnycloud serve` binary of a `vault` build on PostgreSQL 18 (`serve_support`):
//! accounts with a folder to hold a vault, a device token and a browser cookie for the auth checks (requests through
//! `serve_support::http`, which keeps the response headers such as `ETag`), and probes of what the database stores.

#![allow(dead_code)]

use std::net::SocketAddr;
use std::time::Duration;

use serde_json::json;
use sqlx::Connection;
use uuid::Uuid;

use crate::serve_support::account::Account;
use crate::serve_support::http::{Response, send};
use crate::serve_support::{Server, Stack};

pub const WITHIN: Duration = Duration::from_secs(10);
pub const PUBLIC_URL: &str = "https://cloud.example.org";
/// The default `session_cookie`.
pub const SESSION_COOKIE: &str = "__Host-session";

pub fn bearer(token: &str) -> (&'static str, String) {
    ("Authorization", format!("Bearer {token}"))
}

pub fn json_type() -> (&'static str, String) {
    ("Content-Type", "application/json".to_owned())
}

/// The quoted strong tag of `hex`.
pub fn quoted(hex: &str) -> String {
    format!("\"{hex}\"")
}

/// The serve stack of a `vault` build, its binary ready.
pub struct Running {
    pub stack: Stack,
    pub server: Server,
}

impl Running {
    pub async fn start() -> Running {
        let stack = Stack::new().await;
        let mut server = stack.spawn();
        server.wait_ready(stack.ports.ops, WITHIN);
        Running { stack, server }
    }

    pub fn public(&self) -> SocketAddr {
        self.stack.ports.public
    }

    pub fn call(
        &self,
        method: &str,
        path: &str,
        headers: &[(&str, String)],
        body: &[u8],
    ) -> Response {
        send(self.public(), method, path, headers, body)
    }

    /// `method path` as `account`'s session bearer, with `headers` added.
    pub fn as_account(
        &self,
        account: &Account,
        method: &str,
        path: &str,
        headers: &[(&str, String)],
        body: &[u8],
    ) -> Response {
        let mut all = vec![bearer(&account.token)];
        all.extend(headers.iter().cloned());
        self.call(method, path, &all, body)
    }

    /// A folder under the account's root, made through `POST /api/v1/nodes`: a vault root may not be the root.
    pub fn folder(&self, account: &Account, name: &str) -> Uuid {
        let body = json!({"parent_id": account.root, "name": name}).to_string();
        let made = self.as_account(
            account,
            "POST",
            "/api/v1/nodes",
            &[json_type()],
            body.as_bytes(),
        );
        assert!((200..300).contains(&made.status), "{made:?}");
        let id = made.json()["node_id"]
            .as_str()
            .expect("a node id")
            .to_owned();
        Uuid::parse_str(&id).expect("a UUID")
    }

    /// `POST /api/v1/vaults` on a new folder of `account` with `header`; returns the vault id and the response.
    pub fn create_vault(&self, account: &Account, header: &[u8]) -> (Uuid, Response) {
        let root = self.folder(account, "Vault");
        let body = create_body(root, header);
        let reply = self.as_account(
            account,
            "POST",
            "/api/v1/vaults",
            &[json_type()],
            body.as_bytes(),
        );
        assert_eq!(reply.status, 201, "{reply:?}");
        let id = reply.json()["id"].as_str().expect("a vault id").to_owned();
        (Uuid::parse_str(&id).expect("a UUID"), reply)
    }

    /// A live device token of `account` (`Stack::device_token`).
    pub async fn device_token(&self, account: &Account) -> String {
        self.stack.device_token(account).await
    }

    /// PostgreSQL's lowercase hex SHA-256 of `bytes`.
    pub async fn sha256_hex(&self, bytes: &[u8]) -> String {
        let mut admin = self.stack.root.db.admin().await;
        let hex: String = sqlx::query_scalar("SELECT encode(sha256($1), 'hex')")
            .bind(bytes)
            .fetch_one(&mut admin)
            .await
            .expect("sha256");
        admin.close().await.expect("close admin connection");
        hex
    }

    /// The stored header bytes of `vault`.
    pub async fn stored_header(&self, vault: Uuid) -> Vec<u8> {
        let mut admin = self.stack.root.db.admin().await;
        let header: Vec<u8> = sqlx::query_scalar("SELECT header FROM vaults WHERE id = $1")
            .bind(vault)
            .fetch_one(&mut admin)
            .await
            .expect("the vault row");
        admin.close().await.expect("close admin connection");
        header
    }

    /// `table: n` for every table of the schema with a row whose text contains `needle`. One literal statement:
    /// `query_to_xml` runs the per-table count, with the table named by `%I` and the needle quoted by `%L`.
    pub async fn rows_containing(&self, needle: &str) -> Vec<String> {
        let mut admin = self.stack.root.db.admin().await;
        let found: Vec<(String, i32)> = sqlx::query_as(
            "SELECT table_name::text, n FROM ( \
               SELECT table_name, (xpath('/row/n/text()', query_to_xml(format( \
                 'SELECT count(*) AS n FROM %I.%I AS r WHERE strpos(r::text, %L) > 0', \
                 table_schema, table_name, $1), false, true, '')))[1]::text::int AS n \
               FROM information_schema.tables \
               WHERE table_schema = 'public' AND table_type = 'BASE TABLE') AS counts \
             WHERE n > 0 ORDER BY table_name",
        )
        .bind(needle)
        .fetch_all(&mut admin)
        .await
        .expect("scan the tables");
        admin.close().await.expect("close admin connection");
        found
            .into_iter()
            .map(|(table, count)| format!("{table}: {count}"))
            .collect()
    }

    /// Stops the binary and drops the database; returns the binary's output lines.
    pub async fn finish(mut self) -> Vec<String> {
        self.server.signal("TERM");
        let exited = self.server.wait_exit(WITHIN);
        assert_eq!(exited.code, Some(0), "{}", exited.stderr_text());
        self.stack.finish().await;
        exited.stderr
    }
}

/// `{"root_node_id", "header"}` with the header in standard base64.
pub fn create_body(root: Uuid, header: &[u8]) -> String {
    use base64::Engine;
    json!({
        "root_node_id": root,
        "header": base64::engine::general_purpose::STANDARD.encode(header),
    })
    .to_string()
}

/// `{"header"}` with the header in standard base64.
pub fn replace_body(header: &[u8]) -> String {
    use base64::Engine;
    json!({"header": base64::engine::general_purpose::STANDARD.encode(header)}).to_string()
}
