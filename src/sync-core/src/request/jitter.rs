use std::time::Duration;

/// Jitter never reaches one second (`sync_api.md`, rate limits).
const CEILING: Duration = Duration::from_secs(1);

/// A random delay in `[0, min(bound, 1 s))`. When the system has no randomness the delay is zero and
/// the failure is logged: a retry without jitter is still a correct retry.
pub(crate) fn jitter(bound: Duration) -> Duration {
    let bound = bound.min(CEILING);
    let mut bytes = [0u8; 4];
    match getrandom::fill(&mut bytes) {
        Ok(()) => bound.mul_f64(f64::from(u32::from_le_bytes(bytes)) / 4_294_967_296.0),
        Err(error) => {
            tracing::warn!(%error, "no randomness for retry jitter, retrying without it");
            Duration::ZERO
        }
    }
}
