//! One h2c connection (HTTP/2 with prior knowledge) to the public listener; requests run as parallel streams on it,
//! as the desktop and mobile uploaders send them.

use std::net::SocketAddr;

use http_body_util::{BodyExt, Full};
use hyper::Request;
use hyper::body::Bytes;
use hyper::client::conn::http2::{self, SendRequest};
use hyper_util::rt::{TokioExecutor, TokioIo};
use tokio::net::TcpStream;

use crate::serve_support::http::Response;

const HOST: &str = "cloud.example.org";

#[derive(Clone)]
pub struct H2 {
    sender: SendRequest<Full<Bytes>>,
}

impl H2 {
    pub async fn connect(addr: SocketAddr) -> H2 {
        let stream = TcpStream::connect(addr).await.expect("tcp connect");
        let (sender, connection) = http2::handshake(TokioExecutor::new(), TokioIo::new(stream))
            .await
            .expect("h2c handshake");
        tokio::spawn(async move {
            if let Err(error) = connection.await {
                eprintln!("h2c connection to {addr} ended: {error}");
            }
        });
        H2 { sender }
    }

    /// `method path` with a bearer token, extra headers and a body on a new stream; panics on a transport failure.
    pub async fn call(
        &self,
        method: &str,
        path: &str,
        token: &str,
        headers: &[(&str, String)],
        body: Bytes,
    ) -> Response {
        let mut builder = Request::builder()
            .method(method)
            .uri(path)
            .header("host", HOST)
            .header("authorization", format!("Bearer {token}"))
            .header("content-length", body.len());
        for (name, value) in headers {
            builder = builder.header(*name, value);
        }
        let request = builder.body(Full::new(body)).expect("a request");
        let mut sender = self.sender.clone();
        sender.ready().await.expect("the h2c connection is open");
        let response = sender
            .send_request(request)
            .await
            .unwrap_or_else(|error| panic!("{method} {path} over h2c: {error}"));
        assert_eq!(response.version(), hyper::Version::HTTP_2);
        let status = response.status().as_u16();
        let body = response
            .into_body()
            .collect()
            .await
            .unwrap_or_else(|error| panic!("{method} {path} body over h2c: {error}"))
            .to_bytes()
            .to_vec();
        Response { status, body }
    }
}
