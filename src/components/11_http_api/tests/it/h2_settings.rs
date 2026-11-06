use std::net::SocketAddr;
use std::time::Duration;

use hyper::StatusCode;
use sunnycloud_http_api::HttpConfig;

use crate::harness::raw_h2::{
    DEFAULT_WINDOW, Frame, RawH2, Read, frame_type, setting, settings, window_increment,
};
use crate::harness::{Server, clients, default_config};

const WIRE_WAIT: Duration = Duration::from_secs(5);

struct Advertised {
    initial_stream_window: u32,
    max_streams: u32,
    max_header_list: u32,
    connection_window: u32,
}

/// Reads frames until the server's SETTINGS and its first connection-level WINDOW_UPDATE have arrived.
async fn read_advertised(addr: SocketAddr) -> Advertised {
    let mut raw = RawH2::connect(addr).await;
    let mut server_settings: Option<Frame> = None;
    let mut increment: Option<u32> = None;
    while server_settings.is_none() || increment.is_none() {
        match raw.read(WIRE_WAIT).await {
            Read::Frame(frame) if frame.kind == frame_type::SETTINGS && frame.flags == 0 => {
                server_settings = Some(frame);
            }
            Read::Frame(frame) if frame.kind == frame_type::WINDOW_UPDATE && frame.stream == 0 => {
                increment.get_or_insert(window_increment(&frame));
            }
            Read::Frame(_) => {}
            other => panic!("expected SETTINGS and WINDOW_UPDATE, got {other:?}"),
        }
    }
    let value = |id| {
        settings(server_settings.as_ref().expect("settings"))
            .into_iter()
            .find_map(|(key, value)| (key == id).then_some(value))
            .unwrap_or_else(|| panic!("setting {id:#x} not advertised"))
    };
    Advertised {
        initial_stream_window: value(setting::INITIAL_WINDOW_SIZE),
        max_streams: value(setting::MAX_CONCURRENT_STREAMS),
        max_header_list: value(setting::MAX_HEADER_LIST_SIZE),
        connection_window: DEFAULT_WINDOW + increment.expect("window update"),
    }
}

#[tokio::test]
async fn default_configuration_is_advertised_on_the_wire_and_http1_shares_the_port() {
    // Arrange
    let server = Server::start(default_config()).await;

    // Act
    let advertised = read_advertised(server.public).await;
    let (h1_status, _) = clients::h1_get(server.public, "/").await;
    server.stop().await.expect("run returns");

    // Assert
    assert_eq!(advertised.initial_stream_window, 4_194_304);
    assert_eq!(advertised.connection_window, 33_554_432);
    assert_eq!(advertised.max_streams, 100);
    assert_eq!(advertised.max_header_list, 16_384);
    assert_eq!(h1_status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn overridden_configuration_is_advertised_on_the_wire_on_both_listeners() {
    // Arrange
    let config = HttpConfig {
        h2_stream_window: 1 << 20,
        h2_conn_window: 2 << 20,
        h2_max_streams: 7,
        h2_max_header_list: 8 << 10,
        ..default_config()
    };
    let server = Server::start(config).await;

    // Act
    let public = read_advertised(server.public).await;
    let internal = read_advertised(server.internal).await;
    server.stop().await.expect("run returns");

    // Assert
    for advertised in [public, internal] {
        assert_eq!(advertised.initial_stream_window, 1_048_576);
        assert_eq!(advertised.connection_window, 2_097_152);
        assert_eq!(advertised.max_streams, 7);
        assert_eq!(advertised.max_header_list, 8_192);
    }
}
