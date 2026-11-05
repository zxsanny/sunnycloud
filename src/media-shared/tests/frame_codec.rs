//! AC-1, AC-2, AC-8: the frame codec. Prefix byte order, the 64 KiB cap, partial frames.

mod common;

use common::*;
use media_shared::decoder_protocol::{
    FrameReader, MAX_PAYLOAD, Message, Polled, ReadError, Reply, Request, ViolationKind,
    WriteError, parse_message, write_message,
};
use std::collections::VecDeque;
use std::io::{self, Cursor, Read};

enum Step {
    Bytes(Vec<u8>),
    WouldBlock,
    Interrupted,
    Eof,
    Fail(io::ErrorKind),
}

/// A source that plays a script; panics when read after the script ended.
struct Script(VecDeque<Step>);

impl Read for Script {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self.0.pop_front().expect("read past the end of the script") {
            Step::Bytes(mut bytes) => {
                let n = bytes.len().min(buf.len());
                buf[..n].copy_from_slice(&bytes[..n]);
                if n < bytes.len() {
                    self.0.push_front(Step::Bytes(bytes.split_off(n)));
                }
                Ok(n)
            }
            Step::WouldBlock => Err(io::ErrorKind::WouldBlock.into()),
            Step::Interrupted => Err(io::ErrorKind::Interrupted.into()),
            Step::Eof => Ok(0),
            Step::Fail(kind) => Err(kind.into()),
        }
    }
}

fn script(steps: Vec<Step>) -> Script {
    Script(steps.into())
}

/// Yields the 4 prefix bytes, then panics on any further read.
struct PrefixThenPanic(Cursor<Vec<u8>>);

impl Read for PrefixThenPanic {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        assert!(
            self.0.position() < 4,
            "a payload byte was read after the prefix"
        );
        self.0.read(buf)
    }
}

fn violation_of<T: std::fmt::Debug>(result: Result<T, ReadError>) -> ViolationKind {
    match result {
        Err(ReadError::Violation(violation)) => violation.0,
        other => panic!("expected a violation, got {other:?}"),
    }
}

fn ready_request(polled: Polled<Request>) -> Request {
    match polled {
        Polled::Ready(request) => request,
        other => panic!("expected a request, got {other:?}"),
    }
}

#[test]
fn every_message_round_trips_through_a_frame() {
    let all = messages();
    assert!(all.len() >= 30, "samples cover every variant");
    for message in all {
        let mut wire = Vec::new();
        write_message(&mut wire, &message).expect("write");

        let mut reader = FrameReader::new();
        let mut source = wire.as_slice();
        let parsed = match &message {
            Message::Request(_) => reader
                .poll_request(&mut source)
                .expect("poll")
                .into_message(),
            Message::Response(_) | Message::Event(_) => {
                reader.poll_reply(&mut source).expect("poll").into_message()
            }
        };

        assert_eq!(parsed, Some(message));
        assert!(source.is_empty(), "the whole frame was consumed");
    }
}

trait IntoMessage {
    fn into_message(self) -> Option<Message>;
}

impl IntoMessage for Polled<Request> {
    fn into_message(self) -> Option<Message> {
        match self {
            Polled::Ready(request) => Some(Message::Request(request)),
            _ => None,
        }
    }
}

impl IntoMessage for Polled<Reply> {
    fn into_message(self) -> Option<Message> {
        match self {
            Polled::Ready(reply) => Some(reply.into()),
            _ => None,
        }
    }
}

#[test]
fn the_length_prefix_is_big_endian() {
    let mut wire = Vec::new();

    write_message(&mut wire, &Message::Request(Request::Cancel)).expect("write");

    let payload = br#"{"request":"cancel"}"#;
    assert_eq!(&wire[..4], &[0, 0, 0, 20]);
    assert_eq!(&wire[4..], payload);
    assert_eq!(
        parse_message(payload).expect("parse"),
        Message::Request(Request::Cancel)
    );
}

#[test]
fn a_prefix_longer_than_one_byte_is_read_big_endian() {
    let padding = " ".repeat(300 - br#"{"request":"cancel"}"#.len());
    let payload = format!(r#"{{"request":"cancel"}}{padding}"#);
    let wire = frame(payload.as_bytes());
    assert_eq!(&wire[..4], &[0, 0, 1, 44]);

    let polled = FrameReader::new().poll_request(&mut wire.as_slice());

    assert_eq!(ready_request(polled.expect("poll")), Request::Cancel);
}

#[test]
fn a_payload_of_exactly_64_kib_is_accepted() {
    let mut payload = br#"{"request":"cancel"}"#.to_vec();
    payload.resize(MAX_PAYLOAD, b' ');
    let wire = frame(&payload);

    let polled = FrameReader::new().poll_request(&mut wire.as_slice());

    assert_eq!(ready_request(polled.expect("poll")), Request::Cancel);
}

#[test]
fn a_payload_of_65537_bytes_is_a_violation_before_its_bytes_are_read() {
    let mut payload = br#"{"request":"cancel"}"#.to_vec();
    payload.resize(MAX_PAYLOAD + 1, b' ');
    let mut source = PrefixThenPanic(Cursor::new(frame(&payload)));
    let mut reader = FrameReader::new();

    let result = reader.poll_request(&mut source);

    assert_eq!(violation_of(result), ViolationKind::Oversize);
    assert_eq!(source.0.position(), 4);
    assert_eq!(reader.buffered_capacity(), 0);
}

#[test]
fn a_prefix_of_4_gib_is_a_violation_before_any_payload_read() {
    let mut source = PrefixThenPanic(Cursor::new(u32::MAX.to_be_bytes().to_vec()));
    let mut reader = FrameReader::new();

    let result = reader.poll_reply(&mut source);

    assert_eq!(violation_of(result), ViolationKind::Oversize);
    assert_eq!(source.0.position(), 4);
    assert!(reader.buffered_capacity() <= MAX_PAYLOAD);
}

#[test]
fn a_reader_stays_failed_after_a_violation() {
    let mut reader = FrameReader::new();
    let mut hostile = PrefixThenPanic(Cursor::new(u32::MAX.to_be_bytes().to_vec()));
    assert_eq!(
        violation_of(reader.poll_request(&mut hostile)),
        ViolationKind::Oversize
    );
    let valid = frame(br#"{"request":"cancel"}"#);

    let again = reader.poll_request(&mut valid.as_slice());

    assert_eq!(violation_of(again), ViolationKind::Oversize);
}

#[test]
fn a_prefix_split_over_reads_is_assembled() {
    let wire = frame(br#"{"request":"cancel"}"#);
    let mut source = script(vec![
        Step::Bytes(wire[..1].to_vec()),
        Step::Bytes(wire[1..3].to_vec()),
        Step::Bytes(wire[3..].to_vec()),
    ]);

    let polled = FrameReader::new().poll_request(&mut source).expect("poll");

    assert_eq!(ready_request(polled), Request::Cancel);
}

#[test]
fn a_frame_announcing_100_bytes_and_delivering_40_returns_no_message() {
    let mut wire = 100u32.to_be_bytes().to_vec();
    wire.extend_from_slice(&[b' '; 40]);
    let mut source = script(vec![Step::Bytes(wire), Step::WouldBlock, Step::Eof]);
    let mut reader = FrameReader::new();

    let waiting = reader.poll_request(&mut source).expect("poll");
    assert_eq!(waiting, Polled::Pending);
    assert!(reader.buffered_capacity() <= 100);

    let closed = reader.poll_request(&mut source).expect("poll");
    assert_eq!(closed, Polled::Closed);
    assert_eq!(
        reader.buffered_capacity(),
        0,
        "the partial buffer was dropped"
    );
}

#[test]
fn a_partial_frame_completes_when_the_rest_arrives() {
    let wire = frame(br#"{"request":"cancel"}"#);
    let (first, rest) = wire.split_at(9);
    let mut source = script(vec![
        Step::Bytes(first.to_vec()),
        Step::WouldBlock,
        Step::Interrupted,
        Step::Bytes(rest.to_vec()),
    ]);
    let mut reader = FrameReader::new();

    assert_eq!(
        reader.poll_request(&mut source).expect("poll"),
        Polled::Pending
    );
    let done = reader.poll_request(&mut source).expect("poll");

    assert_eq!(ready_request(done), Request::Cancel);
}

#[test]
fn a_close_at_a_frame_boundary_is_not_a_violation() {
    let mut source = script(vec![Step::Eof]);

    let polled = FrameReader::new().poll_reply(&mut source).expect("poll");

    assert_eq!(polled, Polled::Closed);
}

#[test]
fn a_close_inside_the_prefix_returns_no_message() {
    let mut source = script(vec![Step::Bytes(vec![0, 0]), Step::Eof]);

    let polled = FrameReader::new().poll_reply(&mut source).expect("poll");

    assert_eq!(polled, Polled::Closed);
}

#[test]
fn a_read_error_is_an_io_error_not_a_violation() {
    let mut source = script(vec![Step::Fail(io::ErrorKind::ConnectionReset)]);

    let result = FrameReader::new().poll_request(&mut source);

    assert!(matches!(result, Err(ReadError::Io(e)) if e.kind() == io::ErrorKind::ConnectionReset));
}

#[test]
fn the_reader_stops_at_the_end_of_its_frame() {
    let mut stream = frame(br#"{"request":"cancel"}"#);
    let second = frame(br#"{"request":{"hello":{"protocol":1}}}"#);
    stream.extend_from_slice(&second);
    let mut source = stream.as_slice();
    let mut reader = FrameReader::new();

    let first = reader.poll_request(&mut source).expect("poll");
    assert_eq!(
        source.len(),
        second.len(),
        "no byte of the next frame was read"
    );
    let next = reader.poll_request(&mut source).expect("poll");

    assert_eq!(ready_request(first), Request::Cancel);
    assert_eq!(ready_request(next), Request::Hello { protocol: 1 });
}

#[test]
fn an_empty_payload_is_a_violation() {
    let wire = frame(b"");

    let result = FrameReader::new().poll_request(&mut wire.as_slice());

    assert_eq!(violation_of(result), ViolationKind::Json);
}

#[test]
fn a_request_frame_where_a_reply_is_expected_is_a_violation() {
    let wire = frame(br#"{"request":"cancel"}"#);

    let result = FrameReader::new().poll_reply(&mut wire.as_slice());

    assert_eq!(violation_of(result), ViolationKind::WrongDirection);
}

#[test]
fn a_message_the_parser_would_refuse_is_not_written() {
    let mut wire = Vec::new();
    let request = Request::RenderStill {
        job_dir: "/s/j".to_owned(),
        max_edge: 0,
        poster_at_ms: None,
        limits: limits(),
    };

    let result = write_message(&mut wire, &Message::Request(request));

    assert!(
        matches!(result, Err(WriteError::Invalid(v)) if v.0 == ViolationKind::OutOfRange("max_edge"))
    );
    assert!(wire.is_empty());
}

#[test]
fn the_largest_valid_messages_fit_in_one_frame() {
    let request = Message::Request(Request::SelfCheck {
        forbidden_targets: vec!["h".repeat(255); 64],
    });
    let mut wire = Vec::new();

    write_message(&mut wire, &request).expect("write");

    assert!(wire.len() <= MAX_PAYLOAD + 4);
}

#[test]
fn a_write_error_is_an_io_error() {
    struct Broken;
    impl io::Write for Broken {
        fn write(&mut self, _: &[u8]) -> io::Result<usize> {
            Err(io::ErrorKind::BrokenPipe.into())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    let result = write_message(&mut Broken, &Message::Request(Request::Cancel));

    assert!(matches!(result, Err(WriteError::Io(e)) if e.kind() == io::ErrorKind::BrokenPipe));
}
