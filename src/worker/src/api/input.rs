use media_shared::types::{ClaimedJob, JobKind};
use reqwest::StatusCode;
use sc_observability::log_error;
use tokio::fs::File;
use tokio::io::AsyncWriteExt;

use super::rollup::BlockRollup;
use super::{ApiError, Call, WorkerApiClient, expect_status, transient};

impl WorkerApiClient {
    /// `GET /input`: streams the plaintext into `dest`, hashing it per 2 MiB block on the way, and returns the
    /// byte count only when the roll-up equals `job.content_hash`.
    ///
    /// The limit is `worker_max_input_photo` for `stills` and `worker_max_input_video` for `poster` and
    /// `transcode`. A declared `Content-Length` over it is refused before the first byte is written; a body
    /// that passes it while streaming is abandoned at the limit. On any error `dest` is truncated to zero bytes,
    /// so a failed fetch never leaves a usable input behind.
    pub async fn fetch_input(
        &self,
        job: &ClaimedJob,
        dest: std::fs::File,
    ) -> Result<u64, ApiError> {
        let limit = self.input_limit(job.kind);
        let mut dest = File::from_std(dest);
        let result = self.stream_input(job, limit, &mut dest).await;
        if result.is_err()
            && let Err(error) = dest.set_len(0).await
        {
            log_error!("failed fetch left bytes in the input file", err = &error);
        }
        result
    }

    fn input_limit(&self, kind: JobKind) -> u64 {
        match kind {
            JobKind::Stills => self.max_input_photo,
            JobKind::Poster | JobKind::Transcode => self.max_input_video,
        }
    }

    async fn stream_input(
        &self,
        job: &ClaimedJob,
        limit: u64,
        dest: &mut File,
    ) -> Result<u64, ApiError> {
        let request = self.http.get(self.url(&format!("/{}/input", job.job_id)));
        let mut response = expect_status(self.send(request).await?, StatusCode::OK, Call::Input)?;
        if response
            .content_length()
            .is_some_and(|length| length > limit)
        {
            return Err(ApiError::TooLarge);
        }
        let mut rollup = BlockRollup::new();
        let mut received: u64 = 0;
        while let Some(chunk) = response.chunk().await.map_err(|e| transient(&e))? {
            received = received.saturating_add(u64::try_from(chunk.len()).unwrap_or(u64::MAX));
            if received > limit {
                return Err(ApiError::TooLarge);
            }
            rollup.push(&chunk);
            dest.write_all(&chunk)
                .await
                .map_err(ApiError::Destination)?;
        }
        dest.flush().await.map_err(ApiError::Destination)?;
        if rollup.finish() == job.content_hash {
            Ok(received)
        } else {
            Err(ApiError::HashMismatch)
        }
    }
}
