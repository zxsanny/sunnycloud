//! The supervisor's client of decoder protocol v1 (09 section 3, E-20/c05).
//!
//! The decoder is less trusted than the supervisor, so [`DecoderConn`] treats every reply as hostile and
//! [`Slot`] lets a decoder take jobs only after `hello` (protocol 1) and a `self_check` whose items are all
//! ok, on the first connection and after every reconnect. The job flow (c06) and the slot health and silence
//! reactions (c07) build on these two types.
//!
//! # Decisions
//!
//! - **One metric family per name, labelled `slot`**: a worker has an interactive and a general decoder, and
//!   a family can be registered once. `observability.md` lists both families without labels.
//! - **No timeout inside [`DecoderConn`]**: dropping a call closes the connection, so the caller bounds a call
//!   with `tokio::time::timeout` or a `select!`. The gate bounds its handshake with `decoder_silence_timeout`.
//! - **Reconnect backoff** is a constructor argument ([`Backoff`]): the config table has no key for it.

mod conn;
mod error;
mod metrics;
mod slot;

pub use conn::{DecoderConn, EventSink, PROTOCOL_VERSION};
pub use error::{BackoffError, CallError, GateFailure};
pub use metrics::{DecoderMetrics, SlotName};
pub use slot::{Backoff, GatedConn, Slot, SlotConfig, SlotReadiness, SlotState};
