//! AZ-3795 AC-4: `admin dev-seed` through the real handler with a recording `DevSeed`.

mod support;

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use contracts::{DevSeed, DevSeedError};
use serde_json::Value;
use support::{Captured, Harness, locked};

struct Recorder {
    calls: Mutex<u32>,
    answer: fn() -> Result<(), DevSeedError>,
}

#[async_trait]
impl DevSeed for Recorder {
    async fn seed(&self) -> Result<(), DevSeedError> {
        *locked(&self.calls) += 1;
        (self.answer)()
    }
}

fn run(answer: fn() -> Result<(), DevSeedError>, args: &[&str]) -> (Captured, Arc<Recorder>) {
    let recorder = Arc::new(Recorder {
        calls: Mutex::new(0),
        answer,
    });
    let served = Arc::clone(&recorder);
    let captured = Harness::new()
        .real_handler()
        .service::<dyn DevSeed, _, _>(move |_| {
            let seed: Arc<dyn DevSeed> = served.clone();
            async move { Ok(seed) }
        })
        .run(args);
    (captured, recorder)
}

#[test]
fn a_development_run_makes_one_seed_call_and_exits_0() {
    // Arrange and Act
    let (result, recorder) = run(|| Ok(()), &["--json", "admin", "dev-seed"]);

    // Assert
    assert_eq!(result.exit, 0, "{}", result.stderr);
    assert_eq!(*locked(&recorder.calls), 1);
    let envelope: Value = serde_json::from_str(&result.stdout).expect("one JSON object");
    assert_eq!(envelope["data"]["seeded"], Value::Bool(true));
}

#[test]
fn the_production_refusal_exits_2_and_says_so() {
    // Arrange and Act
    let (result, recorder) = run(
        || Err(DevSeedError::RefusedInProduction),
        &["admin", "dev-seed"],
    );

    // Assert
    assert_eq!(result.exit, 2);
    assert!(result.stderr.contains("production"), "{}", result.stderr);
    assert_eq!(result.stdout, "");
    assert_eq!(*locked(&recorder.calls), 1);
}

#[test]
fn an_unexpected_failure_exits_2_with_the_reason() {
    // Arrange and Act
    let (result, _recorder) = run(
        || {
            Err(DevSeedError::Unexpected(
                "the identity provider is down".into(),
            ))
        },
        &["admin", "dev-seed"],
    );

    // Assert
    assert_eq!(result.exit, 2);
    assert!(
        result.stderr.contains("the identity provider is down"),
        "{}",
        result.stderr
    );
}
