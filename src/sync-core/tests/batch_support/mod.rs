#![allow(dead_code, unused_imports)]
//! Shared pieces of the small-upload tests: in-memory files, a stateful fake account (staged rows, the staged
//! cap, commit receipts) served through the scripted fixture, and one timeline that orders what the fake, the
//! checkpoint store and the host sink saw.

mod fake;
mod host;

pub use fake::{Fake, FakeConfig};
pub use host::{Gate, MemSource, RecordingSink, SpyStore, files, resolver, restart};

use std::sync::{Arc, Mutex, PoisonError};

use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    /// The fake received a staged `PUT` for this key.
    Stage(Uuid),
    /// The fake received a `commits:batch` request with this id.
    Commit(Uuid),
    Save(Uuid),
    Clear(Uuid),
    /// The host sink got the results of this batch.
    Report(Uuid),
}

#[derive(Clone, Default)]
pub struct Timeline(Arc<Mutex<Vec<Event>>>);

impl Timeline {
    pub fn push(&self, event: Event) {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(event);
    }

    pub fn events(&self) -> Vec<Event> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    pub fn position(&self, wanted: &Event) -> Option<usize> {
        self.events().iter().position(|event| event == wanted)
    }
}

pub fn id(n: u128) -> Uuid {
    Uuid::from_u128(n)
}

/// A fake account behind a fixture, and a client connected to it.
pub async fn start(
    config: FakeConfig,
    timeline: &Timeline,
) -> (
    Arc<Fake>,
    crate::support::Fixture,
    sync_core::HttpSyncClient,
) {
    let fake = Fake::new(config, timeline);
    let fixture = crate::support::Fixture::start(fake.script()).await;
    let client = restart(&fixture).await;
    (fake, fixture, client)
}
