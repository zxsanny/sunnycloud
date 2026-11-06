//! The connection task: connects (with the trust check), serves commands and frames, and
//! reconnects. All connection state lives here, so there is no lock.

use std::collections::HashMap;
use std::time::Duration;

use ipc_schema::{
    MethodCall, Notification, Request, RequestId, Response, ServerMessage, StatusEvent, encode_line,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::{mpsc, oneshot, watch};
use tokio::time::{sleep, timeout};

use super::backoff::Backoff;
use super::endpoint::{self, Stream};
use super::frames::FrameReader;
use super::{ClientConfig, ClientError, Command, ConnectionState, Reply};

const READ_CHUNK: usize = 64 * 1024;

type Pending = HashMap<i64, oneshot::Sender<Reply>>;

enum SessionEnd {
    /// Every client handle is gone.
    Shutdown,
    Lost(ClientError),
}

pub(super) struct Actor {
    config: ClientConfig,
    commands: mpsc::UnboundedReceiver<Command>,
    state: watch::Sender<ConnectionState>,
    events: mpsc::UnboundedSender<StatusEvent>,
}

impl Actor {
    pub(super) fn new(
        config: ClientConfig,
        commands: mpsc::UnboundedReceiver<Command>,
        state: watch::Sender<ConnectionState>,
        events: mpsc::UnboundedSender<StatusEvent>,
    ) -> Self {
        Self {
            config,
            commands,
            state,
            events,
        }
    }
}

pub(super) async fn run(mut actor: Actor) {
    let mut backoff = Backoff::new(actor.config.backoff_min, actor.config.backoff_max);
    let mut failed_attempts = 0_u32;
    loop {
        let attempt = timeout(
            actor.config.request_timeout,
            endpoint::connect(&actor.config),
        )
        .await
        .unwrap_or_else(|_| Err(ClientError::Io("connecting timed out".to_owned())));
        let cause = match attempt {
            Ok(stream) => {
                failed_attempts = 0;
                backoff.reset();
                actor.state.send_replace(ConnectionState::Connected);
                match actor.session(stream).await {
                    SessionEnd::Shutdown => return,
                    SessionEnd::Lost(cause) => {
                        tracing::warn!(%cause, "connection lost");
                        actor.state.send_replace(ConnectionState::Connecting);
                        ClientError::ConnectionLost
                    }
                }
            }
            Err(cause) => {
                failed_attempts += 1;
                actor.report_down(failed_attempts, &cause);
                cause
            }
        };
        if !actor.wait(backoff.next_delay(), &cause).await {
            return;
        }
    }
}

impl Actor {
    /// WARN once when the reason first appears; a daemon that is simply absent is normal.
    fn report_down(&self, attempt: u32, cause: &ClientError) {
        let repeated = matches!(
            &*self.state.borrow(),
            ConnectionState::Down { cause: previous, .. }
                if std::mem::discriminant(previous) == std::mem::discriminant(cause)
        );
        if !repeated {
            match cause {
                ClientError::EndpointUntrusted(reason) => {
                    tracing::warn!(reason, "endpoint untrusted");
                }
                ClientError::Io(reason) => {
                    tracing::warn!(reason, "daemon connection failed");
                }
                _ => {}
            }
        }
        self.state.send_replace(ConnectionState::Down {
            attempt,
            cause: cause.clone(),
        });
    }

    /// Sleeps for `delay`, answering every call that arrives meanwhile with `cause`.
    /// `false` when every client handle is gone.
    async fn wait(&mut self, delay: Duration, cause: &ClientError) -> bool {
        let sleeping = sleep(delay);
        tokio::pin!(sleeping);
        loop {
            tokio::select! {
                () = &mut sleeping => return true,
                command = self.commands.recv() => match command {
                    None => return false,
                    Some(Command::Request { reply, .. }) => answer(reply, Err(cause.clone())),
                    Some(Command::Forget(_)) => {}
                },
            }
        }
    }

    async fn session(&mut self, stream: Stream) -> SessionEnd {
        let (mut reader, mut writer) = tokio::io::split(stream);
        let mut pending = Pending::new();
        let mut highest_sent = 0_i64;
        let mut frames = FrameReader::new(self.config.max_frame_bytes);
        let mut chunk = vec![0_u8; READ_CHUNK];
        let end = loop {
            tokio::select! {
                command = self.commands.recv() => match command {
                    None => break SessionEnd::Shutdown,
                    Some(Command::Forget(id)) => {
                        pending.remove(&id);
                    }
                    Some(Command::Request { id, call, reply }) => {
                        highest_sent = highest_sent.max(id);
                        let line = match encode_request(id, &call) {
                            Ok(line) => line,
                            Err(cause) => {
                                answer(reply, Err(cause));
                                continue;
                            }
                        };
                        pending.insert(id, reply);
                        let written = timeout(self.config.request_timeout, async {
                            writer.write_all(&line).await?;
                            writer.flush().await
                        })
                        .await;
                        match written {
                            Ok(Ok(())) => {}
                            Ok(Err(error)) => break SessionEnd::Lost(ClientError::Io(error.to_string())),
                            Err(_) => break SessionEnd::Lost(ClientError::Io("write stalled".to_owned())),
                        }
                    }
                },
                read = reader.read(&mut chunk) => match read {
                    Ok(0) => break SessionEnd::Lost(ClientError::ConnectionLost),
                    Err(error) => break SessionEnd::Lost(ClientError::Io(error.to_string())),
                    Ok(read) => {
                        let received = self.receive(&mut frames, &chunk[..read], &mut pending, highest_sent);
                        if let Err(cause) = received {
                            break SessionEnd::Lost(cause);
                        }
                    }
                },
            }
        };
        let cause = match &end {
            SessionEnd::Lost(cause) => cause.clone(),
            SessionEnd::Shutdown => ClientError::Closed,
        };
        for (_, reply) in pending.drain() {
            answer(reply, Err(cause.clone()));
        }
        end
    }

    /// `Err` only for a violation that makes the rest of the stream untrustworthy.
    fn receive(
        &self,
        frames: &mut FrameReader,
        bytes: &[u8],
        pending: &mut Pending,
        highest_sent: i64,
    ) -> Result<(), ClientError> {
        if frames.push(bytes).is_err() {
            tracing::error!(
                limit = self.config.max_frame_bytes,
                "protocol violation: frame exceeds the size limit"
            );
            return Err(ClientError::Protocol(
                "frame exceeds the size limit".to_owned(),
            ));
        }
        while let Some(line) = frames.next_line() {
            self.handle_line(&line, pending, highest_sent);
        }
        Ok(())
    }

    fn handle_line(&self, line: &[u8], pending: &mut Pending, highest_sent: i64) {
        let message = match serde_json::from_slice::<ServerMessage>(line) {
            Ok(message) => message,
            Err(error) => {
                // The text of a serde error can quote the input, which may hold user paths.
                tracing::error!(kind = ?error.classify(), column = error.column(), "protocol violation: line is not a JSON-RPC message");
                return;
            }
        };
        match message {
            ServerMessage::Notification(notification) => self.handle_notification(&notification),
            ServerMessage::Response(response) => handle_response(response, pending, highest_sent),
        }
    }

    fn handle_notification(&self, notification: &Notification) {
        match notification.as_status_event() {
            Ok(Some(event)) => {
                if self.events.send(event).is_err() {
                    // The host dropped the receiver: nobody consumes events any more.
                }
            }
            Ok(None) => {
                tracing::warn!(method = %notification.method, "notification of an unknown method ignored")
            }
            Err(error) => {
                tracing::error!(kind = ?error.classify(), "protocol violation: status event does not decode");
            }
        }
    }
}

fn handle_response(response: Response, pending: &mut Pending, highest_sent: i64) {
    let Some(RequestId::Number(id)) = response.id else {
        tracing::error!("protocol violation: answer without a usable id");
        return;
    };
    match pending.remove(&id) {
        Some(reply) => answer(reply, ClientError::from_outcome(response.outcome)),
        None if (1..=highest_sent).contains(&id) => {
            // The answer of a call that already timed out: dropped, as the contract says.
        }
        None => tracing::error!("protocol violation: answer to a request that was never sent"),
    }
}

fn answer(reply: oneshot::Sender<Reply>, value: Reply) {
    if reply.send(value).is_err() {
        // The caller stopped waiting (timeout or cancellation); nobody wants the answer.
    }
}

fn encode_request(id: i64, call: &MethodCall) -> Result<Vec<u8>, ClientError> {
    Request::new(RequestId::Number(id), call)
        .and_then(|request| encode_line(&request))
        .map_err(|error| ClientError::Protocol(format!("request cannot be encoded: {error}")))
}
