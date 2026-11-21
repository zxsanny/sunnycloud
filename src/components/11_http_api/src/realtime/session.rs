//! One open socket: `Hello`, then a single task that reads the client, keeps the connection alive and pushes the
//! feed (`realtime.md`, Connection lifecycle). No per-connection queue: the task sends inline, so a client that
//! stops reading stops its own task, and each send has a timeout.

use std::collections::{HashMap, HashSet};
use std::future::{Future, pending};
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use axum::body::Bytes;
use axum::extract::ws::{CloseFrame, Message, WebSocket};
use contracts::{
    ChangeFeedError, ChangeItem, ChangePage, Cursor, NodeKind, NodeState, Principal, ThumbEvent,
    ThumbNotifier, ThumbState, ThumbSubscription, ThumbWake,
};
use sc_error::CorrelationId;
use sc_ids::FileHash;
use sc_observability::{log_debug, log_error, log_warn};
use tokio::time::{Instant, interval_at, sleep_until, timeout};

use super::RealtimeLimits;
use super::cap::ConnectionPermit;
use super::frames::{
    ChangeFrame, ClientFrame, FrameError, ResetReason, decode_client, encode_change, encode_hello,
    encode_pong, encode_reset, encode_thumb_state,
};
use crate::services::ApiServices;

/// The longest a held read waits; the feed clamps it to `longpoll_wait_max`.
const READ_WAIT: Duration = Duration::from_secs(55);
/// The thumbnail port is asked for at most this many hashes at once (the cap of `POST /thumbnail-states`).
const STATE_BATCH: usize = 1_000;
/// How long each step of the close handshake may take before the socket is dropped.
const CLOSE_WAIT: Duration = Duration::from_secs(1);

pub(super) struct Connection {
    pub services: Arc<ApiServices>,
    pub principal: Principal,
    pub request_id: CorrelationId,
    pub limits: RealtimeLimits,
    /// The preview events of the process; `None` where nothing announces them (no `ThumbState` is pushed).
    pub thumbs: Option<Arc<dyn ThumbNotifier>>,
    /// Held for as long as the socket is open.
    pub _permit: ConnectionPermit,
}

/// How a connection ends.
#[derive(Debug, PartialEq, Eq)]
enum End {
    /// The server closes with this code; the reason is a fixed word.
    Close(u16, &'static str),
    /// A send did not finish in time: the client stopped reading.
    Slow,
    /// The peer closed, or the socket failed.
    Gone,
}

type Read = Pin<Box<dyn Future<Output = Result<ChangePage, ChangeFeedError>> + Send>>;

/// What the loop knows of the client's resume point: the held read of the next page, once subscribed.
#[derive(Default)]
struct Subscription {
    read: Option<Read>,
}

pub(super) async fn run(mut socket: WebSocket, connection: Connection) {
    let end = drive(&mut socket, &connection).await;
    let account = connection.principal.account_id;
    match end {
        End::Close(code, reason) => send_close(&mut socket, code, reason).await,
        End::Slow => send_close(&mut socket, 4429, "too slow").await,
        End::Gone => {}
    }
    log_debug!("realtime socket closed", account_id = account);
}

/// The close handshake: the Close frame, then reading until the peer's own Close (or the end) so the socket is
/// not reset under frames still in flight; both steps are bounded by [`CLOSE_WAIT`].
async fn send_close(socket: &mut WebSocket, code: u16, reason: &'static str) {
    let frame = CloseFrame {
        code,
        reason: reason.into(),
    };
    match timeout(CLOSE_WAIT, socket.send(Message::Close(Some(frame)))).await {
        Ok(Ok(())) => {}
        Ok(Err(error)) => return log_debug!("realtime close frame not sent", err = &error),
        Err(_) => return log_debug!("realtime close frame timed out"),
    }
    let drained = timeout(CLOSE_WAIT, async {
        while let Some(Ok(message)) = socket.recv().await {
            if matches!(message, Message::Close(_)) {
                break;
            }
        }
    })
    .await;
    if drained.is_err() {
        log_debug!("realtime peer did not answer the close frame");
    }
}

async fn drive(socket: &mut WebSocket, connection: &Connection) -> End {
    let limits = &connection.limits;
    // Registered before `Hello`: a preview that lands after the client sees `Hello` is pushed, never missed.
    let mut thumbs = connection
        .thumbs
        .as_ref()
        .map(|notifier| notifier.subscribe());
    let hello = match encode_hello(limits.ping_interval_secs(), limits.max_client_frame_bytes()) {
        Ok(hello) => hello,
        Err(error) => return internal("hello did not pack", &error),
    };
    if let Err(end) = send_binary(socket, hello, limits.send_timeout).await {
        return end;
    }
    let started = Instant::now();
    let mut last_inbound = started;
    let mut keepalive = interval_at(started + limits.ping_interval, limits.ping_interval);
    let mut subscription = Subscription::default();
    let mut subscribed = false;
    loop {
        let deadline = if subscribed {
            last_inbound + limits.idle_timeout
        } else {
            started + limits.subscribe_timeout
        };
        tokio::select! {
            biased;
            () = connection.services.feed_shutdown.cancelled() => {
                return End::Close(1001, "going away");
            }
            () = sleep_until(deadline) => return End::Close(4408, "idle"),
            message = socket.recv() => {
                last_inbound = Instant::now();
                let outcome = match message {
                    Some(Ok(message)) => inbound(socket, connection, message).await,
                    Some(Err(error)) => return on_socket_error(&error),
                    None => return End::Gone,
                };
                match outcome {
                    Ok(Some(cursor)) => {
                        subscribed = true;
                        subscription.read = Some(arm(connection, cursor));
                    }
                    Ok(None) => {}
                    Err(end) => return end,
                }
            }
            _ = keepalive.tick() => {
                if let Err(end) = send_message(socket, Message::Ping(Bytes::new()), limits.send_timeout).await {
                    return end;
                }
            }
            wake = next_thumb(&mut thumbs) => {
                match thumb_wake(socket, connection, wake).await {
                    Ok(true) => subscription.read = None,
                    Ok(false) => {}
                    Err(end) => return end,
                }
            }
            page = next_page(&mut subscription.read) => {
                subscription.read = None;
                match deliver(socket, connection, page).await {
                    Ok(Some(cursor)) => subscription.read = Some(arm(connection, cursor)),
                    Ok(None) => {}
                    Err(end) => return end,
                }
            }
        }
    }
}

/// Resolves with the next preview event; never when nothing announces them.
async fn next_thumb(thumbs: &mut Option<Box<dyn ThumbSubscription>>) -> ThumbWake {
    match thumbs {
        Some(subscription) => subscription.next().await,
        None => pending().await,
    }
}

/// An event of this account becomes a `ThumbState` frame (other accounts' events are not this socket's). A lag
/// is a `Reset` (`true`: the subscription is over until the client subscribes again).
async fn thumb_wake(
    socket: &mut WebSocket,
    connection: &Connection,
    wake: ThumbWake,
) -> Result<bool, End> {
    match wake {
        ThumbWake::Event(ThumbEvent {
            account,
            content_hash,
            state,
        }) if account == connection.principal.account_id => {
            let frame = encode_thumb_state(content_hash.into_bytes(), thumb_number(state))
                .map_err(|error| internal("thumb state did not pack", &error))?;
            send_binary(socket, frame, connection.limits.send_timeout).await?;
            Ok(false)
        }
        ThumbWake::Event(_) => Ok(false),
        ThumbWake::Lagged => {
            reset(socket, connection, ResetReason::Lagged).await?;
            Ok(true)
        }
    }
}

/// Resolves when the held read of the subscription does; never while there is none.
async fn next_page(read: &mut Option<Read>) -> Result<ChangePage, ChangeFeedError> {
    match read {
        Some(read) => read.await,
        None => pending().await,
    }
}

/// The held read after `cursor`: the same call, with the same cursor rules and wake, as `GET /changes`.
fn arm(connection: &Connection, cursor: Cursor) -> Read {
    let feed = Arc::clone(&connection.services.change_feed);
    let revalidator = Arc::clone(&connection.services.principal_revalidator);
    let principal = connection.principal.clone();
    Box::pin(async move {
        feed.read(
            &principal,
            revalidator.as_ref(),
            cursor,
            Some(READ_WAIT),
            None,
        )
        .await
    })
}

/// One client message. `Some(cursor)` is a `Subscribe` to start reading from.
async fn inbound(
    socket: &mut WebSocket,
    connection: &Connection,
    message: Message,
) -> Result<Option<Cursor>, End> {
    match message {
        Message::Binary(bytes) => {
            if bytes.len() > connection.limits.max_client_frame {
                return Err(End::Close(1009, "message too big"));
            }
            match decode_client(&bytes) {
                Ok(ClientFrame::Subscribe { epoch, seq }) => {
                    match (i64::try_from(epoch), i64::try_from(seq)) {
                        (Ok(epoch), Ok(seq)) => Ok(Some(Cursor { epoch, seq })),
                        _ => Err(End::Close(1002, "bad value")),
                    }
                }
                Ok(ClientFrame::Ping { nonce }) => {
                    let pong = encode_pong(nonce)
                        .map_err(|error| internal("pong did not pack", &error))?;
                    send_binary(socket, pong, connection.limits.send_timeout).await?;
                    Ok(None)
                }
                Err(error) => Err(malformed(connection, &error)),
            }
        }
        Message::Text(_) => Err(End::Close(1003, "binary frames only")),
        Message::Ping(_) | Message::Pong(_) => Ok(None),
        Message::Close(_) => Err(End::Gone),
    }
}

fn malformed(connection: &Connection, error: &FrameError) -> End {
    log_debug!(
        "realtime frame refused",
        account_id = connection.principal.account_id,
        request_id = connection.request_id,
        err = &Debugged(format!("{error:?}"))
    );
    End::Close(1002, error.reason())
}

/// tungstenite reports a message above the configured size as a read error without a Close frame, so the
/// server sends the 1009 itself; every other read error is a socket that is gone.
fn on_socket_error(error: &axum::Error) -> End {
    log_debug!("realtime socket read failed", err = error);
    if error
        .to_string()
        .starts_with("Space limit exceeded: Message too long")
    {
        End::Close(1009, "message too big")
    } else {
        End::Gone
    }
}

fn internal(what: &'static str, error: &dyn std::fmt::Debug) -> End {
    log_error!(
        "realtime internal failure",
        err = &Debugged(format!("{what}: {error:?}"))
    );
    End::Close(1011, "internal error")
}

/// A `Debug` text as a loggable error: packbin and the ports' own errors are not all `Display`.
#[derive(Debug)]
struct Debugged(String);

impl std::fmt::Display for Debugged {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Debugged {}

/// The answer of a held read: its frames, then the cursor of the next read; or a reset, or the end.
async fn deliver(
    socket: &mut WebSocket,
    connection: &Connection,
    page: Result<ChangePage, ChangeFeedError>,
) -> Result<Option<Cursor>, End> {
    let page = match page {
        Ok(page) => page,
        Err(ChangeFeedError::ResyncRequired { reason }) => {
            reset(socket, connection, reset_reason(reason)).await?;
            return Ok(None);
        }
        Err(ChangeFeedError::Unauthenticated) => return Err(End::Close(4401, "session ended")),
        Err(ChangeFeedError::TooManyHolds { .. }) => {
            return Err(End::Close(1013, "try again later"));
        }
        Err(error @ (ChangeFeedError::BadCursor | ChangeFeedError::Unexpected(_))) => {
            return Err(internal("feed read failed", &error));
        }
    };
    let epoch = u64::try_from(page.next_cursor.epoch).map_err(|error| internal("epoch", &error))?;
    for frame in change_frames(connection, epoch, &page.items).await? {
        send_binary(socket, frame, connection.limits.send_timeout).await?;
    }
    Ok(Some(page.next_cursor))
}

const fn reset_reason(reason: contracts::ResyncReason) -> ResetReason {
    match reason {
        contracts::ResyncReason::EpochChanged => ResetReason::EpochChanged,
        contracts::ResyncReason::BelowPurgeHorizon => ResetReason::BelowPurgeHorizon,
        contracts::ResyncReason::CursorAhead => ResetReason::CursorAhead,
    }
}

/// The head of the account, so the client can reload and subscribe from it.
async fn reset(
    socket: &mut WebSocket,
    connection: &Connection,
    reason: ResetReason,
) -> Result<(), End> {
    let head = connection
        .services
        .change_feed
        .head(connection.principal.account_id)
        .await
        .map_err(|error| internal("feed head failed", &error))?;
    let epoch = u64::try_from(head.epoch).map_err(|error| internal("epoch", &error))?;
    let seq = u64::try_from(head.seq).map_err(|error| internal("seq", &error))?;
    let frame =
        encode_reset(reason, epoch, seq).map_err(|error| internal("reset did not pack", &error))?;
    send_binary(socket, frame, connection.limits.send_timeout).await
}

/// The `Change` frames of one page, in page order, the last of each `seq` batch marked `end`. One thumbnail
/// state call per 1,000 distinct content hashes fills `thumb`.
async fn change_frames(
    connection: &Connection,
    epoch: u64,
    items: &[ChangeItem],
) -> Result<Vec<Vec<u8>>, End> {
    let states = thumb_states(connection, items).await?;
    let mut frames = Vec::with_capacity(items.len());
    for (index, item) in items.iter().enumerate() {
        let end = items.get(index + 1).is_none_or(|next| next.seq != item.seq);
        let frame = ChangeFrame {
            epoch,
            seq: u64::try_from(item.seq).map_err(|error| internal("seq", &error))?,
            node_id: item.node_id,
            folder: item.kind == NodeKind::Folder,
            thumb: item
                .content_hash
                .and_then(|hash| states.get(&hash))
                .map_or(0, |state| thumb_number(*state)),
            name: item.name.clone(),
            parent_id: item.parent_id,
            revision_id: item.revision_id,
            size: item
                .size
                .map(u64::try_from)
                .transpose()
                .map_err(|error| internal("size", &error))?,
            content_hash: item.content_hash.map(FileHash::into_bytes),
            media_type: item.media_type.clone(),
            deleted: item.state == NodeState::Tombstone,
            end,
        };
        frames
            .push(encode_change(&frame).map_err(|error| internal("change did not pack", &error))?);
    }
    Ok(frames)
}

async fn thumb_states(
    connection: &Connection,
    items: &[ChangeItem],
) -> Result<HashMap<FileHash, ThumbState>, End> {
    let mut seen = HashSet::new();
    let hashes: Vec<FileHash> = items
        .iter()
        .filter(|item| item.kind == NodeKind::File && is_media(item))
        .filter_map(|item| item.content_hash)
        .filter(|hash| seen.insert(*hash))
        .collect();
    let mut states = HashMap::with_capacity(hashes.len());
    for batch in hashes.chunks(STATE_BATCH) {
        let part = connection
            .services
            .thumbnail_states
            .states(connection.principal.account_id, batch)
            .await
            .map_err(|error| internal("thumbnail states failed", &error))?;
        states.extend(part);
    }
    Ok(states)
}

const fn thumb_number(state: ThumbState) -> u8 {
    match state {
        ThumbState::None => 0,
        ThumbState::Pending => 1,
        ThumbState::Ready => 2,
        ThumbState::Failed => 3,
    }
}

async fn send_binary(socket: &mut WebSocket, frame: Vec<u8>, limit: Duration) -> Result<(), End> {
    send_message(socket, Message::Binary(frame.into()), limit).await
}

/// A send that does not finish within `limit` ends the connection as slow; one that fails ends it as gone.
async fn send_message(
    socket: &mut WebSocket,
    message: Message,
    limit: Duration,
) -> Result<(), End> {
    match timeout(limit, socket.send(message)).await {
        Ok(Ok(())) => Ok(()),
        Ok(Err(error)) => {
            log_debug!("realtime send failed", err = &error);
            Err(End::Gone)
        }
        Err(_) => {
            log_warn!("realtime send timed out, the client stopped reading");
            Err(End::Slow)
        }
    }
}

/// A file whose sniffed type is an image or a video is the only kind that has a preview; the same rule as the
/// tree listing's. Without a type (a feed that does not know it) the file is asked about.
fn is_media(item: &ChangeItem) -> bool {
    item.media_type.as_deref().is_none_or(|media| {
        let top = media.split('/').next().unwrap_or_default();
        top.eq_ignore_ascii_case("image") || top.eq_ignore_ascii_case("video")
    })
}
