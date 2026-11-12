//! The test-only trigger of the self-abort watchdog (10 section 3 and 13.6): `SIGUSR1` makes
//! the next pass of the main loop block for good while the watchdog thread keeps running.
//! The whole module is compiled only with the `test-freeze` feature.

use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;

use sc_observability::log_error;

use crate::error::RunnerError;
use crate::sys;

static REQUESTED: AtomicBool = AtomicBool::new(false);

/// Only stores to an atomic, so it is async-signal-safe and may run on any thread: the freeze
/// itself happens in [`block_if_requested`], on the main loop's own thread.
extern "C" fn request(_signal: i32) {
    REQUESTED.store(true, Ordering::Release);
}

pub fn install() -> Result<(), RunnerError> {
    sys::on_sigusr1(request).map_err(RunnerError::io("install the test freeze handler"))
}

/// Called where the main loop bumps the watchdog counter.
pub fn block_if_requested() {
    if REQUESTED.load(Ordering::Acquire) {
        log_error!("test_freeze_main_loop: main loop blocked");
        loop {
            thread::park();
        }
    }
}
