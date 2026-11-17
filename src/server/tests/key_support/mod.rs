//! A scripted `MasterKeyring` for the key command tests.

#![allow(dead_code)]

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use contracts::{KeyError, Keyring, MasterKey, MasterKeyring};

use crate::support::locked;

pub fn ring(current: i16, ids: &[i16]) -> Keyring {
    Keyring {
        current_key_id: current,
        key_ids: ids.to_vec(),
    }
}

pub struct Scripted {
    verify: Box<dyn Fn() -> Result<Keyring, KeyError> + Send + Sync>,
    init: Box<dyn Fn() -> Result<(), KeyError> + Send + Sync>,
    batches: Mutex<VecDeque<u64>>,
    delay: Duration,
    on_batch: Option<Box<dyn Fn() + Send + Sync>>,
    pub batch_calls: Mutex<Vec<(i16, i16, u32)>>,
    pub candidates: Mutex<Vec<Vec<u8>>>,
    pub verifies: Mutex<u32>,
}

impl Scripted {
    pub fn new() -> Self {
        Self {
            verify: Box::new(|| Ok(ring(2, &[1, 2]))),
            init: Box::new(|| Ok(())),
            batches: Mutex::new(VecDeque::new()),
            delay: Duration::ZERO,
            on_batch: None,
            batch_calls: Mutex::new(Vec::new()),
            candidates: Mutex::new(Vec::new()),
            verifies: Mutex::new(0),
        }
    }

    pub fn verify(
        mut self,
        answer: impl Fn() -> Result<Keyring, KeyError> + Send + Sync + 'static,
    ) -> Self {
        self.verify = Box::new(answer);
        self
    }

    pub fn init(
        mut self,
        answer: impl Fn() -> Result<(), KeyError> + Send + Sync + 'static,
    ) -> Self {
        self.init = Box::new(answer);
        self
    }

    /// The counts `rewrap_batch` reports, in order; after the last one it reports 0.
    pub fn batches(self, counts: &[u64]) -> Self {
        *locked(&self.batches) = counts.iter().copied().collect();
        self
    }

    pub fn delay(mut self, delay: Duration) -> Self {
        self.delay = delay;
        self
    }

    pub fn on_batch(mut self, hook: impl Fn() + Send + Sync + 'static) -> Self {
        self.on_batch = Some(Box::new(hook));
        self
    }
}

#[async_trait]
impl MasterKeyring for Scripted {
    async fn load_and_verify(&self) -> Result<Keyring, KeyError> {
        *locked(&self.verifies) += 1;
        (self.verify)()
    }

    async fn init_key_check(&self, candidate: &MasterKey) -> Result<(), KeyError> {
        locked(&self.candidates).push(candidate.0.clone());
        (self.init)()
    }

    async fn rewrap_batch(&self, from_id: i16, to_id: i16, n: u32) -> Result<u64, KeyError> {
        locked(&self.batch_calls).push((from_id, to_id, n));
        if let Some(hook) = &self.on_batch {
            hook();
        }
        tokio::time::sleep(self.delay).await;
        Ok(locked(&self.batches).pop_front().unwrap_or(0))
    }
}

pub fn as_service(keyring: &Arc<Scripted>) -> Arc<dyn MasterKeyring> {
    let service: Arc<dyn MasterKeyring> = keyring.clone();
    service
}
