//! A recording `Scrubber` that answers scripted steps, each after a delay, for the `scrub run` tests.

#![allow(dead_code)]

use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use async_trait::async_trait;
use contracts::{ByteRate, ScrubError, ScrubMode, ScrubRun, ScrubStatus, Scrubber, StepResult};
use sc_clock::Timestamp;

pub fn step(checked: u64, mismatched: u64, missing: u64, done: bool) -> StepResult {
    StepResult {
        checked,
        mismatched,
        missing,
        done,
    }
}

#[derive(Default)]
pub struct Recording {
    pub starts: Vec<(ScrubMode, Option<ByteRate>)>,
    pub steps: usize,
    pub finishes: usize,
}

pub struct Scripted {
    script: Mutex<Vec<StepResult>>,
    delay: Duration,
    pub status: ScrubStatus,
    pub recording: Mutex<Recording>,
    on_step: Option<Box<dyn Fn() + Send + Sync>>,
}

impl Scripted {
    pub fn new(script: Vec<StepResult>) -> Self {
        Self {
            script: Mutex::new(script.into_iter().rev().collect()),
            delay: Duration::ZERO,
            status: ScrubStatus {
                oldest_scrubbed_at: None,
                unscrubbed: 0,
                latest_summary: None,
                unavailable: 0,
            },
            recording: Mutex::default(),
            on_step: None,
        }
    }

    pub fn delay(mut self, delay: Duration) -> Self {
        self.delay = delay;
        self
    }

    pub fn status(mut self, status: ScrubStatus) -> Self {
        self.status = status;
        self
    }

    /// Called when a step starts, before its delay.
    pub fn on_step(mut self, hook: impl Fn() + Send + Sync + 'static) -> Self {
        self.on_step = Some(Box::new(hook));
        self
    }

    pub fn calls(&self) -> (Vec<(ScrubMode, Option<ByteRate>)>, usize, usize) {
        let recording = self
            .recording
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        (
            recording.starts.clone(),
            recording.steps,
            recording.finishes,
        )
    }

    fn record(&self, change: impl FnOnce(&mut Recording)) {
        change(
            &mut self
                .recording
                .lock()
                .unwrap_or_else(PoisonError::into_inner),
        );
    }
}

#[async_trait]
impl Scrubber for Scripted {
    async fn start_run(
        &self,
        mode: ScrubMode,
        rate: Option<ByteRate>,
    ) -> Result<ScrubRun, ScrubError> {
        self.record(|recording| recording.starts.push((mode, rate)));
        Ok(ScrubRun {
            started_at: Timestamp::from_unix_micros(0),
        })
    }

    async fn run_step(&self, _run: &ScrubRun) -> Result<StepResult, ScrubError> {
        self.record(|recording| recording.steps += 1);
        if let Some(hook) = &self.on_step {
            hook();
        }
        tokio::time::sleep(self.delay).await;
        let next = self
            .script
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .pop();
        Ok(next.unwrap_or_else(|| step(0, 0, 0, true)))
    }

    async fn finish_run(&self, _run: &ScrubRun) -> Result<(), ScrubError> {
        self.record(|recording| recording.finishes += 1);
        Ok(())
    }

    async fn status(&self) -> Result<ScrubStatus, ScrubError> {
        Ok(self.status)
    }
}

/// The scrubber as the service the real handler asks for.
pub fn as_service(scrubber: &Arc<Scripted>) -> Arc<dyn Scrubber> {
    Arc::clone(scrubber) as Arc<dyn Scrubber>
}
