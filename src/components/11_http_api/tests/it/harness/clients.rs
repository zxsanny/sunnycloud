use std::net::SocketAddr;

use http_body_util::{BodyExt, Empty};
use hyper::body::Bytes;
use hyper::client::conn::{http1, http2};
use hyper::{Request, StatusCode};
use hyper_util::rt::{TokioExecutor, TokioIo};
use tokio::net::TcpStream;

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
