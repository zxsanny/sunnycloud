//! AC-3: a block PUT reaches `put_block` with the upload id, the index, the hash, the length and the body as a
//! stream, and is answered 204 only after the port returns its acknowledgement.

use std::future::poll_fn;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use contracts::{
    BlockAck, ByteStream, Staged, StartUpload, StartUploadOutcome, UploadError, UploadService,
    UploadState,
};
use sc_ids::Hash32;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::oneshot;
use uuid::Uuid;

use super::support::*;
use crate::auth::support::{MEMBER_ACCOUNT, MEMBER_TOKEN};
use crate::harness::EVENT_BOUND;
use crate::harness::script::{Script, unscripted};

const BLOCK: usize = 2 * 1024 * 1024;
const FIRST: usize = 64 * 1024;

/// Hands the test one signal when the first chunk of the body arrives, one when the body is read to its end, and
/// returns the acknowledgement only after the test releases it.
struct GatedUploads {
    script: Script,
    first_chunk: Mutex<Option<oneshot::Sender<usize>>>,
    drained: Mutex<Option<oneshot::Sender<usize>>>,
    release: Mutex<Option<oneshot::Receiver<()>>>,
}

fn take<T>(slot: &Mutex<Option<T>>) -> Option<T> {
    slot.lock().unwrap_or_else(PoisonError::into_inner).take()
}

#[contracts::async_trait]
impl UploadService for GatedUploads {
    async fn put_block(
        &self,
        account: Uuid,
        upload_id: Uuid,
        index: u32,
        expected_hash: Hash32,
        mut body: ByteStream,
        length: u64,
    ) -> Result<BlockAck, UploadError> {
        let mut total = 0;
        let mut signal = take(&self.first_chunk);
        while let Some(chunk) = poll_fn(|cx| body.as_mut().poll_next(cx)).await {
            total += chunk.expect("the body reaches the port intact").len();
            if let Some(first) = signal.take() {
                first
                    .send(total)
                    .expect("the test waits for the first chunk");
            }
        }
        self.script.record(format!(
            "put_block ({account}, {upload_id}, {index}, {expected_hash}, length={length}, bytes={total})"
        ));
        if let Some(drained) = take(&self.drained) {
            drained
                .send(total)
                .expect("the test waits for the end of the body");
        }
        if let Some(release) = take(&self.release) {
            release
                .await
                .expect("the test releases the acknowledgement");
        }
        Ok(BlockAck)
    }

    async fn start_upload(
        &self,
        _account: Uuid,
        _request: StartUpload,
    ) -> Result<StartUploadOutcome, UploadError> {
        Err(UploadError::Unexpected(unscripted(
            "UploadService::start_upload",
        )))
    }

    async fn get_upload(
        &self,
        _account: Uuid,
        _upload_id: Uuid,
    ) -> Result<UploadState, UploadError> {
        Err(UploadError::Unexpected(unscripted(
            "UploadService::get_upload",
        )))
    }

    async fn abort_upload(&self, _account: Uuid, _upload_id: Uuid) -> Result<(), UploadError> {
        Err(UploadError::Unexpected(unscripted(
            "UploadService::abort_upload",
        )))
    }

    async fn stage_small(
        &self,
        _account: Uuid,
        _staged_key: Uuid,
        _expected_hash: Option<Hash32>,
        _body: ByteStream,
        _length: u64,
    ) -> Result<Staged, UploadError> {
        Err(UploadError::Unexpected(unscripted(
            "UploadService::stage_small",
        )))
    }
}

#[tokio::test]
async fn ac3_the_block_is_streamed_with_its_hash_and_length_and_answered_204_only_after_the_acknowledgement()
 {
    // Arrange
    let (first_tx, first_rx) = oneshot::channel();
    let (drained_tx, drained_rx) = oneshot::channel();
    let (release_tx, release_rx) = oneshot::channel();
    let stack = Stack::start_with(|script, services| sunnycloud_http_api::ApiServices {
        uploads: Arc::new(GatedUploads {
            script: script.clone(),
            first_chunk: Mutex::new(Some(first_tx)),
            drained: Mutex::new(Some(drained_tx)),
            release: Mutex::new(Some(release_rx)),
        }),
        ..services
    })
    .await;
    let head = format!(
        "PUT /api/v1/uploads/{UPLOAD}/blocks/4 HTTP/1.1\r\nhost: sunnycloud.test\r\nauthorization: Bearer {MEMBER_TOKEN}\r\nx-blake3: {}\r\ncontent-length: {BLOCK}\r\nconnection: close\r\n\r\n",
        hex(9)
    );
    let mut socket = TcpStream::connect(stack.public).await.expect("tcp connect");

    // Act
    socket.write_all(head.as_bytes()).await.expect("send head");
    socket
        .write_all(&vec![7u8; FIRST])
        .await
        .expect("send first part");
    let first = tokio::time::timeout(EVENT_BOUND, first_rx)
        .await
        .expect("the port gets the body before it has all arrived")
        .expect("first chunk signal");
    socket
        .write_all(&vec![7u8; BLOCK - FIRST])
        .await
        .expect("send the rest");
    let total = tokio::time::timeout(EVENT_BOUND, drained_rx)
        .await
        .expect("the port reads the whole body")
        .expect("drained signal");
    let mut early = [0u8; 64];
    let before_ack =
        tokio::time::timeout(Duration::from_millis(300), socket.read(&mut early)).await;
    release_tx
        .send(())
        .expect("the port is waiting for the release");
    let mut answer = Vec::new();
    tokio::time::timeout(EVENT_BOUND, socket.read_to_end(&mut answer))
        .await
        .expect("the answer arrives after the release")
        .expect("read the answer");

    // Assert
    assert!(
        first <= FIRST,
        "{first}: the first chunk is part of what was sent so far"
    );
    assert_eq!(total, BLOCK);
    assert!(
        before_ack.is_err(),
        "an answer was sent before the port acknowledged the block"
    );
    let text = String::from_utf8_lossy(&answer);
    assert!(text.starts_with("HTTP/1.1 204"), "{text}");
    assert_eq!(
        stack.script.log(),
        [format!(
            "put_block ({MEMBER_ACCOUNT}, {UPLOAD}, 4, {}, length={BLOCK}, bytes={BLOCK})",
            hex(9)
        )]
    );
    stack.stop().await;
}
