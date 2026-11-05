//! The JSON envelope and the strict parser. One value per frame, tagged by its role:
//! `{"request": ...}`, `{"response": ...}` or `{"event": ...}`.

use serde::{Deserialize, Serialize};

use crate::decoder_protocol::ranges::{check_event, check_request, check_response};
use crate::decoder_protocol::{Event, ProtocolViolation, Request, Response, ViolationKind};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Message {
    Request(Request),
    Response(Response),
    Event(Event),
}

/// What the runner sends: one response per request, plus events while a job runs.
// A short-lived value handed straight to a match: boxing would add an allocation per reply.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq)]
pub enum Reply {
    Response(Response),
    Event(Event),
}

impl From<Reply> for Message {
    fn from(reply: Reply) -> Self {
        match reply {
            Reply::Response(response) => Message::Response(response),
            Reply::Event(event) => Message::Event(event),
        }
    }
}

fn violation(kind: ViolationKind) -> ProtocolViolation {
    ProtocolViolation(kind)
}

/// Checks the ranges of a message that is already typed.
pub(crate) fn check(message: &Message) -> Result<(), ProtocolViolation> {
    match message {
        Message::Request(request) => check_request(request),
        Message::Response(response) => check_response(response),
        Message::Event(event) => check_event(event),
    }
}

/// Parses one frame payload: UTF-8, one JSON value, the schema, then the range table.
pub fn parse_message(payload: &[u8]) -> Result<Message, ProtocolViolation> {
    let text = std::str::from_utf8(payload).map_err(|_| violation(ViolationKind::NotUtf8))?;
    let message: Message = serde_json::from_str(text).map_err(|error| {
        violation(if error.is_data() {
            ViolationKind::Schema
        } else {
            ViolationKind::Json
        })
    })?;
    check(&message)?;
    Ok(message)
}

/// Runner side: only a request is accepted.
pub fn parse_request(payload: &[u8]) -> Result<Request, ProtocolViolation> {
    match parse_message(payload)? {
        Message::Request(request) => Ok(request),
        Message::Response(_) | Message::Event(_) => Err(violation(ViolationKind::WrongDirection)),
    }
}

/// Supervisor side: only a response or an event is accepted.
pub fn parse_reply(payload: &[u8]) -> Result<Reply, ProtocolViolation> {
    match parse_message(payload)? {
        Message::Response(response) => Ok(Reply::Response(response)),
        Message::Event(event) => Ok(Reply::Event(event)),
        Message::Request(_) => Err(violation(ViolationKind::WrongDirection)),
    }
}

/// The payload of one frame. A message that the parser would refuse is not produced.
pub fn encode_message(message: &Message) -> Result<Vec<u8>, ProtocolViolation> {
    check(message)?;
    serde_json::to_vec(message).map_err(|_| violation(ViolationKind::Schema))
}
