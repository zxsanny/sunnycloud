//! AC-7: the parser survives fuzzing. `cargo fuzz` needs nightly, so the same property runs
//! here on every `cargo test`: a seeded mutation fuzz over the seed corpus in `tests/corpus/`
//! (one valid payload per message kind, written from the samples, plus the hostile cases).
//! The cargo-fuzz harness in `fuzz/` uses the same corpus directory.
//!
//! Properties: the parser and the frame reader never panic; a payload the parser accepts
//! re-encodes and parses back to an equal message; the reader never buffers above 64 KiB.
//!
//! `UPDATE_CORPUS=1 cargo test -p media-shared --test fuzz_parser` rewrites the `valid_*` files.

mod common;

use common::*;
use media_shared::decoder_protocol::{
    FrameReader, MAX_PAYLOAD, Message, Polled, encode_message, parse_message,
};
use serde_json::Value;
use std::fs;
use std::io::{self, Read};
use std::path::PathBuf;

const MUTATIONS_PER_SEED: usize = 600;
const SEED: u64 = 0x5eed_dec0_de01_u64;

fn corpus_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/corpus")
}

fn corpus() -> Vec<(String, Vec<u8>)> {
    let mut files: Vec<_> = fs::read_dir(corpus_dir())
        .expect("corpus directory")
        .map(|entry| entry.expect("entry").path())
        .map(|path| {
            let name = path
                .file_name()
                .expect("name")
                .to_string_lossy()
                .into_owned();
            (name, fs::read(&path).expect("read"))
        })
        .collect();
    files.sort();
    files
}

/// `valid_<role>_<variant>_<index>.json`; the index keeps variants with several samples apart.
fn valid_names() -> Vec<(String, Message)> {
    messages()
        .into_iter()
        .enumerate()
        .map(|(index, message)| {
            let value = serde_json::to_value(&message).expect("json");
            let (role, body) = value
                .as_object()
                .and_then(|o| o.iter().next())
                .expect("role");
            let variant = match body {
                Value::String(name) => name.clone(),
                Value::Object(map) => map.keys().next().cloned().expect("variant"),
                other => panic!("unexpected body {other}"),
            };
            (format!("valid_{role}_{variant}_{index:02}.json"), message)
        })
        .collect()
}

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, bound: usize) -> usize {
        usize::try_from(self.next() % u64::try_from(bound.max(1)).expect("fits")).expect("fits")
    }

    fn byte(&mut self) -> u8 {
        self.next().to_le_bytes()[0]
    }
}

const DICTIONARY: [&[u8]; 10] = [
    b"{",
    b"}",
    b"[",
    b"\"",
    b"-1",
    b"4294967295",
    b"1e999",
    b"null",
    b"\\u0000",
    b"\xff",
];

fn mutate(seed: &[u8], pool: &[(String, Vec<u8>)], rng: &mut Rng) -> Vec<u8> {
    let mut bytes = seed.to_vec();
    for _ in 0..=rng.below(4) {
        let at = rng.below(bytes.len() + 1);
        match rng.below(7) {
            0 if !bytes.is_empty() => {
                let i = at.min(bytes.len() - 1);
                bytes[i] ^= 1 << rng.below(8);
            }
            1 if !bytes.is_empty() => {
                let i = at.min(bytes.len() - 1);
                bytes[i] = rng.byte();
            }
            2 => bytes.insert(at, rng.byte()),
            3 if !bytes.is_empty() => {
                bytes.remove(at.min(bytes.len() - 1));
            }
            4 => bytes.truncate(at),
            5 => {
                let token = DICTIONARY[rng.below(DICTIONARY.len())];
                bytes.splice(at..at, token.iter().copied());
            }
            _ => {
                let other = &pool[rng.below(pool.len())].1;
                let from = rng.below(other.len() + 1);
                let to = (from + rng.below(32)).min(other.len());
                bytes.splice(at..at, other[from..to].iter().copied());
            }
        }
    }
    bytes
}

/// Parses; an accepted payload must survive a re-encode.
fn check_payload(payload: &[u8]) -> bool {
    match parse_message(payload) {
        Ok(message) => {
            let encoded = encode_message(&message).expect("an accepted message encodes");
            assert_eq!(
                parse_message(&encoded),
                Ok(message),
                "re-parse of an accepted payload"
            );
            true
        }
        Err(_) => false,
    }
}

#[test]
fn the_seed_corpus_matches_the_samples_and_the_hostile_cases() {
    if std::env::var_os("UPDATE_CORPUS").is_some() {
        for (name, message) in valid_names() {
            let payload = encode_message(&message).expect("encode");
            fs::write(corpus_dir().join(name), payload).expect("write corpus file");
        }
    }
    let files = corpus();
    for (name, message) in valid_names() {
        let (_, bytes) = files
            .iter()
            .find(|(n, _)| *n == name)
            .unwrap_or_else(|| panic!("{name} is missing: run with UPDATE_CORPUS=1"));
        assert_eq!(parse_message(bytes), Ok(message), "{name}");
    }
    let hostile: Vec<_> = files
        .iter()
        .filter(|(n, _)| n.starts_with("hostile_"))
        .collect();
    assert!(hostile.len() >= 15);
    for (name, bytes) in hostile {
        assert!(parse_message(bytes).is_err(), "{name} is accepted");
    }
    let valid = files
        .iter()
        .filter(|(n, _)| n.starts_with("valid_"))
        .count();
    assert_eq!(
        valid,
        valid_names().len(),
        "a stale valid_* file is in the corpus"
    );
}

#[test]
fn the_parser_survives_mutations_of_every_seed() {
    let pool = corpus();
    let mut rng = Rng(SEED);
    let (mut accepted, mut refused) = (0usize, 0usize);

    for (_, seed) in &pool {
        for _ in 0..MUTATIONS_PER_SEED {
            let payload = mutate(seed, &pool, &mut rng);
            if check_payload(&payload) {
                accepted += 1;
            } else {
                refused += 1;
            }
        }
    }

    assert!(
        accepted > 100,
        "mutations never reach the range checks: {accepted} accepted"
    );
    assert!(refused > 1000, "mutations never fail: {refused} refused");
}

#[test]
fn the_parser_survives_random_and_oversized_payloads() {
    let mut rng = Rng(SEED ^ 0xa5a5);
    for length in [0usize, 1, 2, 7, 64, 1000, MAX_PAYLOAD, MAX_PAYLOAD + 1] {
        let random: Vec<u8> = (0..length).map(|_| rng.byte()).collect();
        assert!(!check_payload(&random), "random bytes of length {length}");
        let text: Vec<u8> = (0..length)
            .map(|_| b" {}[]\":,0123456789-".as_slice()[rng.below(19)])
            .collect();
        check_payload(&text);
    }
    for opener in [
        "[",
        "{\"request\":",
        "{\"request\":{\"probe\":{\"job_dir\":",
    ] {
        let deep = opener.repeat(MAX_PAYLOAD / opener.len());
        assert!(!check_payload(deep.as_bytes()), "nesting of {opener}");
    }
}

/// Hands out the stream in random chunks, with random `WouldBlock` between them.
struct Chunked<'a> {
    data: &'a [u8],
    rng: Rng,
}

impl Read for Chunked<'_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.rng.below(5) == 0 {
            return Err(io::ErrorKind::WouldBlock.into());
        }
        let n = self.data.len().min(buf.len()).min(1 + self.rng.below(97));
        buf[..n].copy_from_slice(&self.data[..n]);
        self.data = &self.data[n..];
        Ok(n)
    }
}

#[test]
fn the_frame_reader_survives_mutated_streams_and_stays_bounded() {
    let pool = corpus();
    let mut rng = Rng(SEED ^ 0x1234);
    let mut messages_read = 0usize;

    for (_, seed) in &pool {
        for round in 0..40 {
            let payload = mutate(seed, &pool, &mut rng);
            let mut stream = match round % 4 {
                0 => frame(&payload),
                1 => {
                    let mut wire = frame(&payload);
                    wire.truncate(rng.below(wire.len() + 1));
                    wire
                }
                2 => {
                    let mut wire = Vec::from(rng.next().to_be_bytes());
                    wire.truncate(4);
                    wire.extend_from_slice(&payload);
                    wire
                }
                _ => {
                    let mut wire = frame(&payload);
                    wire.extend_from_slice(&frame(seed));
                    wire
                }
            };
            stream.extend_from_slice(&[0, 0]);
            let mut source = Chunked {
                data: &stream,
                rng: Rng(rng.next()),
            };
            let mut reader = FrameReader::new();

            for _ in 0..4000 {
                let polled = reader.poll_reply(&mut source);
                assert!(reader.buffered_capacity() <= MAX_PAYLOAD);
                match polled {
                    Ok(Polled::Ready(_)) => messages_read += 1,
                    Ok(Polled::Pending) => {}
                    Ok(Polled::Closed) | Err(_) => break,
                }
            }
        }
    }

    assert!(messages_read > 0, "no stream ever produced a reply");
}
