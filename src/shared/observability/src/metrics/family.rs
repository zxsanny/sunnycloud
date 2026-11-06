//! Typed handles over the `prometheus` vectors. The only way to name a series is a [`LabelSet`] value.

use std::marker::PhantomData;

use prometheus::{Gauge as PlainGauge, Histogram as PlainHistogram};
use prometheus::{
    GaugeVec, HistogramOpts, HistogramVec, IntCounter, IntCounterVec, Opts, core::Collector,
};

use crate::error::ObsError;
use crate::metrics::label::{self, LabelSet, values};

/// The one bucket set (seconds) every histogram family uses, so dashboards compose. It spans a 1 ms cache
/// hit to a 10 minute transcode.
pub const HISTOGRAM_BUCKETS: [f64; 17] = [
    0.001, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0, 30.0, 60.0, 120.0, 300.0,
    600.0,
];

fn help(name: &str) -> String {
    name.replace('_', " ")
}

fn opts(name: &'static str) -> Opts {
    Opts::new(name, help(name))
}

pub(crate) fn registry_error(name: &str, error: &prometheus::Error) -> ObsError {
    match error {
        prometheus::Error::AlreadyReg => ObsError::Duplicate(name.to_owned()),
        other => ObsError::Registry(other.to_string()),
    }
}

/// Monotonic count. Without labels the increment is one atomic add; with labels it also takes a read lock
/// on the child map.
pub struct Counter<L> {
    vec: IntCounterVec,
    fixed: Option<IntCounter>,
    labels: PhantomData<fn(L)>,
}

impl<L: LabelSet> Counter<L> {
    pub(crate) fn new(name: &'static str) -> Result<Self, ObsError> {
        label::validate::<L>(name)?;
        let vec = IntCounterVec::new(opts(name), L::NAMES)
            .map_err(|error| ObsError::Registry(error.to_string()))?;
        let fixed = L::NAMES
            .is_empty()
            .then(|| vec.with_label_values::<&str>(&[]));
        Ok(Counter {
            vec,
            fixed,
            labels: PhantomData,
        })
    }

    pub(crate) fn collector(&self) -> Box<dyn Collector> {
        Box::new(self.vec.clone())
    }

    pub fn inc(&self, labels: L) {
        self.inc_by(labels, 1);
    }

    pub fn inc_by(&self, labels: L, by: u64) {
        match &self.fixed {
            Some(counter) => counter.inc_by(by),
            None => self
                .vec
                .with_label_values(&values(&labels)[..L::NAMES.len()])
                .inc_by(by),
        }
    }

    /// Current count of the series; for tests and self-checks.
    pub fn get(&self, labels: L) -> u64 {
        match &self.fixed {
            Some(counter) => counter.get(),
            None => self
                .vec
                .with_label_values(&values(&labels)[..L::NAMES.len()])
                .get(),
        }
    }
}

/// A value that goes up and down.
pub struct Gauge<L> {
    vec: GaugeVec,
    fixed: Option<PlainGauge>,
    labels: PhantomData<fn(L)>,
}

impl<L: LabelSet> Gauge<L> {
    pub(crate) fn new(name: &'static str) -> Result<Self, ObsError> {
        label::validate::<L>(name)?;
        let vec = GaugeVec::new(opts(name), L::NAMES)
            .map_err(|error| ObsError::Registry(error.to_string()))?;
        let fixed = L::NAMES
            .is_empty()
            .then(|| vec.with_label_values::<&str>(&[]));
        Ok(Gauge {
            vec,
            fixed,
            labels: PhantomData,
        })
    }

    pub(crate) fn collector(&self) -> Box<dyn Collector> {
        Box::new(self.vec.clone())
    }

    fn series(&self, labels: &L) -> PlainGauge {
        match &self.fixed {
            Some(gauge) => gauge.clone(),
            None => self
                .vec
                .with_label_values(&values(labels)[..L::NAMES.len()]),
        }
    }

    pub fn set(&self, labels: L, value: f64) {
        self.series(&labels).set(value);
    }

    pub fn add(&self, labels: L, delta: f64) {
        self.series(&labels).add(delta);
    }

    pub fn get(&self, labels: L) -> f64 {
        self.series(&labels).get()
    }
}

/// Observations in seconds against [`HISTOGRAM_BUCKETS`].
pub struct Histogram<L> {
    vec: HistogramVec,
    fixed: Option<PlainHistogram>,
    labels: PhantomData<fn(L)>,
}

impl<L: LabelSet> Histogram<L> {
    pub(crate) fn new(name: &'static str) -> Result<Self, ObsError> {
        label::validate::<L>(name)?;
        let options = HistogramOpts::from(opts(name)).buckets(HISTOGRAM_BUCKETS.to_vec());
        let vec = HistogramVec::new(options, L::NAMES)
            .map_err(|error| ObsError::Registry(error.to_string()))?;
        let fixed = L::NAMES
            .is_empty()
            .then(|| vec.with_label_values::<&str>(&[]));
        Ok(Histogram {
            vec,
            fixed,
            labels: PhantomData,
        })
    }

    pub(crate) fn collector(&self) -> Box<dyn Collector> {
        Box::new(self.vec.clone())
    }

    pub fn observe(&self, labels: L, seconds: f64) {
        match &self.fixed {
            Some(histogram) => histogram.observe(seconds),
            None => self
                .vec
                .with_label_values(&values(&labels)[..L::NAMES.len()])
                .observe(seconds),
        }
    }
}
