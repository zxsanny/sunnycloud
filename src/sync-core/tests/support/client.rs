//! A minimal HTTPS client for the fixture's own tests: one connection, HTTP/2 or HTTP/1.1 by ALPN.

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::Request;
use hyper::client::conn::{http1, http2};
use hyper_util::rt::{TokioExecutor, TokioIo};
use rustls::ClientConfig;
use rustls::ProtocolVersion;
use rustls::pki_types::ServerName;
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;

#[derive(Debug)]
pub struct Reply {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Bytes,
}

impl Reply {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }

    pub fn header_pairs(&self) -> Vec<(&str, &str)> {
        self.headers
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect()
    }
}

/// A request that ended without a complete response; `received` is the body bytes read before the failure.
#[derive(Debug)]
pub struct Failure {
    pub status: Option<u16>,
    pub received: usize,
    pub error: String,
}

enum Sender {
    H2(http2::SendRequest<Full<Bytes>>),
    H1(http1::SendRequest<Full<Bytes>>),
}

pub struct Client {
    sender: Sender,
    pub alpn: Option<String>,
    pub tls_version: Option<ProtocolVersion>,
    connection_error: Arc<Mutex<Option<String>>>,
}

impl Client {
    /// Connects to `addr`, naming `server_name` in the handshake. A handshake error is returned as text.
    pub async fn connect(
        addr: SocketAddr,
        server_name: &str,
        config: Arc<ClientConfig>,
    ) -> Result<Client, String> {
        let tcp = TcpStream::connect(addr)
            .await
            .map_err(|error| format!("tcp: {error}"))?;
        let name = ServerName::try_from(server_name.to_owned())
            .map_err(|error| format!("server name: {error}"))?;
        let tls = TlsConnector::from(config)
            .connect(name, tcp)
            .await
            .map_err(|error| format!("tls: {error}"))?;
        let (_, session) = tls.get_ref();
        let alpn = session
            .alpn_protocol()
            .map(|p| String::from_utf8_lossy(p).into_owned());
        let tls_version = session.protocol_version();
        let io = TokioIo::new(tls);
        let connection_error = Arc::new(Mutex::new(None));
        let sender = if alpn.as_deref() == Some("h2") {
            let (sender, connection) = http2::handshake(TokioExecutor::new(), io)
                .await
                .map_err(|error| format!("h2 handshake: {error}"))?;
            watch(connection_error.clone(), connection);
            Sender::H2(sender)
        } else {
            let (sender, connection) = http1::handshake(io)
                .await
                .map_err(|error| format!("h1 handshake: {error}"))?;
            watch(connection_error.clone(), connection);
            Sender::H1(sender)
        };
        Ok(Client {
            sender,
            alpn,
            tls_version,
            connection_error,
        })
    }

    pub fn connection_error(&self) -> Option<String> {
        self.connection_error
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    pub async fn get(&mut self, target: &str) -> Result<Reply, Failure> {
        self.send("GET", target, &[], Bytes::new()).await
    }

    pub async fn send(
        &mut self,
        method: &str,
        target: &str,
        headers: &[(&str, &str)],
        body: Bytes,
    ) -> Result<Reply, Failure> {
        let mut builder = Request::builder()
            .method(method)
            .uri(format!("https://fixture.test{target}"));
        for (name, value) in headers {
            builder = builder.header(*name, *value);
        }
        let request = builder.body(Full::new(body)).map_err(|error| Failure {
            status: None,
            received: 0,
            error: error.to_string(),
        })?;
        let sent = match &mut self.sender {
            Sender::H2(sender) => sender.send_request(request).await,
            Sender::H1(sender) => {
                let (parts, body) = request.into_parts();
                let mut parts = parts;
                parts.uri = target
                    .parse()
                    .map_err(|error: hyper::http::uri::InvalidUri| Failure {
                        status: None,
                        received: 0,
                        error: error.to_string(),
                    })?;
                parts.headers.insert(
                    "host",
                    hyper::header::HeaderValue::from_static("fixture.test"),
                );
                sender.send_request(Request::from_parts(parts, body)).await
            }
        };
        let response = sent.map_err(|error| Failure {
            status: None,
            received: 0,
            error: error.to_string(),
        })?;
        let status = response.status().as_u16();
        let headers = response
            .headers()
            .iter()
            .map(|(k, v)| {
                (
                    k.as_str().to_owned(),
                    String::from_utf8_lossy(v.as_bytes()).into_owned(),
                )
            })
            .collect();
        let mut body = response.into_body();
        let mut data = Vec::new();
        while let Some(frame) = body.frame().await {
            match frame {
                Ok(frame) => {
                    if let Ok(chunk) = frame.into_data() {
                        data.extend_from_slice(&chunk);
                    }
                }
                Err(error) => {
                    return Err(Failure {
                        status: Some(status),
                        received: data.len(),
                        error: error.to_string(),
                    });
                }
            }
        }
        Ok(Reply {
            status,
            headers,
            body: Bytes::from(data),
        })
    }
}

fn watch<C>(slot: Arc<Mutex<Option<String>>>, connection: C)
where
    C: std::future::Future<Output = Result<(), hyper::Error>> + Send + 'static,
{
    tokio::spawn(async move {
        if let Err(error) = connection.await {
            *slot
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(error.to_string());
        }
    });
}
