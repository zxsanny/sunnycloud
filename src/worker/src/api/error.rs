use std::io;

use thiserror::Error;

/// One typed outcome per failed call; no caller inspects an HTTP status. No variant carries the bearer, a header
/// or a URL.
#[derive(Debug, Error)]
pub enum ApiError {
    /// 403: the worker secret is wrong. The supervisor stops claiming.
    #[error("the API refused the worker secret")]
    Forbidden,
    /// 409: another worker holds the job or the lease expired. Stop quietly and post nothing more for the job.
    #[error("the lease on the job is lost")]
    LeaseLost,
    /// 404 on input or output: the revision is collected or foreign. The job closes as `obsolete`.
    #[error("the revision is obsolete")]
    Gone,
    /// An input over `worker_max_input_photo` / `worker_max_input_video` (declared or streamed), or a 413 on an
    /// output.
    #[error("over the size limit")]
    TooLarge,
    /// 422 on an output.
    #[error("the API rejected the output type")]
    TypeRejected,
    /// The streamed input does not roll up to `ClaimedJob.content_hash`. Retry-class failure.
    #[error("the input does not match the claimed content hash")]
    HashMismatch,
    /// A refused or broken connection, a timeout or a 5xx.
    #[error("transient failure: {0}")]
    Transient(String),
    /// An answer the contract does not describe: another status, or a body that does not parse.
    #[error("unexpected answer from the API: {0}")]
    Protocol(String),
    /// The input file could not be written.
    #[error("cannot write the input file: {0}")]
    Destination(io::Error),
}

/// Why the client could not be built. No variant carries the bearer or the worker id.
#[derive(Debug, Error)]
pub enum ApiConfigError {
    #[error("worker_api_url is not an http URL")]
    Url,
    #[error("the worker bearer is not a valid header value")]
    Token,
    #[error("worker_id is not a valid header value")]
    WorkerId,
    #[error("cannot build the HTTP client: {0}")]
    Client(String),
}
