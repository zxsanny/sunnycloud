use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};

use bytes::Bytes;
use http_body::{Body, Frame, SizeHint};
use media_shared::types::{DerivedKind, OutputMeta};
use reqwest::header::{CONTENT_TYPE, HeaderValue};
use reqwest::{Body as RequestBody, StatusCode};
use tokio::fs::File;
use tokio::io::{AsyncRead, ReadBuf};
use uuid::Uuid;

use super::{ApiError, Call, WorkerApiClient, status_error};
use crate::scratch::OutputFile;

const CHUNK: usize = 64 * 1024;

impl WorkerApiClient {
    /// `PUT /outputs/{kind}`: streams the safe-opened output with its media type and, when given, dimensions
    /// and duration. 204 (accepted) and 200 (already present) are both `Ok`.
    pub async fn post_output(
        &self,
        job_id: Uuid,
        kind: DerivedKind,
        meta: &OutputMeta,
        output: OutputFile,
    ) -> Result<(), ApiError> {
        let media_type = HeaderValue::from_str(&meta.media_type).map_err(|_| {
            ApiError::Protocol("the output media type is not a header value".into())
        })?;
        let body = FileBody::new(output);
        let mut request = self
            .http
            .put(self.url(&format!("/{job_id}/outputs/{}", kind_segment(kind))))
            .header(CONTENT_TYPE, media_type)
            .body(RequestBody::wrap(body));
        if let Some(width) = meta.width {
            request = request.header("X-Width", width);
        }
        if let Some(height) = meta.height {
            request = request.header("X-Height", height);
        }
        if let Some(duration_ms) = meta.duration_ms {
            request = request.header("X-Duration-Ms", duration_ms);
        }
        let response = self.send(request).await?;
        match response.status() {
            StatusCode::NO_CONTENT | StatusCode::OK => Ok(()),
            status => Err(status_error(status, Call::Output)),
        }
    }
}

fn kind_segment(kind: DerivedKind) -> &'static str {
    match kind {
        DerivedKind::Thumb => "thumb",
        DerivedKind::Preview => "preview",
        DerivedKind::Transcode => "transcode",
    }
}

/// The checked output file as a request body of exactly the size `fstat` reported: a file that grows after the
/// check is cut at that size, and one that shrinks fails the request instead of sending a short body.
struct FileBody {
    file: File,
    remaining: u64,
    buf: Vec<u8>,
}

impl FileBody {
    fn new(output: OutputFile) -> Self {
        let remaining = output.size();
        Self {
            file: File::from_std(output.into_reader().into_inner()),
            remaining,
            buf: vec![0; CHUNK],
        }
    }
}

impl Body for FileBody {
    type Data = Bytes;
    type Error = io::Error;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Bytes>, io::Error>>> {
        let this = &mut *self;
        if this.remaining == 0 {
            return Poll::Ready(None);
        }
        let want = usize::try_from(this.remaining).map_or(CHUNK, |left| left.min(CHUNK));
        let mut read_buf = ReadBuf::new(&mut this.buf[..want]);
        match Pin::new(&mut this.file).poll_read(cx, &mut read_buf) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Err(error)) => Poll::Ready(Some(Err(error))),
            Poll::Ready(Ok(())) => {
                let filled = read_buf.filled();
                if filled.is_empty() {
                    return Poll::Ready(Some(Err(io::ErrorKind::UnexpectedEof.into())));
                }
                this.remaining = this
                    .remaining
                    .saturating_sub(u64::try_from(filled.len()).unwrap_or(u64::MAX));
                Poll::Ready(Some(Ok(Frame::data(Bytes::copy_from_slice(filled)))))
            }
        }
    }

    fn is_end_stream(&self) -> bool {
        self.remaining == 0
    }

    fn size_hint(&self) -> SizeHint {
        SizeHint::with_exact(self.remaining)
    }
}
