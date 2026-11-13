//! The system trust store on Android (`uniffi_surface.md` platform verifier, 13 risk 4). rustls-platform-verifier
//! reaches Android's verifier through the JVM, which it learns only from an init call the app makes once per
//! process; a TLS handshake before that panics inside the verifier. `connect` therefore refuses to start
//! with `TrustStoreUninitialized` until [`init_once`] recorded a successful init. Other platforms need no init.

use std::sync::atomic::{AtomicBool, Ordering};

use crate::client::ConnectError;

/// Set once the verifier's init succeeded in this process; never cleared.
static READY: AtomicBool = AtomicBool::new(false);

/// Called by `connect` before any TLS setup.
pub(crate) fn ensure_ready() -> Result<(), ConnectError> {
    gate(cfg!(target_os = "android"), READY.load(Ordering::Acquire))
}

fn gate(needs_init: bool, ready: bool) -> Result<(), ConnectError> {
    if needs_init && !ready {
        return Err(ConnectError::TrustStoreUninitialized);
    }
    Ok(())
}

/// The process-wide flag the Android JNI entry point records into.
#[cfg(all(target_os = "android", feature = "uniffi"))]
pub(crate) fn process_flag() -> &'static AtomicBool {
    &READY
}

/// Runs `init` unless `ready` is already set and sets it when `init` succeeds; a second call is a no-op.
/// Returns whether the store is ready afterwards.
#[cfg(any(test, all(target_os = "android", feature = "uniffi")))]
pub(crate) fn init_once(ready: &AtomicBool, init: impl FnOnce() -> bool) -> bool {
    if ready.load(Ordering::Acquire) {
        return true;
    }
    let done = init();
    if done {
        ready.store(true, Ordering::Release);
    }
    done
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::sync::atomic::{AtomicBool, Ordering};

    use super::{gate, init_once};
    use crate::client::ConnectError;

    #[test]
    fn gate_refuses_an_uninitialised_store_only_where_init_is_needed() {
        // Arrange
        let cases = [(true, false), (true, true), (false, false), (false, true)];

        // Act
        let answers: Vec<_> = cases
            .iter()
            .map(|(needs_init, ready)| gate(*needs_init, *ready))
            .collect();

        // Assert
        assert_eq!(
            answers,
            vec![
                Err(ConnectError::TrustStoreUninitialized),
                Ok(()),
                Ok(()),
                Ok(())
            ]
        );
    }

    #[test]
    fn init_once_runs_the_init_once_and_records_success() {
        // Arrange
        let ready = AtomicBool::new(false);
        let calls = Cell::new(0);
        let init = || {
            calls.set(calls.get() + 1);
            true
        };

        // Act
        let first = init_once(&ready, init);
        let second = init_once(&ready, init);

        // Assert
        assert!(first && second);
        assert_eq!(calls.get(), 1);
        assert!(ready.load(Ordering::Acquire));
    }

    #[test]
    fn a_failed_init_leaves_the_store_unready_and_is_tried_again() {
        // Arrange
        let ready = AtomicBool::new(false);
        let calls = Cell::new(0);

        // Act
        let failed = init_once(&ready, || {
            calls.set(calls.get() + 1);
            false
        });
        let gate_after_failure = gate(true, ready.load(Ordering::Acquire));
        let retried = init_once(&ready, || {
            calls.set(calls.get() + 1);
            true
        });

        // Assert
        assert!(!failed);
        assert_eq!(
            gate_after_failure,
            Err(ConnectError::TrustStoreUninitialized)
        );
        assert!(retried);
        assert_eq!(calls.get(), 2);
    }
}
