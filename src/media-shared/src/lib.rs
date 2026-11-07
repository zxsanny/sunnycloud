//! Plain data types shared by the API side (08), the worker supervisor (09) and the untrusted
//! decoder (10): worker job DTOs, transcode policy types and the decoder protocol v1 messages.
//!
//! No sqlx, no I/O, no queue (ADR-009). The pure policy functions, the job-kind table and the
//! `Params` values live here.
//! `decoder_protocol` also holds the synchronous frame codec, the strict parser and the
//! session rules of the decoder socket, working on byte streams it is given.

pub mod decoder_protocol;
pub mod policy;
pub mod types;
