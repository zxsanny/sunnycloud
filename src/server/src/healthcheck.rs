//! `healthcheck`: the Compose probe of the ops listener's `/readyz` (the API image has no curl).

use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use sc_error::ErrorCode;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio_util::sync::CancellationToken;

use crate::error::CliError;
use crate::exit::Exit;
use crate::outcome::Outcome;

/// The ops listener as seen from inside the container (`operator_command_surface.md`: `127.0.0.1:9090/readyz`).
pub(crate) const OPS_ADDR: SocketAddr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 9090);

const READY: u16 = 200;

pub(crate) async fn run(addr: SocketAddr, cancel: &CancellationToken) -> Result<Outcome, CliError> {
    tokio::select! {
        status = readyz_status(addr) => match status {
            Ok(READY) => Ok(Outcome::success()),
            Ok(status) => Err(not_ready(format!("GET /readyz on {addr} answered {status}"))),
            Err(reason) => Err(not_ready(format!("GET /readyz on {addr} failed: {reason}"))),
        },
        () = cancel.cancelled() => Err(not_ready(format!("probe of {addr} interrupted"))),
    }
}

fn not_ready(detail: String) -> CliError {
    CliError::Operator {
        code: ErrorCode::ServiceUnavailable.as_str(),
        detail,
        exit: Exit::Failed,
    }
}

/// The status code of one `GET /readyz`; the body is not read.
async fn readyz_status(addr: SocketAddr) -> Result<u16, String> {
    let mut stream = TcpStream::connect(addr)
        .await
        .map_err(|error| format!("cannot connect: {error}"))?;
    let request = format!("GET /readyz HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n\r\n");
    stream
        .write_all(request.as_bytes())
        .await
        .map_err(|error| format!("cannot send the request: {error}"))?;
    let mut status_line = String::new();
    BufReader::new(stream)
        .read_line(&mut status_line)
        .await
        .map_err(|error| format!("cannot read the answer: {error}"))?;
    parse_status(&status_line)
}

fn parse_status(line: &str) -> Result<u16, String> {
    let mut words = line.split_whitespace();
    match (words.next(), words.next().map(str::parse::<u16>)) {
        (Some(version), Some(Ok(status))) if version.starts_with("HTTP/") => Ok(status),
        _ => Err(format!("`{}` is not an HTTP status line", line.trim_end())),
    }
}

#[cfg(test)]
mod tests {
    use std::net::SocketAddr;

    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;
    use tokio_util::sync::CancellationToken;

    use super::{parse_status, run};
    use crate::error::CliError;
    use crate::exit::Exit;

    /// A listener that answers every connection with `response` after reading the request head.
    async fn serve(response: &'static str) -> SocketAddr {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("address");
        tokio::spawn(async move {
            while let Ok((mut stream, _)) = listener.accept().await {
                let mut head = [0u8; 512];
                if stream.read(&mut head).await.is_ok() {
                    drop(stream.write_all(response.as_bytes()).await);
                }
            }
        });
        addr
    }

    /// An address nothing listens on.
    async fn closed() -> SocketAddr {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        listener.local_addr().expect("address")
    }

    fn refusal(result: Result<crate::outcome::Outcome, CliError>) -> (String, Exit) {
        match result {
            Err(CliError::Operator { detail, exit, .. }) => (detail, exit),
            other => panic!("expected an operator refusal, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn ready_answer_is_exit_zero() {
        // Arrange
        let addr = serve("HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await;

        // Act
        let outcome = run(addr, &CancellationToken::new()).await;

        // Assert
        assert_eq!(outcome.expect("ready").exit(), Exit::Success);
    }

    #[tokio::test]
    async fn not_ready_answer_is_non_zero() {
        // Arrange
        let addr = serve("HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\n\r\n").await;

        // Act
        let result = run(addr, &CancellationToken::new()).await;

        // Assert
        let (detail, exit) = refusal(result);
        assert_eq!(exit, Exit::Failed);
        assert!(detail.contains("503"), "{detail}");
    }

    #[tokio::test]
    async fn any_other_answer_is_non_zero() {
        // Arrange
        let addr = serve("HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n").await;

        // Act
        let result = run(addr, &CancellationToken::new()).await;

        // Assert
        assert_eq!(refusal(result).1, Exit::Failed);
    }

    #[tokio::test]
    async fn nothing_listening_is_non_zero() {
        // Arrange
        let addr = closed().await;

        // Act
        let result = run(addr, &CancellationToken::new()).await;

        // Assert
        let (detail, exit) = refusal(result);
        assert_eq!(exit, Exit::Failed);
        assert!(detail.contains("cannot connect"), "{detail}");
    }

    #[tokio::test]
    async fn a_peer_that_never_answers_ends_on_sigint() {
        // Arrange: accepts, reads, and keeps the connection open without a byte back.
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("address");
        let cancel = CancellationToken::new();
        let probe = tokio::spawn({
            let cancel = cancel.clone();
            async move { run(addr, &cancel).await.map(|_| ()) }
        });
        let (_held, _) = listener.accept().await.expect("the probe connects");

        // Act
        cancel.cancel();
        let result = probe.await.expect("probe task");

        // Assert
        let (detail, exit) = refusal(result.map(|()| crate::outcome::Outcome::success()));
        assert_eq!(exit, Exit::Failed);
        assert!(detail.contains("interrupted"), "{detail}");
    }

    #[test]
    fn a_status_line_gives_its_code() {
        // Act
        let status = parse_status("HTTP/1.1 503 Service Unavailable\r\n");

        // Assert
        assert_eq!(status, Ok(503));
    }

    #[test]
    fn anything_else_is_refused() {
        for line in ["", "garbage\r\n", "HTTP/1.1 abc\r\n", "HTTP/1.1\r\n"] {
            // Act
            let status = parse_status(line);

            // Assert
            assert!(status.is_err(), "{line:?}");
        }
    }
}
