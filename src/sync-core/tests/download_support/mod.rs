#![allow(dead_code)]
//! Shared pieces of the download tests: a spying `DownloadSink`, revisions built from bytes and the script
//! of a revision served with ranges.

use std::io;
use std::sync::{Arc, Mutex, PoisonError};

use bytes::Bytes;
use sync_core::DownloadSink;
use sync_core_proto::{BLOCK_SIZE, Hash32, RevisionRef, RevisionView, hash_block, rollup};

use crate::support::{Answer, Fixture, Route, Script};
use crate::tree_support::id;

pub const BLOCK: usize = BLOCK_SIZE;

#[derive(Default)]
struct Seen {
    writes: Vec<(u64, Bytes)>,
    finishes: Vec<bool>,
}

/// Records every call. `fail_write_at` makes the write at that offset fail.
pub struct SpySink {
    seen: Mutex<Seen>,
    fail_write_at: Option<u64>,
}

impl SpySink {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            seen: Mutex::default(),
            fail_write_at: None,
        })
    }

    pub fn failing_at(offset: u64) -> Arc<Self> {
        Arc::new(Self {
            seen: Mutex::default(),
            fail_write_at: Some(offset),
        })
    }

    fn seen(&self) -> std::sync::MutexGuard<'_, Seen> {
        self.seen.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub fn offsets(&self) -> Vec<u64> {
        self.seen()
            .writes
            .iter()
            .map(|(offset, _)| *offset)
            .collect()
    }

    /// The offsets of blocks `from..to`.
    pub fn block_offsets(from: usize, to: usize) -> Vec<u64> {
        (from..to).map(|block| (block * BLOCK) as u64).collect()
    }

    /// All written bytes in order; panics when a write does not continue the previous one.
    pub fn content(&self) -> Vec<u8> {
        let seen = self.seen();
        let mut out = Vec::new();
        for (offset, bytes) in &seen.writes {
            assert_eq!(*offset, out.len() as u64, "writes are contiguous");
            out.extend_from_slice(bytes);
        }
        out
    }

    pub fn written_len(&self) -> u64 {
        self.seen()
            .writes
            .iter()
            .map(|(_, bytes)| bytes.len() as u64)
            .sum()
    }

    pub fn finishes(&self) -> Vec<bool> {
        self.seen().finishes.clone()
    }
}

impl DownloadSink for SpySink {
    fn write_verified(&self, offset: u64, bytes: Bytes) -> io::Result<()> {
        if self.fail_write_at == Some(offset) {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "disk refused",
            ));
        }
        self.seen().writes.push((offset, bytes));
        Ok(())
    }

    fn finish(&self, verified: bool) -> io::Result<()> {
        self.seen().finishes.push(verified);
        Ok(())
    }
}

/// The revision a server holds for `data`: block hashes and their rollup.
pub fn view_of(data: &[u8]) -> RevisionView {
    let hashes: Vec<Hash32> = data
        .chunks(BLOCK)
        .map(|block| hash_block(block).expect("a chunk is at most one block"))
        .collect();
    RevisionView {
        revision_id: id(0xD0),
        content_hash: rollup(&hashes),
        blocklist: hashes,
        size: data.len() as u64,
        client_meta: None,
    }
}

pub fn reference(view: &RevisionView) -> RevisionRef {
    RevisionRef {
        revision_id: view.revision_id,
        node_id: id(0xD1),
        content_hash: view.content_hash,
        size: view.size,
        client_meta: None,
    }
}

/// `GetRevision` answers with `view`; the content route is left to the caller.
pub fn revision_script(view: &RevisionView) -> Script {
    Script::new().always(Route::GetRevision, Answer::json(200, view))
}

/// `view` is served, and the content route answers every request with a fresh `answer(attempt)`, `attempt`
/// counting content requests from 0.
pub fn content_script(
    view: &RevisionView,
    answer: impl Fn(usize) -> Answer + Send + Sync + 'static,
) -> Script {
    let attempts = std::sync::atomic::AtomicUsize::new(0);
    revision_script(view).respond(Route::RevisionContent, move |_| {
        answer(attempts.fetch_add(1, std::sync::atomic::Ordering::SeqCst))
    })
}

/// The `Range` header of every content request, in arrival order.
pub fn ranges(fixture: &Fixture) -> Vec<String> {
    crate::tree_support::seen(fixture, Route::RevisionContent)
        .iter()
        .map(|request| request.header("range").unwrap_or("").to_owned())
        .collect()
}

/// `start` of `bytes=start-end`.
pub fn range_start(range: &str) -> u64 {
    range
        .strip_prefix("bytes=")
        .and_then(|spec| spec.split_once('-'))
        .and_then(|(start, _)| start.parse().ok())
        .expect("a bytes=start-end range")
}
