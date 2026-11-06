//! The explicit windows and the idle keep-alive of AZ-3959 (13 §3 transport), read off the wire: a raw HTTP/2 peer
//! records the client's SETTINGS, connection WINDOW_UPDATE and PING frames.

mod connect_support;
mod request_support;
mod support;

use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use connect_support::{client, config, valid_info};
use support::{TlsMode, generate};
use sync_core::HttpSyncClient;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio_rustls::TlsAcceptor;

const PREFACE_LEN: usize = 24;
const INITIAL_WINDOW_SIZE: u16 = 0x4;
const DEFAULT_WINDOW: u64 = 65_535;
const HEADERS: u8 = 0x1;
const SETTINGS: u8 = 0x4;
const PING: u8 = 0x6;
const WINDOW_UPDATE: u8 = 0x8;
const END_STREAM: u8 = 0x1;
const END_HEADERS: u8 = 0x4;

#[derive(Default)]
struct Seen {
    stream_window: Option<u32>,
    connection_increment: Option<u32>,
    pings: u32,
}

fn frame(kind: u8, flags: u8, stream: u32, payload: &[u8]) -> Vec<u8> {
    let length = u32::try_from(payload.len()).expect("small frame");
    let mut out = length.to_be_bytes()[1..].to_vec();
    out.extend([kind, flags]);
    out.extend(stream.to_be_bytes());
    out.extend(payload);
    out
}

/// Answers every request with 200 and the `server-info` JSON; records what the client sends.
async fn serve(listener: TcpListener, acceptor: TlsAcceptor, seen: Arc<Mutex<Seen>>) {
    let (tcp, _) = listener.accept().await.expect("accept");
    let mut tls = acceptor.accept(tcp).await.expect("tls handshake");
    let mut preface = [0u8; PREFACE_LEN];
    tls.read_exact(&mut preface).await.expect("preface");
    tls.write_all(&frame(SETTINGS, 0, 0, &[]))
        .await
        .expect("settings");
    let body = serde_json::to_vec(&valid_info()).expect("json");
    loop {
        let mut head = [0u8; 9];
        if tls.read_exact(&mut head).await.is_err() {
            return;
        }
        let length = u32::from_be_bytes([0, head[0], head[1], head[2]]) as usize;
        let (kind, flags) = (head[3], head[4]);
        let stream = u32::from_be_bytes([head[5] & 0x7f, head[6], head[7], head[8]]);
        let mut payload = vec![0u8; length];
        tls.read_exact(&mut payload).await.expect("payload");
        match kind {
            SETTINGS if flags & 1 == 0 => {
                for pair in payload.chunks_exact(6) {
                    let id = u16::from_be_bytes([pair[0], pair[1]]);
                    if id == INITIAL_WINDOW_SIZE {
                        let value = u32::from_be_bytes([pair[2], pair[3], pair[4], pair[5]]);
                        seen.lock()
                            .unwrap_or_else(PoisonError::into_inner)
                            .stream_window = Some(value);
                    }
                }
                tls.write_all(&frame(SETTINGS, 1, 0, &[]))
                    .await
                    .expect("ack");
            }
            WINDOW_UPDATE if stream == 0 => {
                let value =
                    u32::from_be_bytes([payload[0] & 0x7f, payload[1], payload[2], payload[3]]);
                seen.lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .connection_increment = Some(value);
            }
            PING if flags & 1 == 0 => {
                seen.lock().unwrap_or_else(PoisonError::into_inner).pings += 1;
                tls.write_all(&frame(PING, 1, 0, &payload))
                    .await
                    .expect("pong");
            }
            HEADERS => {
                // 0x88 is the HPACK index of `:status: 200`.
                tls.write_all(&frame(HEADERS, END_HEADERS, stream, &[0x88]))
                    .await
                    .expect("headers");
                tls.write_all(&frame(0x0, END_STREAM, stream, &body))
                    .await
                    .expect("data");
            }
            _ => {}
        }
    }
}

#[tokio::test]
async fn windows_are_the_configured_ones_and_idle_pings_keep_the_connection_alive() {
    let pki = generate(TlsMode::Tls13H2);
    let listener = TcpListener::bind(("127.0.0.1", 0)).await.expect("listener");
    let port = listener.local_addr().expect("address").port();
    let seen = Arc::new(Mutex::new(Seen::default()));
    let server = tokio::spawn(serve(
        listener,
        TlsAcceptor::from(Arc::clone(&pki.server_config)),
        Arc::clone(&seen),
    ));
    let mut settings = config(&format!("https://127.0.0.1:{port}"));
    settings.h2_stream_window = 3 * 1024 * 1024;
    settings.h2_conn_window = 5 * 1024 * 1024;
    settings.keepalive_interval = Duration::from_millis(150);

    let connected = HttpSyncClient::connect_with_roots(
        settings,
        request_support::SentinelCredentials::new(),
        vec![pki.ca.clone()],
    )
    .await;

    let (_client, _) = client(connected);
    let ping_seen = async {
        loop {
            if seen.lock().unwrap_or_else(PoisonError::into_inner).pings >= 2 {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    };
    tokio::time::timeout(Duration::from_secs(5), ping_seen)
        .await
        .expect("the idle connection was pinged at least twice");
    let seen = seen.lock().unwrap_or_else(PoisonError::into_inner);
    assert_eq!(seen.stream_window, Some(3 * 1024 * 1024));
    assert_eq!(
        seen.connection_increment.map(u64::from),
        Some(5 * 1024 * 1024 - DEFAULT_WINDOW)
    );
    server.abort();
}
