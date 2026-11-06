//! Accept loop, per-connection serving and the request handler of the scripted fixture.

use std::io;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use bytes::{Bytes, BytesMut};
use hyper::body::{Body, Incoming};
use hyper::service::service_fn;
use hyper::{Request, Response, StatusCode};
use hyper_util::rt::{TokioExecutor, TokioIo};
use serde_json::Value;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::watch;
use tokio::task::JoinSet;
use tokio_rustls::TlsAcceptor;

use super::answer::{Answer, Fault, request_id};
use super::body::{End, ScriptedBody};
use super::log::{AppliedFault, ConnInfo, InFlightBlock, LoggedRequest, Stats};
use super::render::{Rendered, render};
use super::route::{Route, resolve};
use super::script::Script;

const WINDOW_STREAM: u32 = 8 * 1024 * 1024;
const WINDOW_CONN: u32 = 32 * 1024 * 1024;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Api,
    Catcher,
}

/// State shared by every connection of one fixture. Nothing is shared between fixtures.
pub struct Shared {
    script: Mutex<Script>,
    log: Mutex<Vec<LoggedRequest>>,
    catcher_log: Mutex<Vec<LoggedRequest>>,
    stats: Mutex<Stats>,
    next_request_id: AtomicU64,
    pub flush_grace: Duration,
    pub catcher_origin: Mutex<String>,
}

fn locked<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Shared {
    pub fn new(script: Script, flush_grace: Duration) -> Self {
        Shared {
            script: Mutex::new(script),
            log: Mutex::new(Vec::new()),
            catcher_log: Mutex::new(Vec::new()),
            stats: Mutex::new(Stats::default()),
            next_request_id: AtomicU64::new(1),
            flush_grace,
            catcher_origin: Mutex::new(String::new()),
        }
    }

    pub fn stats(&self) -> MutexGuard<'_, Stats> {
        locked(&self.stats)
    }

    pub fn requests(&self) -> Vec<LoggedRequest> {
        locked(&self.log).clone()
    }

    pub fn catcher_requests(&self) -> Vec<LoggedRequest> {
        locked(&self.catcher_log).clone()
    }

    fn record(&self, role: Role, mut entry: LoggedRequest) -> u64 {
        let log = if role == Role::Api {
            &self.log
        } else {
            &self.catcher_log
        };
        let mut log = locked(log);
        entry.seq = log.len() as u64;
        let seq = entry.seq;
        log.push(entry);
        seq
    }

    fn block_started(self: &Arc<Self>) -> InFlightBlock {
        let mut stats = self.stats();
        stats.block_puts_in_flight += 1;
        stats.max_block_puts_in_flight = stats
            .max_block_puts_in_flight
            .max(stats.block_puts_in_flight);
        drop(stats);
        InFlightBlock(Arc::clone(self))
    }

    fn note_staged(&self, route: Route, status: u16, target: &str, body: &[u8]) {
        if !(200..300).contains(&status) {
            return;
        }
        let mut stats = self.stats();
        match route {
            Route::StagedUpload => {
                let key = target.split('?').next().and_then(|p| p.rsplit('/').next());
                if let Some(key) = key {
                    stats.staged_uncommitted.insert(key.to_owned());
                    stats.max_staged_uncommitted = stats
                        .max_staged_uncommitted
                        .max(stats.staged_uncommitted.len());
                }
            }
            Route::BatchCommit => {
                let parsed = serde_json::from_slice::<Value>(body).ok();
                let entries = parsed.as_ref().and_then(|v| v.get("entries")?.as_array());
                for entry in entries.into_iter().flatten() {
                    if let Some(key) = entry.get("staged_key").and_then(Value::as_str) {
                        stats.staged_uncommitted.remove(key);
                    }
                }
            }
            _ => {}
        }
    }
}

/// Lets a handler or a body close the TCP connection it runs on.
#[derive(Clone)]
pub struct ConnControl(Arc<watch::Sender<bool>>);

impl ConnControl {
    pub fn abort(&self) {
        self.0.send_replace(true);
    }
}

struct ConnCtx {
    shared: Arc<Shared>,
    role: Role,
    id: u64,
    control: ConnControl,
}

pub async fn accept_loop(
    listener: TcpListener,
    acceptor: TlsAcceptor,
    shared: Arc<Shared>,
    role: Role,
) {
    let mut connections = JoinSet::new();
    loop {
        tokio::select! {
            accepted = listener.accept() => match accepted {
                Ok((tcp, _)) => {
                    connections.spawn(serve(tcp, acceptor.clone(), Arc::clone(&shared), role));
                }
                Err(error) => shared
                    .stats()
                    .connection_errors
                    .push(format!("accept: {error}")),
            },
            Some(_) = connections.join_next() => {}
        }
    }
}

async fn serve(tcp: TcpStream, acceptor: TlsAcceptor, shared: Arc<Shared>, role: Role) {
    let tls = match acceptor.accept(tcp).await {
        Ok(tls) => tls,
        Err(error) => {
            shared.stats().handshake_failures.push(error.to_string());
            return;
        }
    };
    let (_, session) = tls.get_ref();
    let info = ConnInfo {
        tls_version: session.protocol_version(),
        alpn: session
            .alpn_protocol()
            .map(|p| String::from_utf8_lossy(p).into_owned()),
    };
    let h2 = info.alpn.as_deref() == Some("h2");
    let id = {
        let mut stats = shared.stats();
        stats.connections.push(info);
        stats.connections.len() as u64 - 1
    };
    let (abort_tx, mut abort_rx) = watch::channel(false);
    let ctx = Arc::new(ConnCtx {
        shared: Arc::clone(&shared),
        role,
        id,
        control: ConnControl(Arc::new(abort_tx)),
    });
    let service = service_fn(move |request| handle(Arc::clone(&ctx), request));
    let io = TokioIo::new(tls);
    let outcome = if h2 {
        let connection = hyper::server::conn::http2::Builder::new(TokioExecutor::new())
            .initial_stream_window_size(WINDOW_STREAM)
            .initial_connection_window_size(WINDOW_CONN)
            .serve_connection(io, service);
        tokio::select! {
            result = connection => Some(result),
            _ = abort_rx.wait_for(|aborted| *aborted) => None,
        }
    } else {
        let connection = hyper::server::conn::http1::Builder::new().serve_connection(io, service);
        tokio::select! {
            result = connection => Some(result),
            _ = abort_rx.wait_for(|aborted| *aborted) => None,
        }
    };
    if let Some(Err(error)) = outcome {
        shared.stats().connection_errors.push(error.to_string());
    }
}

async fn read_body(mut body: Incoming) -> Result<Bytes, (Bytes, String)> {
    let mut data = BytesMut::new();
    while !body.is_end_stream() {
        match std::future::poll_fn(|cx| std::pin::Pin::new(&mut body).poll_frame(cx)).await {
            Some(Ok(frame)) => {
                if let Ok(chunk) = frame.into_data() {
                    data.extend_from_slice(&chunk);
                }
            }
            Some(Err(error)) => return Err((data.freeze(), error.to_string())),
            None => break,
        }
    }
    Ok(data.freeze())
}

fn aborted(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::ConnectionAborted, message.to_owned())
}

async fn handle(
    ctx: Arc<ConnCtx>,
    request: Request<Incoming>,
) -> Result<Response<ScriptedBody>, io::Error> {
    let shared = &ctx.shared;
    let (parts, incoming) = request.into_parts();
    let target = parts
        .uri
        .path_and_query()
        .map_or_else(|| parts.uri.path().to_owned(), ToString::to_string);
    let route = (ctx.role == Role::Api)
        .then(|| resolve(&parts.method, parts.uri.path()))
        .flatten();
    let in_flight = (route == Some(Route::PutBlock)).then(|| shared.block_started());
    let (body, body_error) = match read_body(incoming).await {
        Ok(body) => (body, None),
        Err((partial, error)) => (partial, Some(error)),
    };
    let headers: Vec<(String, String)> = parts
        .headers
        .iter()
        .filter(|(name, _)| name.as_str() != "host")
        .map(|(name, value)| {
            (
                name.as_str().to_owned(),
                String::from_utf8_lossy(value.as_bytes()).into_owned(),
            )
        })
        .collect();
    let mut entry = LoggedRequest {
        seq: 0,
        connection: ctx.id,
        route,
        method: parts.method.to_string(),
        target: target.clone(),
        headers,
        body: body.clone(),
        scripted: true,
        answered: None,
        fault: None,
    };

    if ctx.role == Role::Catcher {
        entry.answered = Some(200);
        shared.record(Role::Catcher, entry);
        return Ok(plain(200, &ctx.control));
    }
    let scripted = route.and_then(|route| locked(&shared.script).next(route, &entry));
    entry.scripted = scripted.is_some();
    let answer = scripted.unwrap_or_else(|| unscripted(route, &parts.method, &target));
    if let Some(error) = body_error {
        entry.answered = None;
        let seq = shared.record(Role::Api, entry);
        shared
            .stats()
            .request_errors
            .push(format!("request {seq}: {error}"));
        return Err(aborted("request body incomplete"));
    }

    let origin = locked(&shared.catcher_origin).clone();
    let range = parts.headers.get("range").and_then(|v| v.to_str().ok());
    let id = shared.next_request_id.fetch_add(1, Ordering::Relaxed);
    let rendered = render(&answer, range, request_id(id), &origin);
    let (end, stop) = fault_end(answer.fault, rendered.body.len());
    let answered = match answer.fault {
        Fault::DropConnection | Fault::ResetStream => None,
        _ => Some(rendered.status),
    };
    entry.answered = answered;
    entry.fault = fault_record(answer.fault, stop);
    shared.record(Role::Api, entry);
    if let (Some(route), Some(status)) = (route, answered) {
        shared.note_staged(route, status, &target, &body);
    }

    if !answer.delay.is_zero() {
        tokio::time::sleep(answer.delay).await;
    }
    match answer.fault {
        Fault::DropConnection => {
            ctx.control.abort();
            Err(aborted("scripted connection drop"))
        }
        Fault::ResetStream => Err(aborted("scripted stream reset")),
        _ => Ok(response(
            rendered,
            end,
            stop,
            shared.flush_grace,
            &ctx.control,
            in_flight,
        )),
    }
}

fn fault_end(fault: Fault, len: usize) -> (End, usize) {
    match fault {
        Fault::DropAfterBytes(n) => (End::DropConnection, n.min(len)),
        Fault::ResetAfterBytes(n) => (End::Reset, n.min(len)),
        _ => (End::Clean, len),
    }
}

fn fault_record(fault: Fault, stop: usize) -> Option<AppliedFault> {
    let kind = match fault {
        Fault::None => return None,
        Fault::DropConnection => "drop_connection",
        Fault::DropAfterBytes(_) => "drop_after_bytes",
        Fault::ResetStream => "reset_stream",
        Fault::ResetAfterBytes(_) => "reset_after_bytes",
    };
    Some(AppliedFault {
        kind,
        at_byte: stop,
    })
}

fn response(
    rendered: Rendered,
    end: End,
    stop: usize,
    grace: Duration,
    control: &ConnControl,
    in_flight: Option<InFlightBlock>,
) -> Response<ScriptedBody> {
    let mut builder = Response::builder()
        .status(StatusCode::from_u16(rendered.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR));
    for (name, value) in &rendered.headers {
        builder = builder.header(name, value);
    }
    let body = ScriptedBody::new(rendered.body, stop, end, grace, control.clone(), in_flight);
    builder
        .body(body)
        .expect("scripted headers are valid header names and values")
}

fn plain(status: u16, control: &ConnControl) -> Response<ScriptedBody> {
    let rendered = render(&Answer::status(status), None, request_id(0), "");
    response(rendered, End::Clean, 0, Duration::ZERO, control, None)
}

/// The answer for a request the script has nothing left for; the tests treat it as a failure of the test.
fn unscripted(route: Option<Route>, method: &hyper::Method, target: &str) -> Answer {
    let (status, code) = match route {
        Some(_) => (501, "unscripted_answer"),
        None => (404, "not_found"),
    };
    Answer::problem_with(code, status, "No scripted answer")
        .extra("detail", Value::from(format!("{method} {target}")))
}
