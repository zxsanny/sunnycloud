//! One client connection of the stub: reads request lines, records them, and answers each
//! by script or by the default daemon. Unscripted answers are sent inline, so the stub adds
//! no delay of its own; only a scripted `delay` waits, and it does so off the read loop.

use std::io;
use std::sync::Arc;
use std::time::Duration;

use ipc_schema::{
    ControlError, MethodCall, Notification, RequestId, Response, RpcError, StatusEvent, StatusSink,
    decode_call, dispatch, encode_line,
};
use serde_json::{Value, json};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::sync::{mpsc, watch};
use tokio::time::{Instant, interval_at, sleep};

use super::Shared;
use crate::script::{Answer, EventSchedule, Hostile};

pub const MARKUP: &str = "<b>x</b>{@html}<script>";
const WRONG_ID: &str = "hostile-wrong-id";
const READ_CHUNK: usize = 64 * 1024;

type Frames = mpsc::UnboundedSender<Vec<u8>>;

/// Delivers `StatusEvent`s to the connection's writer.
struct ChannelSink(Frames);

impl StatusSink for ChannelSink {
    fn send(&self, event: StatusEvent) {
        match Notification::status_event(&event).and_then(|n| encode_line(&n)) {
            Ok(line) => {
                if self.0.send(line).is_err() {
                    // The connection is closed; nobody is left to notify.
                }
            }
            Err(error) => eprintln!("stub: status event cannot be encoded: {error}"),
        }
    }
}

struct Request {
    method: String,
    id: Option<RequestId>,
    params: Option<Value>,
}

impl Request {
    fn from_value(value: &Value) -> Option<Self> {
        let method = value.get("method")?.as_str()?.to_owned();
        let id = value
            .get("id")
            .and_then(|id| serde_json::from_value(id.clone()).ok());
        Some(Self {
            method,
            id,
            params: value.get("params").cloned(),
        })
    }
}

pub(crate) async fn serve<S>(shared: Arc<Shared>, stream: S)
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let (mut reader, mut writer) = tokio::io::split(stream);
    let (frames, mut outbound) = mpsc::unbounded_channel::<Vec<u8>>();
    let writer_task = tokio::spawn(async move {
        while let Some(frame) = outbound.recv().await {
            writer.write_all(&frame).await?;
        }
        writer.flush().await
    });
    let sink: Arc<dyn StatusSink> = Arc::new(ChannelSink(frames.clone()));

    let mut lines = ipc_schema::LineBuffer::default();
    let mut chunk = vec![0_u8; READ_CHUNK];
    loop {
        match reader.read(&mut chunk).await {
            Ok(0) => break,
            Ok(read) => lines.push(&chunk[..read]),
            Err(error) => {
                report("read", &error);
                break;
            }
        }
        while let Some(line) = lines.next_line() {
            handle_line(&shared, &frames, &sink, line).await;
        }
    }
    drop(frames);
    drop(sink);
    match writer_task.await {
        Ok(Ok(())) => {}
        Ok(Err(error)) => report("write", &error),
        Err(joined) => eprintln!("stub: connection writer failed: {joined}"),
    }
}

/// A peer that went away is the normal end of a connection; anything else is reported.
fn report(action: &str, error: &io::Error) {
    if !matches!(
        error.kind(),
        io::ErrorKind::BrokenPipe | io::ErrorKind::ConnectionReset | io::ErrorKind::UnexpectedEof
    ) {
        eprintln!("stub: connection {action} failed: {error}");
    }
}

async fn handle_line(shared: &Shared, frames: &Frames, sink: &Arc<dyn StatusSink>, line: Vec<u8>) {
    let parsed = serde_json::from_slice::<Value>(&line);
    let request = parsed.as_ref().ok().and_then(Request::from_value);
    let scripted = match &request {
        Some(request) => {
            shared.record(&request.method, request.params.clone());
            shared.next_answer(&request.method)
        }
        None => Answer::Default,
    };

    let (delay, answer) = resolve(scripted);
    let frame = match (&answer, request.as_ref().and_then(|r| r.id.clone())) {
        (Answer::Default, _) | (_, None) => default_frame(shared, sink, &line, request).await,
        (answer, Some(id)) => scripted_frame(answer, &id, &line),
    };
    let Some(frame) = frame else { return };

    if delay.is_zero() {
        send(frames, frame);
    } else {
        let frames = frames.clone();
        tokio::spawn(async move {
            sleep(delay).await;
            send(&frames, frame);
        });
    }
}

fn send(frames: &Frames, frame: Vec<u8>) {
    if frames.send(frame).is_err() {
        // The client disconnected before the (delayed) answer was due.
    }
}

/// Sums nested delays and returns the answer that is finally sent.
fn resolve(mut answer: Answer) -> (Duration, Answer) {
    let mut total = Duration::ZERO;
    while let Answer::Delay { ms, then } = answer {
        total += Duration::from_millis(ms);
        answer = *then;
    }
    (total, answer)
}

fn encode(response: &Response) -> Vec<u8> {
    encode_line(response).expect("a response of JSON values always serializes")
}

async fn default_frame(
    shared: &Shared,
    sink: &Arc<dyn StatusSink>,
    line: &[u8],
    request: Option<Request>,
) -> Option<Vec<u8>> {
    if let Some(request) = &request
        && let (Some(id), Some(error)) = (&request.id, precheck(request))
    {
        return Some(encode(&Response::from_outcome::<()>(
            id.clone(),
            Err(error),
        )));
    }
    let response = match decode_call(line) {
        Ok(call) => {
            let subscribes = matches!(call.call, MethodCall::Subscribe(_));
            let response = dispatch(&shared.daemon, sink, call.id, call.call).await;
            if subscribes
                && matches!(response.outcome, ipc_schema::Outcome::Success { .. })
                && let Some(schedule) = &shared.events
            {
                tokio::spawn(run_schedule(
                    schedule.clone(),
                    Arc::clone(sink),
                    shared.stop_events.subscribe(),
                ));
            }
            response
        }
        Err(response) => response,
    };
    Some(encode(&response))
}

/// Values the typed schema cannot carry but the contract names an error for:
/// `set_settings` with a negative bandwidth is `SettingInvalid` with the settings
/// unchanged (daemon_control.md, "bad-setting"), not a decode failure.
fn precheck(request: &Request) -> Option<ControlError> {
    if request.method != "set_settings" {
        return None;
    }
    let bandwidth = request.params.as_ref()?.get("bandwidth_limit")?;
    let negative =
        bandwidth.as_i64().is_some_and(|n| n < 0) || bandwidth.as_f64().is_some_and(|n| n < 0.0);
    negative.then(|| ControlError::SettingInvalid {
        key: "bandwidth_limit".to_owned(),
    })
}

fn scripted_frame(answer: &Answer, id: &RequestId, line: &[u8]) -> Option<Vec<u8>> {
    match answer {
        Answer::Silent => None,
        Answer::Result(value) => Some(encode(&checked_result(value, id, line))),
        Answer::Error(error) => Some(encode(&Response::from_outcome::<()>(
            id.clone(),
            Err(error.clone()),
        ))),
        Answer::Hostile(hostile) => Some(hostile_frame(hostile, id)),
        Answer::Default | Answer::Delay { .. } => {
            unreachable!("default and delay are handled before a scripted frame is built")
        }
    }
}

/// A scripted result that does not decode as the method's result type is a script bug;
/// it is answered with an Internal error that names it instead of reaching the client as
/// a malformed success.
fn checked_result(value: &Value, id: &RequestId, line: &[u8]) -> Response {
    if let Ok(call) = decode_call(line)
        && let Err(error) = call.call.validate_result(value.clone())
    {
        return Response::failure(
            Some(id.clone()),
            RpcError::internal(format!("scripted result does not decode: {error}")),
        );
    }
    success(id, value.clone())
}

fn hostile_frame(hostile: &Hostile, id: &RequestId) -> Vec<u8> {
    match hostile {
        Hostile::Oversize { bytes } => {
            let base = encode(&success(id, json!("")));
            let padding = bytes.saturating_sub(base.len());
            encode(&success(id, Value::String("x".repeat(padding))))
        }
        Hostile::InvalidJson => b"{\"jsonrpc\": \"2.0\", \"id\": \n".to_vec(),
        Hostile::WrongId => encode(&success(&RequestId::Text(WRONG_ID.to_owned()), Value::Null)),
        Hostile::MarkupError => encode(&Response::from_outcome::<()>(
            id.clone(),
            Err(ControlError::UpgradeRequired {
                message: MARKUP.to_owned(),
            }),
        )),
        Hostile::Raw(text) => {
            let mut line = text.clone().into_bytes();
            line.push(b'\n');
            line
        }
    }
}

fn success(id: &RequestId, result: Value) -> Response {
    Response {
        jsonrpc: ipc_schema::Version,
        id: Some(id.clone()),
        outcome: ipc_schema::Outcome::Success { result },
    }
}

async fn run_schedule(
    schedule: EventSchedule,
    sink: Arc<dyn StatusSink>,
    mut stop: watch::Receiver<bool>,
) {
    let period = Duration::from_millis(schedule.interval_ms.max(1));
    let mut tick = interval_at(Instant::now() + period, period);
    for status in schedule.statuses {
        if *stop.borrow() {
            return;
        }
        tokio::select! {
            _ = tick.tick() => sink.send(StatusEvent { status }),
            _ = stop.changed() => return,
        }
    }
}
