//! `download` (`sync_api.md`): one ranged `GET` per 2 MiB block, each block hashed and compared to the revision's
//! blocklist before the host sink sees it, the whole file checked against `content_hash` before `finish(true)`.
//! A request holds one block, so a reset costs at most the block in flight and memory stays at one block.

use std::io;
use std::sync::Arc;

use bytes::Bytes;
use sync_core_proto::{BLOCK_SIZE, FileHash, Hash32, RevisionRef, RevisionView, rollup};
use uuid::Uuid;

use crate::error::SyncError;
use crate::host::DownloadSink;
use crate::request::{Method, Request, RequestPath};
use crate::small::blocking;
use crate::tree_ops::{get_revision, send};

const BLOCK: u64 = BLOCK_SIZE as u64;
const ACCEPT: &str = "application/octet-stream, application/problem+json";

/// A download whose every block and whole file matched the revision.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct Verified {
    pub file_hash: FileHash,
}

/// Fetches `revision` into `sink`, continuing at the block that holds `resume_from` (aligned down to a block
/// boundary; the bytes before it are the host's, verified by an earlier call). The sink gets one `finish`:
/// `true` after a full match, `false` after any failure past the revision lookup.
pub(crate) async fn download(
    path: &RequestPath,
    revision: &RevisionRef,
    sink: Arc<dyn DownloadSink>,
    resume_from: u64,
) -> Result<Verified, SyncError> {
    let view = get_revision(path, revision.revision_id).await?;
    check_view(&view, revision, resume_from)?;
    let first = usize::try_from(align_down(resume_from) / BLOCK).unwrap_or(usize::MAX);
    let file_hash = match fetch_blocks(path, &view, first, &sink).await {
        Ok(hash) if hash == view.content_hash => hash,
        Ok(_) => {
            abandon(&sink).await;
            return Err(violation("blocks do not roll up to the content hash"));
        }
        Err(error) => {
            abandon(&sink).await;
            return Err(error);
        }
    };
    let closing = Arc::clone(&sink);
    blocking(move || closing.finish(true))
        .await?
        .map_err(io_error)?;
    Ok(Verified { file_hash })
}

/// The block boundary at or below `offset`.
fn align_down(offset: u64) -> u64 {
    offset - offset % BLOCK
}

fn check_view(
    view: &RevisionView,
    revision: &RevisionRef,
    resume_from: u64,
) -> Result<(), SyncError> {
    if view.content_hash != revision.content_hash {
        Err(violation("revision does not match its reference"))
    } else if u64::try_from(view.blocklist.len()).ok() != Some(view.size.div_ceil(BLOCK)) {
        Err(violation("blocklist does not cover the file size"))
    } else if resume_from > view.size {
        Err(violation("resume offset beyond the file"))
    } else {
        Ok(())
    }
}

/// Returns the rollup of all block hashes: the blocklist entries before `first` stand for the blocks the
/// host already holds, the rest are the hashes of the bytes received now.
async fn fetch_blocks(
    path: &RequestPath,
    view: &RevisionView,
    first: usize,
    sink: &Arc<dyn DownloadSink>,
) -> Result<FileHash, SyncError> {
    let mut hashes: Vec<Hash32> = view.blocklist[..first].to_vec();
    for (index, expected) in view.blocklist.iter().enumerate().skip(first) {
        let start = u64::try_from(index).unwrap_or(u64::MAX) * BLOCK;
        let len = (view.size - start).min(BLOCK);
        let body = fetch_range(path, view, start, len).await?;
        let block = u32::try_from(index).unwrap_or(u32::MAX);
        let (sink, expected) = (Arc::clone(sink), *expected);
        let hash = blocking(move || verify_and_write(sink.as_ref(), block, start, expected, body))
            .await??;
        hashes.push(hash);
    }
    Ok(rollup(&hashes))
}

/// The sink gets the bytes only when their hash is the blocklist's.
fn verify_and_write(
    sink: &dyn DownloadSink,
    block: u32,
    offset: u64,
    expected: Hash32,
    bytes: Bytes,
) -> Result<Hash32, SyncError> {
    let hash = sc_ids::hash_block(&bytes);
    if hash != expected {
        return Err(SyncError::HashMismatch { block });
    }
    sink.write_verified(offset, bytes).map_err(io_error)?;
    Ok(hash)
}

/// Exactly one `206` for `start..start + len`. A `200` (a server that ignored `Range`) or a different range
/// would put bytes at the wrong offset, so both are refused; a body of another length is a short or long stream.
async fn fetch_range(
    path: &RequestPath,
    view: &RevisionView,
    start: u64,
    len: u64,
) -> Result<Bytes, SyncError> {
    let last = start + len - 1;
    let request = Request {
        method: Method::Get,
        target: content_target(view.revision_id),
        headers: vec![
            ("accept".to_owned(), ACCEPT.to_owned()),
            ("range".to_owned(), format!("bytes={start}-{last}")),
        ],
        body: Bytes::new(),
        max_attempts: path.default_attempts(),
    };
    let response = send(path, "download", request).await?;
    let served = format!("bytes {start}-{last}/{}", view.size);
    if response.status != 206 || response.header("content-range").map(str::trim) != Some(&served) {
        return Err(violation("content answer is not the requested range"));
    }
    if u64::try_from(response.body.len()).ok() != Some(len) {
        return Err(violation("content ended before the requested range"));
    }
    Ok(response.body)
}

fn content_target(revision: Uuid) -> String {
    format!("/api/v1/revisions/{revision}/content")
}

/// Tells the sink the file is not verified. The caller already has the error that ended the download, so
/// a sink failure here is logged, not returned.
async fn abandon(sink: &Arc<dyn DownloadSink>) {
    let sink = Arc::clone(sink);
    match blocking(move || sink.finish(false)).await {
        Ok(Ok(())) => {}
        Ok(Err(error)) => {
            tracing::warn!(kind = ?error.kind(), "sink failed to close an unverified download")
        }
        Err(error) => tracing::warn!(%error, "unverified download was not closed"),
    }
}

fn io_error(error: io::Error) -> SyncError {
    SyncError::Io { kind: error.kind() }
}

fn violation(detail: &'static str) -> SyncError {
    SyncError::ProtocolViolation { detail }
}

#[cfg(test)]
mod tests {
    use super::{BLOCK, align_down};

    #[test]
    fn resume_offsets_align_down_to_a_block_boundary() {
        // Arrange
        let cases = [
            (0, 0),
            (1, 0),
            (BLOCK - 1, 0),
            (BLOCK, BLOCK),
            (BLOCK + 1, BLOCK),
            (7 * BLOCK, 7 * BLOCK),
            (7 * BLOCK + BLOCK / 2, 7 * BLOCK),
            (u64::MAX, u64::MAX - u64::MAX % BLOCK),
        ];

        // Act
        let aligned: Vec<u64> = cases
            .iter()
            .map(|(offset, _)| align_down(*offset))
            .collect();

        // Assert
        let expected: Vec<u64> = cases.iter().map(|(_, aligned)| *aligned).collect();
        assert_eq!(aligned, expected);
    }
}
