//! The realtime channel: `GET /api/v1/realtime`, a WebSocket that pushes the account's changes as `packbin`
//! frames (`_docs/02_document/contracts/11_http_api/realtime.md`).
//!
//! The handshake is an ordinary registry route (session auth, the version gate, then this handler): the
//! `Origin` of a cookie-authenticated handshake must equal `public_url`, and a per-account cap bounds the open
//! sockets. After the upgrade one task per socket reads the feed through the same `ChangeFeed::read` as
//! `GET /changes`, so there is one source of truth.

mod cap;
mod frames;
mod session;

use std::sync::Arc;
use std::time::Duration;

use axum::Extension;
use axum::extract::State;
use axum::extract::ws::WebSocketUpgrade;
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Response};
use axum::routing::on;
use sc_error::{BusinessError, BusinessParams, CorrelationId, DomainError, ErrorCode};

use self::cap::ConnectionCap;
use self::session::Connection;
use super::auth::{Authenticated, CredentialSource};
use super::csrf::CsrfGuard;
use super::problem::ApiProblem;
use super::registry::{AuthClass, BodyClass, HttpMethod, RouteContext, RouteDef, RouteSpec};
use super::services::ApiServices;

const PATH: &str = "/api/v1/realtime";

/// The `Retry-After` of a handshake refused for the cap.
const CAP_RETRY_AFTER: Duration = Duration::from_secs(5);

const ERRORS: &[ErrorCode] = &[ErrorCode::CsrfRejected, ErrorCode::RateLimited];

const CONNECT: RouteSpec = RouteSpec::new(
    HttpMethod::Get,
    PATH,
    "ChangeFeed::read",
    AuthClass::Session,
    BodyClass::None,
)
.summary("A WebSocket pushing the account's changes as binary frames (realtime.md)")
.success(101)
.errors(ERRORS);

pub(super) fn routes() -> Vec<RouteDef> {
    vec![RouteDef::new(CONNECT, |context, filter| {
        on(filter, connect).with_state(Arc::new(Realtime::new(context)))
    })]
}

/// The numbers of `realtime.md`: the defaults are the contract's, and tests shorten them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RealtimeLimits {
    /// Open sockets per account; the next handshake is 429.
    pub max_connections_per_account: usize,
    /// The client `Ping` interval told in `Hello`, and the interval of the server's own WebSocket ping.
    pub ping_interval: Duration,
    /// No inbound frame for this long ends a subscribed socket (4408).
    pub idle_timeout: Duration,
    /// No `Subscribe` within this long after `Hello` ends the socket (4408).
    pub subscribe_timeout: Duration,
    /// A send that takes longer ends the socket (4429).
    pub send_timeout: Duration,
    /// The largest message the server reads (1009 above it).
    pub max_client_frame: usize,
}

impl Default for RealtimeLimits {
    fn default() -> Self {
        RealtimeLimits {
            max_connections_per_account: 8,
            ping_interval: Duration::from_secs(30),
            idle_timeout: Duration::from_secs(90),
            subscribe_timeout: Duration::from_secs(10),
            send_timeout: Duration::from_secs(10),
            max_client_frame: 1024,
        }
    }
}

impl RealtimeLimits {
    pub(crate) fn ping_interval_secs(&self) -> u16 {
        u16::try_from(self.ping_interval.as_secs()).unwrap_or(u16::MAX)
    }

    pub(crate) fn max_client_frame_bytes(&self) -> u32 {
        u32::try_from(self.max_client_frame).unwrap_or(u32::MAX)
    }
}

struct Realtime {
    services: Arc<ApiServices>,
    csrf: Arc<CsrfGuard>,
    cap: Arc<ConnectionCap>,
    limits: RealtimeLimits,
}

impl Realtime {
    fn new(context: &RouteContext) -> Self {
        Realtime {
            services: Arc::clone(&context.services),
            csrf: Arc::clone(&context.csrf),
            cap: ConnectionCap::new(context.realtime.max_connections_per_account),
            limits: context.realtime,
        }
    }
}

/// Origin, cap, then the upgrade. Auth and the version gate have run; a request that is not an upgrade at all
/// is the extractor's own 400.
async fn connect(
    State(realtime): State<Arc<Realtime>>,
    Extension(caller): Extension<Authenticated>,
    request_id: Option<Extension<CorrelationId>>,
    headers: HeaderMap,
    upgrade: WebSocketUpgrade,
) -> Response {
    let request_id = request_id.map_or_else(
        || CorrelationId::new(sc_ids::new_id(realtime.services.clock.as_ref())),
        |Extension(id)| id,
    );
    if caller.source == CredentialSource::Cookie && !realtime.csrf.origin_is_public(&headers) {
        return ApiProblem::code(ErrorCode::CsrfRejected, request_id).into_response();
    }
    let Some(permit) = realtime.cap.acquire(caller.principal.account_id) else {
        let error = BusinessError::new(
            ErrorCode::RateLimited,
            BusinessParams {
                retry_after: Some(CAP_RETRY_AFTER),
                ..BusinessParams::default()
            },
        );
        return ApiProblem::new(DomainError::Business(error), request_id).into_response();
    };
    let connection = Connection {
        services: Arc::clone(&realtime.services),
        principal: caller.principal,
        request_id,
        limits: realtime.limits,
        _permit: permit,
    };
    upgrade
        .max_message_size(realtime.limits.max_client_frame)
        .max_frame_size(realtime.limits.max_client_frame)
        .read_buffer_size(4 * 1024)
        .write_buffer_size(0)
        .max_write_buffer_size(1024 * 1024)
        .on_upgrade(move |socket| session::run(socket, connection))
}
