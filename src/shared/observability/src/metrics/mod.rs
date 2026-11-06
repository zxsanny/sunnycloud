//! The metrics registry: `sunnycloud_`-prefixed, enumeration-only labels, one histogram bucket set.

mod collectors;
mod family;
mod label;
mod snapshot;

use std::sync::{Arc, Mutex, PoisonError};

use prometheus::core::Collector;
use prometheus::{Encoder, Registry, TextEncoder};

pub use collectors::{DbCollector, audit_collector, db_collector, ratelimit_collector};
pub use family::{Counter, Gauge, HISTOGRAM_BUCKETS, Histogram};
pub use label::{LabelSet, MAX_LABELS, NoLabels};
pub use snapshot::SnapshotCollector;

use crate::error::ObsError;
use crate::health::CheckFuture;

/// Prefix of every exported name. Constructors take names without it.
pub const PREFIX: &str = "sunnycloud";

/// Runs before each scrape to refresh state that needs an async call (`db_up`).
pub trait ScrapeProbe: Send + Sync {
    fn refresh(&self) -> CheckFuture<'_, ()>;
}

/// Values of `build_info{version,commit,test_clock}`; `test_clock` is true in the `-test` images.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BuildInfo {
    pub version: &'static str,
    pub commit: &'static str,
    pub test_clock: bool,
}

#[derive(Clone, Copy)]
struct BuildLabels(BuildInfo);

impl LabelSet for BuildLabels {
    const NAMES: &'static [&'static str] = &["version", "commit", "test_clock"];

    fn value(&self, index: usize) -> &'static str {
        match index {
            0 => self.0.version,
            1 => self.0.commit,
            _ if self.0.test_clock => "true",
            _ => "false",
        }
    }
}

/// One registry per process. Every family registered through it is exported as `sunnycloud_<name>`.
pub struct Metrics {
    registry: Registry,
    probes: Mutex<Vec<Arc<dyn ScrapeProbe>>>,
}

impl Metrics {
    /// A registry with `build_info` and, on Linux, the standard process collector.
    pub fn new(build: BuildInfo) -> Result<Metrics, ObsError> {
        let registry = Registry::new_custom(Some(PREFIX.to_owned()), None)
            .map_err(|error| ObsError::Registry(error.to_string()))?;
        let metrics = Metrics {
            registry,
            probes: Mutex::new(Vec::new()),
        };
        let build_info = metrics.gauge::<BuildLabels>("build_info")?;
        build_info.set(BuildLabels(build), 1.0);
        #[cfg(target_os = "linux")]
        metrics.register_collector(Box::new(
            prometheus::process_collector::ProcessCollector::for_self(),
        ))?;
        Ok(metrics)
    }

    fn check_name(name: &str) -> Result<(), ObsError> {
        let prefixed = name
            .strip_prefix(PREFIX)
            .is_some_and(|rest| rest.starts_with('_'));
        if label::is_identifier(name) && !prefixed {
            Ok(())
        } else {
            Err(ObsError::BadName(name.to_owned()))
        }
    }

    fn add(&self, name: &str, collector: Box<dyn Collector>) -> Result<(), ObsError> {
        self.registry
            .register(collector)
            .map_err(|error| family::registry_error(name, &error))
    }

    /// `name` has no prefix. `BadLabel` when `L` names an unbounded label, `Duplicate` when the name is taken.
    pub fn counter<L: LabelSet>(&self, name: &'static str) -> Result<Counter<L>, ObsError> {
        Self::check_name(name)?;
        let counter = Counter::<L>::new(name)?;
        self.add(name, counter.collector())?;
        Ok(counter)
    }

    pub fn gauge<L: LabelSet>(&self, name: &'static str) -> Result<Gauge<L>, ObsError> {
        Self::check_name(name)?;
        let gauge = Gauge::<L>::new(name)?;
        self.add(name, gauge.collector())?;
        Ok(gauge)
    }

    pub fn histogram<L: LabelSet>(&self, name: &'static str) -> Result<Histogram<L>, ObsError> {
        Self::check_name(name)?;
        let histogram = Histogram::<L>::new(name)?;
        self.add(name, histogram.collector())?;
        Ok(histogram)
    }

    /// `Duplicate` when a family the collector describes is already registered.
    pub fn register_collector(&self, collector: Box<dyn Collector>) -> Result<(), ObsError> {
        self.registry
            .register(collector)
            .map_err(|error| family::registry_error("collector", &error))
    }

    pub fn register_probe(&self, probe: Arc<dyn ScrapeProbe>) {
        self.probes
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(probe);
    }

    /// Runs the probes, then renders every family in the Prometheus text format.
    pub async fn render(&self) -> Result<String, ObsError> {
        let probes: Vec<_> = self
            .probes
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        for probe in probes {
            probe.refresh().await;
        }
        let mut out = Vec::new();
        TextEncoder::new()
            .encode(&self.registry.gather(), &mut out)
            .map_err(|error| ObsError::Encode(error.to_string()))?;
        String::from_utf8(out).map_err(|error| ObsError::Encode(error.to_string()))
    }
}
