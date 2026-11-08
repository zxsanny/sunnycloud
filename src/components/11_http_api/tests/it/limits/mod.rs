//! Body limits and time limits per route class (E-21/c05, IT-11-13): Mode S over real TCP, with one route per
//! class whose handler reads its body and then calls a stub port.

mod declared;
mod registry;
mod streaming;
pub mod support;
mod timers;
