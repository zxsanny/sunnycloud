//! `DownloadService` (`download_service.md`): stored content streamed back through 04's reader.
//!
//! Every lookup is account-scoped, and a foreign node, revision or blob is `NotFound`. `Current(node)` goes through
//! `TreeReader::resolve_live_file` (a folder or a gone node is `NotFound`); `Revision` through `get_revision`, and
//! a revision that is not the current one of a live file is `NotFound` unless `retained_ok` (set by 11 for an
//! admin principal only). A blob reference from 11 is checked against the account's own blobs and served with
//! the catalog's key material. The serving rules are in `crate::serving`. The reader verifies the first chunk
//! before `open` returns, so the result exists only for readable content; a later corrupt chunk ends the body
//! with `StreamError::Corrupt`. A missing or corrupt blob is `ContentUnavailable` (04's reader marks it). HEAD
//! uses the same call; 11 drops the body.

use contracts::{
    BlobError, BlobRef, Download, DownloadError, DownloadOptions, DownloadStatus, LiveFile,
    LiveFileLookup, NodeKind, NodeSelector, RevisionOrCurrent, RevisionView, TreeReaderError,
};
use thiserror::Error;
use uuid::Uuid;

use crate::serving::{Serve, counted, decide, disposition, no_body};
use crate::upload::PgUploadService;

/// The rows of 02 and 04 disagree about a committed revision.
#[derive(Debug, Error)]
enum DownloadFault {
    #[error("the blob of a revision has no readable row")]
    RevisionBlobMissing,
    #[error("a blob size is negative")]
    NegativeSize,
}

/// What is served: the blob, its sniffed type and the name a save dialog suggests.
struct Content {
    blob: BlobRef,
    media_type: Option<String>,
    suggested_name: String,
}

impl PgUploadService {
    pub async fn open_revision_content(
        &self,
        account: Uuid,
        target: RevisionOrCurrent,
        options: DownloadOptions,
    ) -> Result<Download, DownloadError> {
        let revision = match target {
            RevisionOrCurrent::Current(node) => self
                .current_revision(account, node)
                .await?
                .ok_or(DownloadError::NotFound)?,
            RevisionOrCurrent::Revision(revision_id) => {
                let revision = self
                    .tree
                    .get_revision(account, revision_id)
                    .await
                    .map_err(reader_error)?;
                let current = self.current_revision(account, revision.node_id).await?;
                let is_current = current.is_some_and(|current| current.id == revision.id);
                if !is_current && !options.retained_ok {
                    return Err(DownloadError::NotFound);
                }
                revision
            }
        };
        let node = self
            .tree
            .get_node(account, NodeSelector::Id(revision.node_id))
            .await
            .map_err(reader_error)?;
        let blob = self
            .catalog
            .get_ref(account, revision.blob_id)
            .await
            .map_err(|error| DownloadError::Unexpected(Box::new(error)))?
            .ok_or_else(|| {
                DownloadError::Unexpected(Box::new(DownloadFault::RevisionBlobMissing))
            })?;
        let content = Content {
            blob,
            media_type: revision.media_type,
            suggested_name: node.name,
        };
        self.serve(content, options).await
    }

    /// No suggested name: a `BlobRef` carries none (owner decision 2026-11-14); 11 falls back to its own.
    pub async fn open_blob_content(
        &self,
        account: Uuid,
        blob: BlobRef,
        media_type: Option<String>,
        options: DownloadOptions,
    ) -> Result<Download, DownloadError> {
        let blob = self
            .catalog
            .get_ref(account, blob.blob_id)
            .await
            .map_err(|error| DownloadError::Unexpected(Box::new(error)))?
            .ok_or(DownloadError::NotFound)?;
        let content = Content {
            blob,
            media_type,
            suggested_name: String::new(),
        };
        self.serve(content, options).await
    }

    /// The current revision of a live file; `None` for a folder or a gone node.
    async fn current_revision(
        &self,
        account: Uuid,
        node: Uuid,
    ) -> Result<Option<RevisionView>, DownloadError> {
        match self.tree.resolve_live_file(account, node).await {
            Ok(LiveFileLookup::Found(LiveFile {
                kind: NodeKind::File,
                revision,
                ..
            })) => Ok(revision),
            Ok(LiveFileLookup::Found(_) | LiveFileLookup::Gone) => Ok(None),
            Err(error) => Err(reader_error(error)),
        }
    }

    async fn serve(
        &self,
        content: Content,
        options: DownloadOptions,
    ) -> Result<Download, DownloadError> {
        let Content {
            blob,
            media_type,
            suggested_name,
        } = content;
        let etag = blob.blob_id.to_string();
        let total = u64::try_from(blob.size)
            .map_err(|_| DownloadError::Unexpected(Box::new(DownloadFault::NegativeSize)))?;
        let serve = decide(
            &etag,
            total,
            options.range,
            options.if_range.as_deref(),
            options.if_none_match.as_deref(),
        );
        let (status, range) = match serve {
            Serve::Unsatisfiable => {
                return Err(DownloadError::RangeNotSatisfiable {
                    total_size: blob.size,
                });
            }
            Serve::NotModified => (DownloadStatus::NotModified, None),
            Serve::Full => (DownloadStatus::Full, None),
            Serve::Partial(range) => (DownloadStatus::Partial, Some(range)),
        };
        let body = if status == DownloadStatus::NotModified {
            no_body()
        } else {
            let body = self
                .reader
                .open(
                    blob.blob_id,
                    &blob.wrapped_key,
                    blob.master_key_id,
                    blob.size,
                    range,
                )
                .await
                .map_err(|error| blob_error(error, blob.size))?;
            counted(body, self.metrics.as_ref())
        };
        Ok(Download {
            status,
            total_size: blob.size,
            range,
            etag,
            disposition: disposition(media_type.as_deref()),
            media_type,
            suggested_name,
            body,
        })
    }
}

fn reader_error(error: TreeReaderError) -> DownloadError {
    match error {
        TreeReaderError::NotFound => DownloadError::NotFound,
        other => DownloadError::Unexpected(Box::new(other)),
    }
}

fn blob_error(error: BlobError, total_size: i64) -> DownloadError {
    match error {
        BlobError::BlobMissing | BlobError::Corrupt => DownloadError::ContentUnavailable,
        BlobError::RangeNotSatisfiable => DownloadError::RangeNotSatisfiable { total_size },
        other => DownloadError::Storage(Box::new(other)),
    }
}
