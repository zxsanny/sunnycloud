//! Families built from a snapshot a helper crate exposes (`Db::stats`, `AuditWriter::stats`,
//! `AbuseLimiter::stats`). The helpers hold plain counters, so the exporter reads them at scrape time
//! instead of the helpers depending on this crate.

use std::collections::HashMap;

use prometheus::core::{Collector, Desc};
use prometheus::proto::{self, MetricType};

type Read = Box<dyn Fn() -> Vec<proto::MetricFamily> + Send + Sync>;

/// A collector whose families come from `read` at every scrape. `declared` lists `(name, help, label names)`
/// of every family `read` may return, so the registry can refuse a duplicate registration up front.
pub struct SnapshotCollector {
    descs: Vec<Desc>,
    read: Read,
}

impl SnapshotCollector {
    pub(crate) fn new(
        declared: &[(&str, &str, &[&str])],
        read: impl Fn() -> Vec<proto::MetricFamily> + Send + Sync + 'static,
    ) -> Self {
        let descs = declared
            .iter()
            .map(|(name, help, labels)| {
                let labels = labels.iter().map(|label| (*label).to_owned()).collect();
                // The names are constants of this crate, so a failure is a programming error found by the first test.
                Desc::new(
                    (*name).to_owned(),
                    (*help).to_owned(),
                    labels,
                    HashMap::new(),
                )
                .unwrap_or_else(|error| panic!("invalid metric description `{name}`: {error}"))
            })
            .collect();
        SnapshotCollector {
            descs,
            read: Box::new(read),
        }
    }
}

impl Collector for SnapshotCollector {
    fn desc(&self) -> Vec<&Desc> {
        self.descs.iter().collect()
    }

    fn collect(&self) -> Vec<proto::MetricFamily> {
        (self.read)()
    }
}

pub(crate) type Labels<'a> = &'a [(&'a str, &'a str)];

fn metric(labels: Labels<'_>) -> proto::Metric {
    let mut metric = proto::Metric::default();
    metric.set_label(
        labels
            .iter()
            .map(|(name, value)| {
                let mut pair = proto::LabelPair::default();
                pair.set_name((*name).to_owned());
                pair.set_value((*value).to_owned());
                pair
            })
            .collect(),
    );
    metric
}

fn family(
    name: &str,
    help: &str,
    kind: MetricType,
    metrics: Vec<proto::Metric>,
) -> proto::MetricFamily {
    let mut family = proto::MetricFamily::default();
    family.set_name(name.to_owned());
    family.set_help(help.to_owned());
    family.set_field_type(kind);
    family.set_metric(metrics);
    family
}

#[cfg(feature = "db")]
pub(crate) fn gauge_family(
    name: &str,
    help: &str,
    samples: &[(Labels<'_>, f64)],
) -> proto::MetricFamily {
    let metrics = samples
        .iter()
        .map(|(labels, value)| {
            let mut metric = metric(labels);
            let mut gauge = proto::Gauge::default();
            gauge.set_value(*value);
            metric.set_gauge(gauge);
            metric
        })
        .collect();
    family(name, help, MetricType::GAUGE, metrics)
}

pub(crate) fn counter_family(
    name: &str,
    help: &str,
    samples: &[(Labels<'_>, f64)],
) -> proto::MetricFamily {
    let metrics = samples
        .iter()
        .map(|(labels, value)| {
            let mut metric = metric(labels);
            let mut counter = proto::Counter::default();
            counter.set_value(*value);
            metric.set_counter(counter);
            metric
        })
        .collect();
    family(name, help, MetricType::COUNTER, metrics)
}

/// A summary with a count and a sum and no quantiles: all the snapshots record is how many and how long.
#[cfg(feature = "db")]
pub(crate) fn summary_family(
    name: &str,
    help: &str,
    samples: &[(Labels<'_>, u64, f64)],
) -> proto::MetricFamily {
    let metrics = samples
        .iter()
        .map(|(labels, count, sum)| {
            let mut metric = metric(labels);
            let mut summary = proto::Summary::default();
            summary.set_sample_count(*count);
            summary.set_sample_sum(*sum);
            metric.set_summary(summary);
            metric
        })
        .collect();
    family(name, help, MetricType::SUMMARY, metrics)
}
