//! Client-side cryptography of the end-to-end encrypted vault (component 19, v1.2). A pure library: no
//! sunnycloud crate, no I/O, no network, no logging and no global state; randomness is injected through
//! [`RandomSource`]. The same code builds natively, for `wasm32-unknown-unknown` and through UniFFI.
#![no_std]

extern crate alloc;
// UniFFI's generated scaffolding and the test harness need std; the core stays no_std.
#[cfg(any(test, feature = "uniffi"))]
extern crate std;

pub mod constants;
mod error;
#[cfg(feature = "os-rng")]
mod os_random;
mod random;

pub use error::{ParamField, VaultError};
#[cfg(feature = "os-rng")]
pub use os_random::OsRandom;
pub use random::RandomSource;

#[cfg(feature = "uniffi")]
uniffi::setup_scaffolding!("vault_core");

#[cfg(test)]
mod tests;
