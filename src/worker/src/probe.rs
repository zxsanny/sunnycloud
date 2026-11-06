use std::io;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::Duration;

use thiserror::Error;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::timeout;

/// Longest the whole probe may take; Compose's own timeout is 5 s.
pub const PROBE_TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Debug, Error)]
pub enum ProbeError {
    #[error("cannot reach {addr}: {source}")]
    Io { addr: SocketAddr, source: io::Error },
    #[error("no answer from {0} within {1:?}")]
    Timeout(SocketAddr, Duration),
    #[error("{path} answered `{status}`")]
    Status { path: String, status: String },
}

/// The address a local probe connects to: a wildcard listen address is reached through loopback.
pub fn probe_target(listen: SocketAddr) -> SocketAddr {
    if listen.ip().is_unspecified() {
        let loopback = match listen.ip() {
            IpAddr::V4(_) => IpAddr::V4(Ipv4Addr::LOCALHOST),
            IpAddr::V6(_) => IpAddr::V6(std::net::Ipv6Addr::LOCALHOST),
        };
        SocketAddr::new(loopback, listen.port())
    } else {
        listen
    }
}

/// One `GET path` over HTTP/1.1; `Ok` only for status 200. There is no HTTP client in the image, so the
/// status line is read by hand.
pub async fn get_ok(addr: SocketAddr, path: &str) -> Result<(), ProbeError> {
    timeout(PROBE_TIMEOUT, request(addr, path))
        .await
        .map_err(|_| ProbeError::Timeout(addr, PROBE_TIMEOUT))?
}

async fn request(addr: SocketAddr, path: &str) -> Result<(), ProbeError> {
    let io_error = |source| ProbeError::Io { addr, source };
    let mut stream = TcpStream::connect(addr).await.map_err(io_error)?;
    let request = format!("GET {path} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n\r\n");
    stream
        .write_all(request.as_bytes())
        .await
        .map_err(io_error)?;
    // "HTTP/1.1 200": the version, a space and the three status digits.
    let mut head = [0u8; 12];
    stream.read_exact(&mut head).await.map_err(io_error)?;
    if &head[9..] == b"200" && head.starts_with(b"HTTP/1.") {
        Ok(())
    } else {
        Err(ProbeError::Status {
            path: path.to_owned(),
            status: String::from_utf8_lossy(&head).into_owned(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wildcard_listen_probes_through_loopback() {
        let target = probe_target("0.0.0.0:9090".parse().unwrap());

        assert_eq!(target, "127.0.0.1:9090".parse().unwrap());
    }

    #[test]
    fn specific_listen_is_probed_as_given() {
        let listen: SocketAddr = "10.1.2.3:9090".parse().unwrap();

        assert_eq!(probe_target(listen), listen);
    }
}
