//! Plain data types shared by the API side (08), the worker supervisor (09) and the untrusted
//! decoder (10): worker job DTOs, transcode policy types and the decoder protocol v1 messages.
//!
//! Types only (ADR-009): no sqlx, no I/O, no queue, no behaviour. The policy functions, the
//! `Params` values, the socket framing and the numeric range checks belong to the epics that
//! consume these types.

pub mod decoder_protocol;
pub mod policy;
pub mod types;
