//! Test tooling for the settings window (E-25, AZ-3888): a stub daemon at the JSON-RPC
//! boundary and a UI driver that acts through the OS accessibility tree.
//!
//! Dev-only. `sunnycloud-settings` never depends on this crate, and nothing here is linked
//! into a shipped binary.

pub mod client;
pub mod driver;
pub mod script;
pub mod stub;

mod state;
