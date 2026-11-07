use std::net::SocketAddr;

use http_body_util::{BodyExt, Empty};
use hyper::body::Bytes;
use hyper::client::conn::{http1, http2};
use hyper::{HeaderMap, Request, StatusCode};
use hyper_util::rt::{TokioExecutor, TokioIo};
use tokio::net::{TcpSocket, TcpStream};

/// What a request got back.
pub struct Reply {
    pub status: StatusCode,
    pub headers: HeaderMap,
    pub body: Vec<u8>,
}

impl Reply {
    pub fn header(&self, name: &str) -> &str {
        self.headers
            .get(name)
            .unwrap_or_else(|| panic!("response has {name}"))
            .to_str()
            .expect("header is text")
    }

    pub fn request_id(&self) -> &str {
        self.header("x-request-id")
    }

    pub fn text(&self) -> String {
        String::from_utf8(self.body.clone()).expect("body is text")
    }

    pub fn json(&self) -> serde_json::Value {
        serde_json::from_slice(&self.body).expect("body is JSON")
    }
}

fn request(method: &str, path: &str, headers: &[(&str, &str)]) -> Request<Empty<Bytes>> {
    let mut builder = Request::builder()
        .method(method)
        .uri(path)
        .header("host", "sunnycloud.test");
    for (name, value) in headers {
        builder = builder.header(*name, *value);
    }
    builder.body(Empty::new()).expect("request")
}

async fn read(response: hyper::Response<hyper::body::Incoming>) -> Reply {
    let (parts, body) = response.into_parts();
    let body = body.collect().await.expect("body").to_bytes().to_vec();
    Reply {
        status: parts.status,
        headers: parts.headers,
        body,
    }
}

/// One h2c connection (prior knowledge) that carries many requests, as a real client does.
#[derive(Clone)]
pub struct H2 {
    sender: http2::SendRequest<Empty<Bytes>>,
}

impl H2 {
    pub async fn connect(addr: SocketAddr) -> H2 {
        let stream = TcpStream::connect(addr).await.expect("tcp connect");
        let (sender, connection) = http2::handshake(TokioExecutor::new(), TokioIo::new(stream))
            .await
            .expect("h2 handshake");
        tokio::spawn(connection);
        H2 { sender }
    }

    pub async fn send(&mut self, method: &str, path: &str, headers: &[(&str, &str)]) -> Reply {
        let response = self
            .sender
            .send_request(request(method, path, headers))
            .await
            .expect("h2 response on the same connection");
        read(response).await
    }

    pub async fn get(&mut self, path: &str) -> Reply {
        self.send("GET", path, &[]).await
    }
}

/// One request over a fresh HTTP/1.1 connection whose source address is `source`.
pub async fn h1_from(
    source: &str,
    addr: SocketAddr,
    path: &str,
    headers: &[(&str, &str)],
) -> Reply {
    let socket = TcpSocket::new_v4().expect("socket");
    socket
        .bind((source.parse::<std::net::IpAddr>().expect("source ip"), 0).into())
        .expect("bind source address");
    let stream = socket.connect(addr).await.expect("tcp connect");
    let (mut sender, connection) = http1::handshake(TokioIo::new(stream))
        .await
        .expect("h1 handshake");
    tokio::spawn(connection);
    let response = sender
        .send_request(request("GET", path, headers))
        .await
        .expect("h1 response");
    read(response).await
}
