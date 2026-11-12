use std::sync::mpsc::{RecvTimeoutError, sync_channel};
use std::thread;
use std::time::Duration;

/// The work did not answer within its bound, or its worker could not run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TimedOut;

/// Runs `work` on its own detached thread and waits at most `bound`. keyring 4.2.0 has no call timeout and an
/// unanswered unlock prompt blocks forever (14 §5), so a worker past its bound is abandoned, never joined.
pub(crate) fn run_bounded<T, F>(name: &'static str, bound: Duration, work: F) -> Result<T, TimedOut>
where
    T: Send + 'static,
    F: FnOnce() -> T + Send + 'static,
{
    let (done, answer) = sync_channel(1);
    let spawned = thread::Builder::new()
        .name(format!("token-store-{name}"))
        .spawn(move || {
            if done.send(work()).is_err() {
                tracing::warn!(
                    component = "token_store",
                    call = name,
                    "token store call finished after its bound; its result was dropped"
                );
            }
        });
    if let Err(error) = spawned {
        tracing::error!(component = "token_store", call = name, %error, "token store worker did not start");
        return Err(TimedOut);
    }
    match answer.recv_timeout(bound) {
        Ok(value) => Ok(value),
        Err(RecvTimeoutError::Timeout) => {
            tracing::warn!(
                component = "token_store",
                call = name,
                bound_ms = u64::try_from(bound.as_millis()).unwrap_or(u64::MAX),
                "token store call exceeded its bound and was abandoned"
            );
            Err(TimedOut)
        }
        Err(RecvTimeoutError::Disconnected) => {
            tracing::error!(
                component = "token_store",
                call = name,
                "token store worker panicked"
            );
            Err(TimedOut)
        }
    }
}
