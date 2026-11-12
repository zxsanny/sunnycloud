//! Helpers shared by the security cases: a second account, blobs of chosen bytes for a chosen account, the
//! consumption of a plaintext stream and the text forms a leaked key could take.

use std::sync::Arc;

use bytes::Bytes;
use contracts::{
    BlobCatalog as _, BlobError, BlobHandle, BlobRef, BlobWriter, PlaintextStream, RegisterState,
    StreamError, WrittenBlob, WrittenOrPlanned,
};
use sc_db::Scope;
use sc_testdb::{MIGRATOR, START};
use sunnycloud_blobstore::FileBlobWriter;
use uuid::Uuid;

use crate::support::{MIB, Rig};

pub const BLOCK: usize = 2 * MIB;
pub const KNOWN: &str = "PASSWORD=hunter2";

pub async fn second_account(rig: &Rig) -> Uuid {
    let id = Uuid::new_v4();
    let mut conn = rig.db.conn_as(MIGRATOR).await;
    sqlx::query(
        "INSERT INTO accounts (id, state, created_at, display_name, role) \
         VALUES ($1, 'active', 'epoch'::timestamptz + $2::bigint * interval '1 microsecond', 'Other', 'user')",
    )
    .bind(id)
    .bind(START.as_unix_micros())
    .execute(&mut conn)
    .await
    .expect("insert account");
    sqlx::Connection::close(conn).await.expect("close");
    id
}

/// The marker of DS-09: 64 bytes, `SUNNYCLOUD-MARKER-<seed>` padded with `#`.
pub fn marker(seed: u64) -> Vec<u8> {
    let mut marker = format!("SUNNYCLOUD-MARKER-{seed}").into_bytes();
    marker.resize(64, b'#');
    marker
}

/// Dense bytes with the marker at every MiB and the known string right after it.
pub fn marked(seed: u64, len: usize) -> Arc<Vec<u8>> {
    let mut state = seed | 1;
    let mut bytes: Vec<u8> = (0..len)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            (state >> 32) as u8
        })
        .collect();
    let stamp = [marker(seed), KNOWN.as_bytes().to_vec()].concat();
    let mut at = 0;
    while at + stamp.len() <= len {
        bytes[at..at + stamp.len()].copy_from_slice(&stamp);
        at += MIB;
    }
    Arc::new(bytes)
}

pub fn block_of(data: &[u8], index: usize) -> Bytes {
    let start = (index * BLOCK).min(data.len());
    let end = (start + BLOCK).min(data.len());
    Bytes::copy_from_slice(&data[start..end])
}

/// Writes every block of `data` through `writer`, waits for each to be durable and registers the blob committed
/// for `account`; returns its reference as the catalog gives it to that account.
pub async fn store_with(rig: &Rig, writer: &FileBlobWriter, account: Uuid, data: &[u8]) -> BlobRef {
    let size = data.len() as i64;
    let plan = writer.plan(account, size).expect("plan");
    let handle = writer.create_file(plan, false).await.expect("create_file");
    for index in 0..data.len().div_ceil(BLOCK).max(1) {
        let token = writer
            .write_block(&handle, index as u32, block_of(data, index))
            .await
            .expect("block is written");
        writer.await_durable(token).await.expect("block is durable");
    }
    commit(rig, account, &handle).await
}

/// Registers the blob of `handle` committed for `account` and returns its reference as the catalog gives it.
pub async fn commit(rig: &Rig, account: Uuid, handle: &BlobHandle) -> BlobRef {
    let written = WrittenOrPlanned::Written(WrittenBlob {
        plan: handle.plan.clone(),
        size: handle.plaintext_size,
    });
    rig.api
        .run(Scope::Account(account), async |tx| {
            rig.registry
                .register_in_tx(tx, account, written.clone(), RegisterState::Committed)
                .await
        })
        .await
        .expect("blob row");
    rig.catalog
        .get_ref(account, handle.plan.blob_id)
        .await
        .expect("catalog lookup")
        .expect("committed blob has a reference")
}

pub async fn store(rig: &Rig, account: Uuid, data: &[u8]) -> BlobRef {
    store_with(rig, &rig.writer, account, data).await
}

/// What a full read produced: the plaintext bytes before the first failure and that failure, if any.
#[derive(Debug)]
pub struct Read {
    pub bytes: Vec<u8>,
    pub open: Option<BlobError>,
    pub stream: Option<StreamError>,
}

impl Read {
    pub fn failed(&self) -> bool {
        self.open.is_some() || self.stream.is_some()
    }
}

pub async fn read_all(reader: &dyn contracts::BlobReader, blob_id: Uuid, blob: &BlobRef) -> Read {
    match reader
        .open(
            blob_id,
            &blob.wrapped_key,
            blob.master_key_id,
            blob.size,
            None,
        )
        .await
    {
        Ok(stream) => consume(stream).await,
        Err(error) => Read {
            bytes: Vec::new(),
            open: Some(error),
            stream: None,
        },
    }
}

async fn consume(mut stream: PlaintextStream) -> Read {
    let mut bytes = Vec::new();
    while let Some(item) = std::future::poll_fn(|cx| stream.as_mut().poll_next(cx)).await {
        match item {
            Ok(part) => bytes.extend_from_slice(&part),
            Err(error) => {
                return Read {
                    bytes,
                    open: None,
                    stream: Some(error),
                };
            }
        }
    }
    Read {
        bytes,
        open: None,
        stream: None,
    }
}

/// Occurrences of `needle` in `haystack`.
pub fn hits(haystack: &[u8], needle: &[u8]) -> usize {
    let Some((&first, _)) = needle.split_first() else {
        return 0;
    };
    let mut count = 0;
    let mut from = 0;
    while let Some(at) = haystack[from..].iter().position(|byte| *byte == first) {
        let start = from + at;
        if haystack[start..].starts_with(needle) {
            count += 1;
        }
        from = start + 1;
    }
    count
}

const STANDARD: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn base64(bytes: &[u8], alphabet: &[u8; 64]) -> String {
    let mut text = String::new();
    for group in bytes.chunks(3) {
        let word = group.iter().enumerate().fold(0u32, |word, (i, byte)| {
            word | (u32::from(*byte) << (16 - 8 * i))
        });
        for i in 0..=group.len() {
            text.push(char::from(alphabet[((word >> (18 - 6 * i)) & 63) as usize]));
        }
    }
    text
}

/// The text forms the 32 bytes of a key could take: hex either case, base64 standard and URL-safe (the
/// unpadded prefix, so padded forms match too) and the `Debug` list of the bytes.
pub fn key_forms(key: &[u8]) -> Vec<String> {
    let hex: String = key.iter().map(|byte| format!("{byte:02x}")).collect();
    let mut url = *STANDARD;
    url[62] = b'-';
    url[63] = b'_';
    let list: Vec<String> = key.iter().map(u8::to_string).collect();
    vec![
        hex.clone(),
        hex.to_uppercase(),
        base64(key, STANDARD),
        base64(key, &url),
        list.join(", "),
    ]
}
