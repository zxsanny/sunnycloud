//! Shared pieces of the download tests: open calls through the `DownloadService` port, a body read to its end
//! (or its first error), and options built from the request headers 11 hands over.

use contracts::{
    ByteRange, Download, DownloadError, DownloadOptions, DownloadService, PlaintextStream,
    RevisionOrCurrent, StreamError,
};
use uuid::Uuid;

use crate::start_support::Rig;

/// The bytes a body yields before it ends, and the error that ended it, if any.
pub async fn drain(mut body: PlaintextStream) -> (Vec<u8>, Option<StreamError>) {
    let mut bytes = Vec::new();
    while let Some(chunk) = std::future::poll_fn(|cx| body.as_mut().poll_next(cx)).await {
        match chunk {
            Ok(chunk) => bytes.extend_from_slice(&chunk),
            Err(error) => return (bytes, Some(error)),
        }
    }
    (bytes, None)
}

/// The whole body of a download that must read cleanly.
pub async fn read_all(download: Download) -> Vec<u8> {
    let (bytes, error) = drain(download.body).await;
    assert!(error.is_none(), "body failed: {error:?}");
    bytes
}

pub fn ranged(start: u64, end: u64) -> DownloadOptions {
    DownloadOptions {
        range: Some(ByteRange { start, end }),
        ..DownloadOptions::default()
    }
}

/// A `start-` range as 11 hands it over: the open end is `u64::MAX`.
pub fn from(start: u64) -> DownloadOptions {
    ranged(start, u64::MAX)
}

pub fn quoted(etag: &str) -> String {
    format!("\"{etag}\"")
}

impl Rig {
    pub async fn open(
        &self,
        target: RevisionOrCurrent,
        options: DownloadOptions,
    ) -> Result<Download, DownloadError> {
        self.open_as(self.account, target, options).await
    }

    pub async fn open_as(
        &self,
        account: Uuid,
        target: RevisionOrCurrent,
        options: DownloadOptions,
    ) -> Result<Download, DownloadError> {
        let service: &dyn DownloadService = self.service.as_ref();
        service
            .open_revision_content(account, target, options)
            .await
    }

    pub async fn open_current(&self, node: Uuid) -> Result<Download, DownloadError> {
        self.open(RevisionOrCurrent::Current(node), DownloadOptions::default())
            .await
    }
}
