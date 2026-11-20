//! The realtime channel (`realtime.md`) over real TCP against a scripted change feed: the handshake rules, the
//! frames, the close codes and the timing limits. The real feed on PostgreSQL is `pipeline_realtime` of the
//! server crate.

mod handshake;
mod limits;
mod session;
mod support;
