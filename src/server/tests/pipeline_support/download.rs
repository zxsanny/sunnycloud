//! Flow F6 on the [`Pipeline`]: a file too large to hold in memory, generated block by block and sent over the F4
//! routes; small files published through the staged path (F5); a streaming HTTP/1.1 read of the content routes that
//! keeps the headers, hashes the body as it arrives and tells a complete body from a cut one; and the probes of the
//! blob file and its row that the corruption case needs.

use std::fs::OpenOptions;
use std::io::{Read, Seek, SeekFrom, Write};
use std::net::TcpStream;
use std::path::Path;
use std::time::{Duration, Instant};

use hyper::body::Bytes;
use sc_ids::{BLOCK_SIZE, FileHash, FileHasher, Hash32, hash_block, rollup};
use serde_json::{Value, json};
use sqlx::Connection;
use tokio::task::JoinSet;
use uuid::Uuid;

use super::h2::H2;
use super::{File, Pipeline, content, json_type};
use crate::serve_support::http::Response;

const IO_TIMEOUT: Duration = Duration::from_secs(60);
const READ_BUFFER: usize = 64 * 1024;
/// 04's blob file: a 64-byte header, then each 64 KiB chunk sealed with a 16-byte tag.
const BLOB_HEADER_LEN: u64 = 64;
const SEALED_CHUNK_LEN: u64 = 64 * 1024 + 16;

/// A file of `size` bytes whose block `i` is generated on demand, so a 1 GiB file never sits in memory.
pub struct GiantFile {
    pub name: String,
    pub size: usize,
    seed: u64,
}

impl GiantFile {
    pub fn new(name: &str, size: usize, seed: u64) -> GiantFile {
        GiantFile {
            name: name.to_owned(),
            size,
            seed,
        }
    }

    pub fn blocks(&self) -> usize {
        self.size.div_ceil(BLOCK_SIZE)
    }

    pub fn block(&self, index: usize) -> Vec<u8> {
        let len = (self.size - index * BLOCK_SIZE).min(BLOCK_SIZE);
        let seed = self.seed * 1_000_003 + u64::try_from(index).expect("small");
        content(seed, len)
    }

    /// The `len` bytes from `start`, across block boundaries.
    pub fn slice(&self, start: usize, len: usize) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(len);
        let mut at = start;
        while bytes.len() < len {
            let (index, offset) = (at / BLOCK_SIZE, at % BLOCK_SIZE);
            let block = self.block(index);
            let take = (block.len() - offset).min(len - bytes.len());
            bytes.extend_from_slice(&block[offset..offset + take]);
            at += take;
        }
        bytes
    }

    pub fn blocklist(&self) -> Vec<Hash32> {
        (0..self.blocks())
            .map(|index| hash_block(&self.block(index)))
            .collect()
    }
}

/// One read of a content route: the head, and the body as it arrived.
pub struct Fetched {
    pub status: u16,
    headers: Vec<(String, String)>,
    /// `Content-Length`.
    pub declared: Option<u64>,
    pub received: u64,
    /// The body, when the read was asked to keep it.
    pub body: Vec<u8>,
    /// The file hash (2 MiB blocks) of the bytes received.
    pub file_hash: FileHash,
    /// From the request to the first body byte.
    pub first_byte: Option<Duration>,
    /// From the request to the end of the body.
    pub total: Duration,
    /// The transport error that ended the body, if one did; `None` is a clean end of stream.
    pub cut: Option<String>,
}

impl Fetched {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }

    /// The body ended cleanly with every declared byte.
    pub fn complete(&self) -> bool {
        self.cut.is_none() && self.declared == Some(self.received)
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

    /// The unquoted `ETag`.
    pub fn etag(&self) -> String {
        self.header("etag")
            .map(|tag| tag.trim_matches('"').to_owned())
            .expect("an ETag")
    }
}

/// The file hash of a stream, block by block.
struct StreamHash {
    hasher: FileHasher,
    in_block: usize,
}

impl StreamHash {
    fn update(&mut self, mut bytes: &[u8]) {
        while !bytes.is_empty() {
            let take = (BLOCK_SIZE - self.in_block).min(bytes.len());
            self.hasher.update(&bytes[..take]).expect("within a block");
            self.in_block += take;
            bytes = &bytes[take..];
            if self.in_block == BLOCK_SIZE {
                self.hasher.finish_block();
                self.in_block = 0;
            }
        }
    }
}

impl Pipeline {
    /// `POST /api/v1/uploads` of `size` bytes named `name` under `parent`; the session id.
    pub fn start_sized(&self, parent: Uuid, name: &str, size: usize) -> Uuid {
        let body = json!({
            "parent_id": parent,
            "name": name,
            "declared_size": size,
            "client_key": format!("{name}-{}", Uuid::new_v4().simple()),
        });
        let started = self.call(
            "POST",
            "/api/v1/uploads",
            &[json_type()],
            body.to_string().as_bytes(),
        );
        assert_eq!(started.status, 200, "{started:?}");
        id(&started.json(), "upload_id")
    }

    /// Every block of `file` over `lanes` h2c streams, each block generated when its lane sends it.
    pub async fn put_generated(&self, upload: Uuid, file: &GiantFile, lanes: usize) {
        let h2 = H2::connect(self.public()).await;
        let mut running = JoinSet::new();
        for lane in 0..lanes {
            let (h2, device) = (h2.clone(), self.device.clone());
            let file = GiantFile::new(&file.name, file.size, file.seed);
            running.spawn(async move {
                for index in (lane..file.blocks()).step_by(lanes) {
                    let block = Bytes::from(file.block(index));
                    let headers = [
                        ("Content-Type", "application/octet-stream".to_owned()),
                        ("X-Blake3", hash_block(&block).to_string()),
                    ];
                    let path = format!("/api/v1/uploads/{upload}/blocks/{index}");
                    let acked = h2.call("PUT", &path, &device, &headers, block).await;
                    assert_eq!(acked.status, 204, "block {index}: {acked:?}");
                }
            });
        }
        while let Some(lane) = running.join_next().await {
            lane.expect("a lane ends without a panic");
        }
    }

    /// `POST /api/v1/commits` of `blocklist`; the node and revision ids.
    pub fn commit_blocklist(
        &self,
        upload: Uuid,
        parent: Uuid,
        name: &str,
        blocklist: &[Hash32],
    ) -> (Uuid, Uuid) {
        let hashes: Vec<String> = blocklist.iter().map(ToString::to_string).collect();
        let body =
            json!({"upload_id": upload, "parent_id": parent, "name": name, "blocklist": hashes});
        let committed = self.call(
            "POST",
            "/api/v1/commits",
            &[json_type()],
            body.to_string().as_bytes(),
        );
        assert_eq!(committed.status, 200, "{committed:?}");
        let committed = committed.json();
        (id(&committed, "node_id"), id(&committed, "revision_id"))
    }

    /// `file` staged and published under `parent` by its own `commits:batch`, as a new revision of the file whose
    /// current revision is `base` when given; the node and revision ids.
    pub fn publish_small(&self, parent: Uuid, file: &File, base: Option<Uuid>) -> (Uuid, Uuid) {
        let staged = self.stage(file);
        assert_eq!(staged.status, 200, "{staged:?}");
        let mut entry = super::entry(file.key, parent, &file.name);
        if let Some(base) = base {
            entry["base_revision"] = json!(base);
        }
        let committed = self.commit_batch(Uuid::new_v4(), &[entry]);
        assert_eq!(committed.status, 200, "{committed:?}");
        let result = &committed.json()["entries"][0];
        assert_eq!(result["status"], "committed", "{result}");
        (id(result, "node_id"), id(result, "revision_id"))
    }

    /// `DELETE /api/v1/nodes/{node}`.
    pub fn delete_file(&self, node: Uuid) -> Response {
        self.call("DELETE", &format!("/api/v1/nodes/{node}"), &[], &[])
    }

    /// `GET path` as the device over its own HTTP/1.1 connection, read to the end of the stream.
    pub fn fetch(&self, path: &str, headers: &[(&str, String)], keep: bool) -> Fetched {
        fetch(self.public(), path, &self.device, headers, keep)
    }

    /// XORs one byte of chunk `chunk` of the blob file, inside its ciphertext.
    pub fn flip_chunk_byte(&self, blob: Uuid, chunk: u64) {
        flip_byte(
            &self.blob_file(blob),
            BLOB_HEADER_LEN + chunk * SEALED_CHUNK_LEN + 1000,
        );
    }

    /// True once 04's reader has marked the blob row unavailable.
    pub async fn blob_flagged(&self, blob: Uuid) -> bool {
        let mut admin = self.stack.root.db.admin().await;
        let flagged: bool =
            sqlx::query_scalar("SELECT unavailable_at IS NOT NULL FROM blobs WHERE id = $1")
                .bind(blob)
                .fetch_one(&mut admin)
                .await
                .expect("read the blob row");
        admin.close().await.expect("close admin connection");
        flagged
    }
}

pub fn id(value: &Value, member: &str) -> Uuid {
    value[member]
        .as_str()
        .and_then(|id| Uuid::parse_str(id).ok())
        .unwrap_or_else(|| panic!("no `{member}` in {value}"))
}

fn flip_byte(path: &Path, offset: u64) {
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    let mut byte = [0u8];
    file.seek(SeekFrom::Start(offset)).expect("seek");
    file.read_exact(&mut byte).expect("read the byte");
    byte[0] ^= 0x01;
    file.seek(SeekFrom::Start(offset)).expect("seek back");
    file.write_all(&byte).expect("write the flipped byte");
    file.sync_all().expect("sync the blob file");
}

fn fetch(
    public: std::net::SocketAddr,
    path: &str,
    token: &str,
    headers: &[(&str, String)],
    keep: bool,
) -> Fetched {
    let sent = Instant::now();
    let mut stream = TcpStream::connect(public).expect("tcp connect");
    stream.set_read_timeout(Some(IO_TIMEOUT)).expect("timeout");
    let mut head = format!(
        "GET {path} HTTP/1.1\r\nHost: cloud.example.org\r\nConnection: close\r\n\
         Authorization: Bearer {token}\r\n"
    );
    for (name, value) in headers {
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    head.push_str("\r\n");
    stream.write_all(head.as_bytes()).expect("the request head");
    let (status, response_headers, early) = read_head(&mut stream, path);
    let mut fetched = Fetched {
        status,
        declared: response_headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case("content-length"))
            .and_then(|(_, value)| value.parse().ok()),
        headers: response_headers,
        received: 0,
        body: Vec::new(),
        file_hash: rollup(&[]),
        first_byte: None,
        total: Duration::ZERO,
        cut: None,
    };
    assert!(
        fetched.header("transfer-encoding").is_none(),
        "GET {path}: a chunked answer"
    );
    let mut hash = StreamHash {
        hasher: FileHasher::new(),
        in_block: 0,
    };
    let mut take = |bytes: &[u8], fetched: &mut Fetched| {
        if bytes.is_empty() {
            return;
        }
        fetched.first_byte.get_or_insert_with(|| sent.elapsed());
        fetched.received += bytes.len() as u64;
        hash.update(bytes);
        if keep {
            fetched.body.extend_from_slice(bytes);
        }
    };
    take(&early, &mut fetched);
    let mut buffer = vec![0u8; READ_BUFFER];
    loop {
        match stream.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => take(&buffer[..read], &mut fetched),
            Err(error) => {
                fetched.cut = Some(error.to_string());
                break;
            }
        }
    }
    fetched.total = sent.elapsed();
    fetched.file_hash = hash.hasher.finish().1;
    fetched
}

/// The status, the headers, and the body bytes read along with the head.
fn read_head(stream: &mut TcpStream, path: &str) -> (u16, Vec<(String, String)>, Vec<u8>) {
    let mut raw = Vec::new();
    let mut buffer = [0u8; 4096];
    let split = loop {
        if let Some(at) = raw.windows(4).position(|window| window == b"\r\n\r\n") {
            break at;
        }
        let read = stream
            .read(&mut buffer)
            .unwrap_or_else(|error| panic!("GET {path}: the head: {error}"));
        assert!(
            read > 0,
            "GET {path}: the connection closed before the head"
        );
        raw.extend_from_slice(&buffer[..read]);
    };
    let head = std::str::from_utf8(&raw[..split]).expect("the head is UTF-8");
    let mut lines = head.split("\r\n");
    let status = lines
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|code| code.parse().ok())
        .expect("a status line");
    let headers = lines
        .filter_map(|line| line.split_once(':'))
        .map(|(name, value)| (name.trim().to_owned(), value.trim().to_owned()))
        .collect();
    (status, headers, raw[split + 4..].to_vec())
}
