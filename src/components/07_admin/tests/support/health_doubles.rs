//! Health contributors for the 07 tests. IT-07-6 names test contributors; the real ones belong to other
//! components and are wired by the composition root.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use contracts::{CheckFuture, HealthContributor, HealthItem, HealthStatus};

pub enum Behaviour {
    Answers(HealthStatus, &'static str),
    Sleeps(Duration),
    Panics,
}

pub struct TestContributor {
    name: &'static str,
    behaviour: Behaviour,
    pub calls: Arc<AtomicUsize>,
}

impl TestContributor {
    pub fn answering(name: &'static str, status: HealthStatus, detail: &'static str) -> Arc<Self> {
        Self::with(name, Behaviour::Answers(status, detail))
    }

    pub fn sleeping(name: &'static str, duration: Duration) -> Arc<Self> {
        Self::with(name, Behaviour::Sleeps(duration))
    }

    pub fn panicking(name: &'static str) -> Arc<Self> {
        Self::with(name, Behaviour::Panics)
    }

    fn with(name: &'static str, behaviour: Behaviour) -> Arc<Self> {
        Arc::new(TestContributor {
            name,
            behaviour,
            calls: Arc::new(AtomicUsize::new(0)),
        })
    }
}

impl HealthContributor for TestContributor {
    fn name(&self) -> &str {
        self.name
    }

    fn check(&self) -> CheckFuture<'_, HealthItem> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Box::pin(async move {
            match &self.behaviour {
                Behaviour::Answers(status, detail) => HealthItem::new(*status, *detail),
                Behaviour::Sleeps(duration) => {
                    tokio::time::sleep(*duration).await;
                    HealthItem::ok()
                }
                Behaviour::Panics => panic!("the test contributor fails"),
            }
        })
    }
}

/// The six contributors of IT-07-6 in the order the page lists them: only blob volume is degraded and only
/// dead jobs is down.
pub fn six() -> Vec<Arc<TestContributor>> {
    vec![
        TestContributor::answering("database", HealthStatus::Ok, "reachable"),
        TestContributor::answering(
            "blob volume",
            HealthStatus::Degraded,
            "below the free floor",
        ),
        TestContributor::answering("master key", HealthStatus::Ok, "loaded"),
        TestContributor::answering("scrub", HealthStatus::Ok, "last run clean"),
        TestContributor::answering("dead jobs", HealthStatus::Down, "3 dead jobs"),
        TestContributor::answering("janitor lag", HealthStatus::Ok, "on time"),
    ]
}

pub fn as_ports(list: &[Arc<TestContributor>]) -> Vec<Arc<dyn HealthContributor>> {
    list.iter()
        .map(|item| item.clone() as Arc<dyn HealthContributor>)
        .collect()
}
