//! A scripted change feed, the server around it, and a WebSocket client.

use std::net::SocketAddr;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use axum::http::HeaderValue;
use contracts::{
    ChangeFeed, ChangeFeedError, ChangeItem, ChangePage, Cursor, NodeKind, NodeState, Principal,
    PrincipalRevalidator,
};
use futures_util::{SinkExt, StreamExt};
use sunnycloud_http_api::registry::{Routers, document, mount, production_routes};
use sunnycloud_http_api::{ApiServices, RealtimeLimits};
use tokio::net::TcpStream;
use tokio::sync::{Mutex as AsyncMutex, mpsc};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::protocol::Message;
use tokio_tungstenite::{WebSocketStream, client_async};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::auth::support::{MEMBER_TOKEN, ScriptedAuthenticator};
use crate::harness::router_server::{RouterServer, route_context_with};
use crate::harness::{EVENT_BOUND, SESSION_COOKIE, Script, new_metrics, stub_services};

pub const ORIGIN: &str = "https://sunnycloud.test";

/// What the test pushes into a held read.
pub type Answer = Result<ChangePage, ChangeFeedError>;

/// A feed whose `read` parks until the test hands it an answer, as a held read does, and remembers the cursors
/// it was asked from.
pub struct ScriptedFeed {
    answers: AsyncMutex<mpsc::UnboundedReceiver<Answer>>,
    cursors: Mutex<Vec<Cursor>>,
    head: Mutex<Cursor>,
}

/// The test's end of a [`ScriptedFeed`].
pub struct FeedHandle {
    pub answers: mpsc::UnboundedSender<Answer>,
    feed: Arc<ScriptedFeed>,
}

impl FeedHandle {
    pub fn answer(&self, answer: Answer) {
        self.answers.send(answer).expect("the feed is alive");
    }

    pub fn set_head(&self, head: Cursor) {
        *self
            .feed
            .head
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = head;
    }

    /// Waits until `read` was called `count` times and returns every cursor it was called with.
    pub async fn cursors(&self, count: usize) -> Vec<Cursor> {
        let waited = async {
            loop {
                let seen = self
                    .feed
                    .cursors
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .clone();
                if seen.len() >= count {
                    return seen;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        };
        tokio::time::timeout(EVENT_BOUND, waited)
            .await
            .expect("the feed was read as often as expected")
    }
}

#[contracts::async_trait]
impl ChangeFeed for ScriptedFeed {
    async fn read(
        &self,
        _principal: &Principal,
        _revalidator: &dyn PrincipalRevalidator,
        cursor: Cursor,
        _wait: Option<Duration>,
        _limit: Option<u32>,
    ) -> Answer {
        self.cursors
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(cursor);
        let mut answers = self.answers.lock().await;
        answers.recv().await.unwrap_or_else(|| {
            Ok(ChangePage {
                items: Vec::new(),
                next_cursor: cursor,
                has_more: false,
            })
        })
    }

    async fn head(&self, _account: Uuid) -> Result<Cursor, ChangeFeedError> {
        Ok(*self.head.lock().unwrap_or_else(PoisonError::into_inner))
    }
}

/// The public router of the production registry over the scripted feed and authenticator.
pub struct Realtime {
    pub script: Script,
    pub server: RouterServer,
    pub feed: FeedHandle,
    pub shutdown: CancellationToken,
}

impl Realtime {
    pub async fn start(limits: RealtimeLimits) -> Realtime {
        let (sender, receiver) = mpsc::unbounded_channel();
        let feed = Arc::new(ScriptedFeed {
            answers: AsyncMutex::new(receiver),
            cursors: Mutex::default(),
            head: Mutex::new(Cursor { epoch: 1, seq: 0 }),
        });
        let shutdown = CancellationToken::new();
        let script = Script::default();
        let services = ApiServices {
            authenticator: Arc::new(ScriptedAuthenticator::default()),
            change_feed: Arc::clone(&feed) as Arc<dyn ChangeFeed>,
            feed_shutdown: shutdown.clone(),
            ..stub_services(&script, new_metrics())
        };
        let defs = production_routes();
        let context = sunnycloud_http_api::registry::RouteContext {
            services: Arc::new(services),
            realtime: limits,
            ..route_context_with(document(&defs), &script)
        };
        let Routers { public, .. } = mount(&defs, &context, None);
        Realtime {
            script,
            server: RouterServer::start(public).await,
            feed: FeedHandle {
                answers: sender,
                feed,
            },
            shutdown,
        }
    }

    pub fn addr(&self) -> SocketAddr {
        self.server.addr
    }

    /// A socket of the member's session cookie from the web app's origin.
    pub async fn connect(&self) -> Client {
        Client::open(self.addr(), &member_headers())
            .await
            .expect("the handshake succeeds")
    }

    pub async fn stop(self) {
        self.server.stop().await;
    }
}

pub fn member_headers() -> Vec<(&'static str, String)> {
    vec![
        ("Cookie", format!("{SESSION_COOKIE}={MEMBER_TOKEN}")),
        ("Origin", ORIGIN.to_owned()),
    ]
}

/// The handshake answer when it is not a 101.
#[derive(Debug)]
pub struct Refused {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Refused {
    pub fn code(&self) -> String {
        serde_json::from_slice::<serde_json::Value>(&self.body)
            .ok()
            .and_then(|body| body["code"].as_str().map(str::to_owned))
            .unwrap_or_default()
    }

    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(found, _)| found.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }
}

pub struct Client {
    pub socket: WebSocketStream<TcpStream>,
}

impl Client {
    pub async fn open(addr: SocketAddr, headers: &[(&str, String)]) -> Result<Client, Refused> {
        let mut request = "ws://sunnycloud.test/api/v1/realtime"
            .into_client_request()
            .expect("a valid request");
        for (name, value) in headers {
            request.headers_mut().insert(
                axum::http::HeaderName::from_bytes(name.as_bytes()).expect("a header name"),
                HeaderValue::from_str(value).expect("a header value"),
            );
        }
        let stream = TcpStream::connect(addr).await.expect("tcp connect");
        match client_async(request, stream).await {
            Ok((socket, _)) => Ok(Client { socket }),
            Err(tokio_tungstenite::tungstenite::Error::Http(response)) => Err(Refused {
                status: response.status().as_u16(),
                headers: response
                    .headers()
                    .iter()
                    .map(|(name, value)| {
                        (
                            name.as_str().to_owned(),
                            value.to_str().unwrap_or_default().to_owned(),
                        )
                    })
                    .collect(),
                body: response.into_body().unwrap_or_default(),
            }),
            Err(error) => panic!("the handshake failed without an answer: {error}"),
        }
    }

    pub async fn send(&mut self, frame: &[u8]) {
        self.socket
            .send(Message::Binary(frame.to_vec().into()))
            .await
            .expect("the frame is sent");
    }

    pub async fn send_message(&mut self, message: Message) {
        self.socket
            .send(message)
            .await
            .expect("the message is sent");
    }

    /// The next message, or `None` when the connection ended (a failed read counts as ended).
    pub async fn next(&mut self) -> Option<Message> {
        let waited = tokio::time::timeout(EVENT_BOUND, self.socket.next())
            .await
            .expect("a message or the end arrives in time");
        waited.and_then(Result::ok)
    }

    /// The next binary frame; control frames are skipped.
    pub async fn frame(&mut self) -> Vec<u8> {
        loop {
            match self.next().await {
                Some(Message::Binary(bytes)) => return bytes.to_vec(),
                Some(Message::Ping(_) | Message::Pong(_)) => {}
                other => panic!("expected a binary frame, got {other:?}"),
            }
        }
    }

    /// The code of the Close frame that ends the connection; `None` when it ended without one.
    pub async fn close_code(&mut self) -> Option<u16> {
        loop {
            match self.next().await {
                Some(Message::Close(frame)) => return frame.map(|frame| u16::from(frame.code)),
                Some(_) => {}
                None => return None,
            }
        }
    }
}

pub fn subscribe(epoch: u64, seq: u64) -> Vec<u8> {
    let mut frame = vec![0x01];
    frame.extend(epoch.to_le_bytes());
    frame.extend(seq.to_le_bytes());
    frame
}

pub fn ping(nonce: u32) -> Vec<u8> {
    let mut frame = vec![0x02];
    frame.extend(nonce.to_le_bytes());
    frame
}

/// `Hello` of the contract's defaults: version 1, ping 30 s, client frames up to 1,024 bytes.
pub const HELLO: [u8; 9] = [0x20, 1, 0, 30, 0, 0, 4, 0, 0];

pub fn item(seq: i64, id: u128, name: &str) -> ChangeItem {
    ChangeItem {
        node_id: Uuid::from_u128(id),
        parent_id: None,
        kind: NodeKind::Folder,
        name: name.to_owned(),
        state: NodeState::Live,
        revision_id: None,
        size: None,
        content_hash: None,
        seq,
    }
}

pub fn page(items: Vec<ChangeItem>, epoch: i64) -> ChangePage {
    let seq = items.last().map_or(0, |item| item.seq);
    ChangePage {
        items,
        next_cursor: Cursor { epoch, seq },
        has_more: false,
    }
}

/// The bytes of a `Change` of a folder named `a` (compact text `02 10 61`), with `present` flags and their payload.
pub fn change_frame(epoch: u64, seq: u64, id: u128, present: &[u8]) -> Vec<u8> {
    let mut frame = vec![0x21];
    frame.extend(epoch.to_le_bytes());
    frame.extend(seq.to_le_bytes());
    frame.extend(Uuid::from_u128(id).as_bytes());
    frame.extend([1, 0, 0x02, 0x10, b'a']);
    frame.extend(present);
    frame
}
