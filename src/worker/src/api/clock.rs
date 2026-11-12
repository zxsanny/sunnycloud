//! `GET /internal/v1/test/clock-offset` on the worker listener (feature `test-clock`, 09 section 2 "Test clock").

use std::time::Duration;

use reqwest::StatusCode;
use serde::Deserialize;

use super::{ApiError, Call, WorkerApiClient, read_json, status_error};

const CLOCK_OFFSET_PATH: &str = "/internal/v1/test/clock-offset";

#[derive(Deserialize)]
struct OffsetBody {
    offset_seconds: u64,
}

impl WorkerApiClient {
    /// The API's clock offset: 200 `{"offset_seconds": N}`. The bearer and `X-Worker-Id` are the client's
    /// default headers, as on every worker route.
    pub async fn clock_offset(&self) -> Result<Duration, ApiError> {
        let url = format!("{}{CLOCK_OFFSET_PATH}", self.base);
        let response = self.send(self.http.get(url)).await?;
        match response.status() {
            StatusCode::OK => {
                let body: OffsetBody = read_json(response).await?;
                Ok(Duration::from_secs(body.offset_seconds))
            }
            status => Err(status_error(status, Call::Unleased)),
        }
    }
}
