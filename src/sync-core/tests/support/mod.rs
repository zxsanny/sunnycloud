#![allow(dead_code, unused_imports)]
//! The scripted fake HTTP server of AZ-3970. Test-only: it lives in `tests/`, its dependencies are
//! dev-dependencies, and its CA is generated at runtime, so no release build can contain either.

mod answer;
mod body;
mod client;
mod fixture;
mod log;
mod render;
mod route;
mod script;
mod server;
mod tls;

pub use answer::{Answer, Fault};
pub use client::{Client, Failure, Reply};
pub use fixture::{Fixture, FixtureBuilder};
pub use log::{AppliedFault, ConnInfo, LoggedRequest};
pub use route::Route;
pub use script::{Script, seeded_bytes};
pub use tls::{TlsMode, client_config};
