//! The vault routes against the real `sunnycloud serve` binary of a `vault` build on PostgreSQL 18 (`serve_support`):
//! accounts with a folder to hold a vault, a device token and a browser cookie for the auth checks, a client that
//! keeps response headers (`ETag`), and probes of what the database stores.

#![allow(dead_code)]

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::time::Duration;

use sc_clock::{Clock, SystemClock};
use serde_json::{Value, json};
use sqlx::Connection;
use uuid::Uuid;

use crate::serve_support::account::Account;
use crate::serve_support::{Server, Stack};

pub const WITHIN: Duration = Duration::from_secs(10);
const HOST: &str = "cloud.example.org";
pub const PUBLIC_URL: &str = "https://cloud.example.org";
/// The default `session_cookie`.
pub const SESSION_COOKIE: &str = "__Host-session";
const IO_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Debug)]
pub struct Reply {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Reply {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(found, _)| found.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }

    pub fn json(&self) -> Value {
        serde_json::from_slice(&self.body).unwrap_or_else(|error| {
            panic!(
                "status {}, body is not JSON ({error}): {}",
                self.status,
                String::from_utf8_lossy(&self.body)
            )
        })
    }

    pub fn code(&self) -> String {
        self.json()["code"].as_str().unwrap_or_default().to_owned()
    }
}

/// One HTTP/1.1 request with `Connection: close`; panics on a transport failure.
pub fn send(
    addr: SocketAddr,
    method: &str,
    path: &str,
    headers: &[(&str, String)],
    body: &[u8],
) -> Reply {
    let attempt = || -> std::io::Result<Reply> {
        let mut stream = TcpStream::connect_timeout(&addr, Duration::from_secs(2))?;
        stream.set_read_timeout(Some(IO_TIMEOUT))?;
        stream.set_write_timeout(Some(IO_TIMEOUT))?;
        let mut head = format!(
            "{method} {path} HTTP/1.1\r\nHost: {HOST}\r\nConnection: close\r\nContent-Length: {}\r\n",
            body.len()
        );
        for (name, value) in headers {
            head.push_str(&format!("{name}: {value}\r\n"));
        }
        head.push_str("\r\n");
        stream.write_all(head.as_bytes())?;
        stream.write_all(body)?;
        let mut raw = Vec::new();
        stream.read_to_end(&mut raw)?;
        Ok(parse(&raw))
    };
    attempt().unwrap_or_else(|error| panic!("{method} {path} on {addr}: {error}"))
}

fn parse(raw: &[u8]) -> Reply {
    let split = raw
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .expect("an end of the response head");
    let head = std::str::from_utf8(&raw[..split]).expect("the head is UTF-8");
    let mut lines = head.split("\r\n");
    let status = lines
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|code| code.parse().ok())
        .expect("a status line");
    let headers: Vec<(String, String)> = lines
        .filter_map(|line| line.split_once(':'))
        .map(|(name, value)| (name.trim().to_ascii_lowercase(), value.trim().to_owned()))
        .collect();
    let rest = &raw[split + 4..];
    let chunked = headers
        .iter()
        .any(|(name, value)| name == "transfer-encoding" && value.eq_ignore_ascii_case("chunked"));
    let body = if chunked {
        dechunk(rest).expect("a well-formed chunked body")
    } else {
        rest.to_vec()
    };
    Reply {
        status,
        headers,
        body,
    }
}

fn dechunk(mut rest: &[u8]) -> Option<Vec<u8>> {
    let mut body = Vec::new();
    loop {
        let end = rest.windows(2).position(|window| window == b"\r\n")?;
        let size_text = std::str::from_utf8(&rest[..end]).ok()?;
        let size = usize::from_str_radix(size_text.split(';').next()?.trim(), 16).ok()?;
        rest = &rest[end + 2..];
        if size == 0 {
            return Some(body);
        }
        body.extend_from_slice(rest.get(..size)?);
        rest = rest.get(size + 2..)?;
    }
}

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

    pub fn call(&self, method: &str, path: &str, headers: &[(&str, String)], body: &[u8]) -> Reply {
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
    ) -> Reply {
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
    pub fn create_vault(&self, account: &Account, header: &[u8]) -> (Uuid, Reply) {
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

    /// A live device token of `account`, in 01's table as 01 stores it (SHA-256 of the text).
    pub async fn device_token(&self, account: &Account) -> String {
        let token = format!("sc_dev_vault{}", Uuid::new_v4().simple());
        let mut admin = self.stack.root.db.admin().await;
        sqlx::query(
            "INSERT INTO device_tokens (id, account_id, token_hash, device_name, platform, created_at) \
             VALUES (gen_random_uuid(), $1, sha256(convert_to($2, 'UTF8')), 'laptop', 'linux', \
             'epoch'::timestamptz + $3::bigint * interval '1 microsecond')",
        )
        .bind(account.id)
        .bind(&token)
        .bind(SystemClock::new().now_utc().as_unix_micros())
        .execute(&mut admin)
        .await
        .expect("insert device token");
        admin.close().await.expect("close admin connection");
        token
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
