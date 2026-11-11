use std::net::SocketAddr;

use http_body_util::{BodyExt, Empty};
use hyper::body::Bytes;
use hyper::client::conn::{http1, http2};
use hyper::{Request, StatusCode};
use hyper_util::rt::{TokioExecutor, TokioIo};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

use super::EVENT_BOUND;

pub type H1Sender = http1::SendRequest<Empty<Bytes>>;

/// An HTTP/1.1 connection that can carry several requests.
pub async fn h1_connect(addr: SocketAddr) -> H1Sender {
    let stream = TcpStream::connect(addr).await.expect("tcp connect");
    let (sender, connection) = http1::handshake(TokioIo::new(stream))
        .await
        .expect("h1 handshake");
    tokio::spawn(connection);
    sender
}

pub fn get_request(path: &str) -> Request<Empty<Bytes>> {
    Request::get(path)
        .header("host", "sunnycloud.test")
        .body(Empty::new())
        .expect("request")
}

pub async fn read_body(response: hyper::Response<hyper::body::Incoming>) -> (StatusCode, String) {
    let status = response.status();
    let body = response
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes();
    (status, String::from_utf8_lossy(&body).into_owned())
}

/// `GET path` over HTTP/1.1 on a fresh connection.
pub async fn h1_get(addr: SocketAddr, path: &str) -> (StatusCode, String) {
    let mut sender = h1_connect(addr).await;
    let response = sender
        .send_request(get_request(path))
        .await
        .expect("h1 response");
    read_body(response).await
}

/// `GET path` over h2c with prior knowledge on a fresh connection.
pub async fn h2c_get(addr: SocketAddr, path: &str) -> (StatusCode, String) {
    let stream = TcpStream::connect(addr).await.expect("tcp connect");
    let (mut sender, connection) = http2::handshake(TokioExecutor::new(), TokioIo::new(stream))
        .await
        .expect("h2 handshake");
    tokio::spawn(connection);
    let response = sender
        .send_request(get_request(path))
        .await
        .expect("h2 response");
    read_body(response).await
}

/// An HTTP/1.1 request whose head is left unfinished, so its connection counts as busy and survives the
/// graceful close of a shutdown until [`HalfRequest::finish`] completes it.
pub struct HalfRequest {
    stream: TcpStream,
}

impl HalfRequest {
    pub async fn open(addr: SocketAddr, path: &str) -> HalfRequest {
        let mut stream = TcpStream::connect(addr).await.expect("tcp connect");
        let head = format!("GET {path} HTTP/1.1\r\nhost: sunnycloud.test\r\n");
        stream.write_all(head.as_bytes()).await.expect("write head");
        HalfRequest { stream }
    }

    /// Completes the request and returns the status and body of the answer.
    pub async fn finish(mut self) -> (StatusCode, String) {
        self.stream.write_all(b"\r\n").await.expect("finish head");
        let mut answer = Vec::new();
        let (head_end, length) = loop {
            self.read_more(&mut answer).await;
            let text = String::from_utf8_lossy(&answer).into_owned();
            if let Some(end) = text.find("\r\n\r\n") {
                let length = text[..end]
                    .lines()
                    .filter_map(|line| line.split_once(':'))
                    .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
                    .and_then(|(_, value)| value.trim().parse::<usize>().ok())
                    .unwrap_or(0);
                break (end + 4, length);
            }
        };
        while answer.len() < head_end + length {
            self.read_more(&mut answer).await;
        }
        let text = String::from_utf8_lossy(&answer).into_owned();
        let status = text
            .split_whitespace()
            .nth(1)
            .and_then(|code| code.parse::<u16>().ok())
            .and_then(|code| StatusCode::from_u16(code).ok())
            .expect("a status line");
        (status, text[head_end..].to_owned())
    }

    async fn read_more(&mut self, answer: &mut Vec<u8>) {
        let mut chunk = [0u8; 1024];
        let read = tokio::time::timeout(EVENT_BOUND, self.stream.read(&mut chunk))
            .await
            .expect("the answer arrives")
            .expect("read answer");
        assert!(
            read > 0,
            "the connection closed before the answer was complete"
        );
        answer.extend_from_slice(&chunk[..read]);
    }
}

/// True when a new connection to `addr` is refused, or accepted and closed at once.
pub async fn refuses_connections(addr: SocketAddr) -> bool {
    let Ok(mut stream) = TcpStream::connect(addr).await else {
        return true;
    };
    let mut byte = [0u8; 1];
    matches!(
        tokio::time::timeout(EVENT_BOUND, stream.read(&mut byte)).await,
        Ok(Ok(0) | Err(_))
    )
}
