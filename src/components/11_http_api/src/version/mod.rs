//! Layer 7 of the stack of 11 section 5: the client version gate and the header it reads
//! (`server_info.md`).
//!
//! The gate is applied where the router is built, from the route's auth class, so it sits in front of auth
//! and a route cannot be mounted without it. It reads headers only: a refused request never has its body
//! polled and changes no state.

mod client;
mod gate;
mod minimum;

pub use client::ClientFamily;
pub use gate::{CLIENT_HEADER, VersionGate};
