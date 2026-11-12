//! The time bound of every keyring call (14 §5 rule 2, 15 risk 2).

use std::sync::mpsc;
use std::time::{Duration, Instant};

use crate::auth::token_store::run_bounded;

#[test]
fn a_call_that_never_answers_is_abandoned_after_its_bound() {
    // Arrange
    let bound = Duration::from_millis(200);
    let (_keep_open, never) = mpsc::channel::<()>();
    let started = Instant::now();

    // Act
    let answer = run_bounded("test-hang", bound, move || never.recv().is_ok());
    let took = started.elapsed();

    // Assert
    assert!(answer.is_err());
    assert!(took >= bound, "{took:?}");
    assert!(took < bound + Duration::from_secs(2), "{took:?}");
}

#[test]
fn a_call_within_its_bound_returns_its_value() {
    // Arrange
    let bound = Duration::from_secs(5);

    // Act
    let answer = run_bounded("test-quick", bound, || 7);

    // Assert
    assert_eq!(answer, Ok(7));
}
