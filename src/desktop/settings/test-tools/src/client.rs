//! A bare JSON-RPC line client for tests of the stub itself: it sends exactly the bytes it
//! is told to and reports exactly the lines it reads. It carries no window logic.

use std::io;
use std::time::Duration;

use ipc_schema::{LineBuffer, Notification, Request, RequestId, Response, ServerMessage, Version};
use serde_json::Value;
use tokio::io::{AsyncReadExt, AsyncWriteExt, ReadHalf, WriteHalf};
use tokio::time::timeout;

#[cfg(unix)]
type Stream = tokio::net::UnixStream;
#[cfg(windows)]
type Stream = tokio::net::windows::named_pipe::NamedPipeClient;

#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    #[error("no complete line within the wait")]
    Timeout,
    #[error("the stub closed the connection")]
    Closed,
    #[error("i/o: {0}")]
    Io(#[from] io::Error),
    #[error("line is not a server message: {0}")]
    Decode(#[from] serde_json::Error),
}

pub struct RawClient {
    reader: ReadHalf<Stream>,
    writer: WriteHalf<Stream>,
    lines: LineBuffer,
    next_id: i64,
    /// Notifications that arrived while a call waited for its response.
    pub events: Vec<Notification>,
}

impl RawClient {
    pub async fn connect(endpoint: &str) -> io::Result<Self> {
        #[cfg(unix)]
        let stream = Stream::connect(endpoint).await?;
        #[cfg(windows)]
        let stream = tokio::net::windows::named_pipe::ClientOptions::new().open(endpoint)?;
        let (reader, writer) = tokio::io::split(stream);
        Ok(Self {
            reader,
            writer,
            lines: LineBuffer::default(),
            next_id: 1,
            events: Vec::new(),
        })
    }

    /// Sends `bytes` followed by `\n`, unchanged.
    pub async fn send_line(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.writer.write_all(bytes).await?;
        self.writer.write_all(b"\n").await?;
        self.writer.flush().await
    }

    /// Sends a request with the next id and returns that id.
    pub async fn send_request(&mut self, method: &str, params: Value) -> Result<i64, ClientError> {
        let id = self.next_id;
        self.next_id += 1;
        let request = Request {
            jsonrpc: Version,
            id: RequestId::Number(id),
            method: method.to_owned(),
            params: Some(params),
        };
        self.send_line(&serde_json::to_vec(&request)?).await?;
        Ok(id)
    }

    /// The next line, or `Timeout` when none completes within `within`.
    pub async fn read_line(&mut self, within: Duration) -> Result<Vec<u8>, ClientError> {
        let mut chunk = vec![0_u8; 64 * 1024];
        timeout(within, async {
            loop {
                if let Some(line) = self.lines.next_line() {
                    return Ok(line);
                }
                let read = self.reader.read(&mut chunk).await?;
                if read == 0 {
                    return Err(ClientError::Closed);
                }
                self.lines.push(&chunk[..read]);
            }
        })
        .await
        .map_err(|_| ClientError::Timeout)?
    }

    pub async fn read_message(&mut self, within: Duration) -> Result<ServerMessage, ClientError> {
        let line = self.read_line(within).await?;
        Ok(serde_json::from_slice(&line)?)
    }

    /// One request and its response; notifications that arrive first go to `events`.
    pub async fn call(
        &mut self,
        method: &str,
        params: Value,
        within: Duration,
    ) -> Result<Response, ClientError> {
        let id = RequestId::Number(self.send_request(method, params).await?);
        loop {
            match self.read_message(within).await? {
                ServerMessage::Response(response) if response.id.as_ref() == Some(&id) => {
                    return Ok(response);
                }
                ServerMessage::Response(other) => {
                    return Err(ClientError::Decode(serde::de::Error::custom(format!(
                        "response for another id: {other:?}"
                    ))));
                }
                ServerMessage::Notification(event) => self.events.push(event),
            }
        }
    }
}
