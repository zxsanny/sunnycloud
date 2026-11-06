//! Peers for the decoder client tests: the real runner loop of the `decoder-runner` crate with a scripted
//! handler, and a hostile fake peer that sends raw bytes on a real socket.

#![allow(dead_code, unused_imports)]

mod hostile;
mod runner;

pub use hostile::{AcceptAndClose, HostilePeer, frame, hostile_cases};

use media_shared::decoder_protocol::ViolationKind;

/// Equal kinds; a range violation matches by class because the field name is not part of the contract.
pub fn same_kind(left: ViolationKind, right: ViolationKind) -> bool {
    matches!(
        (left, right),
        (ViolationKind::OutOfRange(_), ViolationKind::OutOfRange(_))
    ) || left == right
}
pub use runner::{Handler, Runner, capabilities, ok_items};

use std::future::Future;
use std::time::{Duration, Instant};

/// Polls `condition` every few milliseconds until it holds or `within` passes.
pub async fn eventually(within: Duration, mut condition: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + within;
    loop {
        if condition() {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}

pub async fn within<T>(limit: Duration, future: impl Future<Output = T>) -> Option<T> {
    tokio::time::timeout(limit, future).await.ok()
}
