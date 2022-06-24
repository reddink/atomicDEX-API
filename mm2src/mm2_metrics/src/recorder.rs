use std::{collections::HashMap, slice::Iter, sync::atomic::Ordering};

use crate::{adapt::{Integer, MetricNameValueMap, PreparedMetric},
            new_lib::{MetricType, MetricsJson}};
use common::log::Tag;
use fomat_macros::wite;
use itertools::Itertools;
use metrics::{Counter, Gauge, Key, KeyName, Label, Recorder, Unit};
use metrics_core::ScopedString;
use metrics_exporter_prometheus::formatting::key_to_parts;
use metrics_util::registry::{GenerationalAtomicStorage, Registry};
use std::fmt::Write;

pub struct Snapshot {
    pub counters: HashMap<String, HashMap<Vec<Label>, u64>>,
    pub gauges: HashMap<String, HashMap<Vec<Label>, u64>>,
    pub histograms: HashMap<String, HashMap<Vec<Label>, f64>>,
}

pub struct MmRecorder {
    pub(crate) registry: Registry<Key, GenerationalAtomicStorage>,
}

impl Default for MmRecorder {
    fn default() -> Self {
        Self {
            registry: Registry::new(metrics_util::registry::GenerationalStorage::atomic()),
        }
    }
}

impl MmRecorder {
    pub fn get_metrics(&self) -> Snapshot {
        let counters = self
            .registry
            .get_counter_handles()
            .into_iter()
            .map(|(key, counter)| {
                counter.get_generation();
                let value = counter.get_inner().load(Ordering::Acquire); // This is a specific part for counter/gauge and histogram.
                let inner = key_value_to_snapshot_entry(key.clone(), value);
                (key.to_string(), inner)
            })
            .collect::<HashMap<String, HashMap<_, _>>>();

        let gauges = self
            .registry
            .get_gauge_handles()
            .into_iter()
            .map(|(key, gauge)| {
                gauge.get_generation();
                let value = gauge.get_inner().load(Ordering::Acquire); // This is a specific part for counter/gauge and histogram.
                let inner = key_value_to_snapshot_entry(key.clone(), value);
                (key.to_string(), inner)
            })
            .collect::<HashMap<String, HashMap<_, _>>>();

        let histograms = self
            .registry
            .get_histogram_handles()
            .into_iter()
            .map(|(key, histogram)| {
                histogram.get_generation();
                let value: f64 = histogram.get_inner().data().iter().sum(); // This is a specific part for counter/gauge and histogram.
                let inner = key_value_to_snapshot_entry(key.clone(), value);
                (key.to_string(), inner)
            })
            .collect::<HashMap<String, HashMap<_, _>>>();

        Snapshot {
            counters,
            gauges,
            histograms,
        }
    }

    pub fn prepare_tag_metrics(&self) -> Vec<PreparedMetric> {
        let mut output: Vec<PreparedMetric> = vec![];

        for (key, counter) in self.registry.get_counter_handles() {
            let value = counter.get_inner().load(Ordering::Acquire);
            output.push(map_metric_to_prepare_metric(key, value));
        }

        for (key, gauge) in self.registry.get_gauge_handles() {
            let value = gauge.get_inner().load(Ordering::Acquire);
            output.push(map_metric_to_prepare_metric(key, value));
        }

        for (key, histogram) in self.registry.get_histogram_handles() {
            let value: f64 = histogram.get_inner().data().iter().sum();
            output.push(map_metric_to_prepare_metric(key, value as u64));
        }

        output
    }

    pub fn prepare_json(&self) -> MetricsJson {
        let Snapshot {
            counters,
            gauges,
            histograms,
        } = self.get_metrics();

        let mut output = vec![];

        for counters in counters {
            for (labels, value) in counters.1.iter() {
                output.push(MetricType::Counter {
                    key: counters.0.clone(),
                    labels: labels_into_parts(labels.clone().iter()),
                    value: *value,
                });
            }
        }

        for gauges in gauges {
            for (labels, value) in gauges.1.iter() {
                output.push(MetricType::Gauge {
                    key: gauges.0.clone(),
                    labels: labels_into_parts(labels.clone().iter()),
                    value: *value as i64,
                });
            }
        }

        for histograms in histograms {
            for (labels, value) in histograms.1.iter() {
                let mut qauntiles_value = HashMap::new();
                qauntiles_value.insert(histograms.0.clone(), *value as u64);
                output.push(MetricType::Histogram {
                    key: histograms.0.clone(),
                    labels: labels_into_parts(labels.clone().iter()),
                    quantiles: qauntiles_value,
                });
            }
        }

        MetricsJson { metrics: output }
    }
}

impl Recorder for MmRecorder {
    fn describe_counter(&self, _key_name: KeyName, _unit: Option<Unit>, _description: &'static str) {
        // mm2_metrics doesn't use this method
    }

    fn describe_gauge(&self, _key_name: KeyName, _unit: Option<Unit>, _description: &'static str) {
        // mm2_metrics doesn't use this method
    }

    fn describe_histogram(&self, _key_name: KeyName, _unit: Option<Unit>, _description: &'static str) {
        // mm2_metrics doesn't use this method
    }

    fn register_counter(&self, key: &Key) -> Counter { self.registry.get_or_create_counter(key, |e| e.clone().into()) }

    fn register_gauge(&self, key: &Key) -> Gauge { self.registry.get_or_create_gauge(key, |e| e.clone().into()) }

    fn register_histogram(&self, key: &Key) -> metrics::Histogram {
        self.registry.get_or_create_histogram(key, |e| e.clone().into())
    }
}

fn key_value_to_snapshot_entry<T: Clone>(key: Key, value: T) -> HashMap<Vec<Label>, T> {
    let (_name, labels) = key.into_parts();
    let mut entry = HashMap::new();
    entry.insert(labels, value);
    entry
}

fn map_metric_to_prepare_metric<T: Clone>(key: Key, value: T) -> PreparedMetric
where
    u64: From<T>,
{
    let (name, _labels) = key_to_parts(&key, None);
    let mut name_value_map = HashMap::new();

    name_value_map.insert(ScopedString::Owned(name), Integer::Unsigned(u64::from(value)));
    PreparedMetric {
        tags: labels_to_tags(key.labels()),
        message: name_value_map_to_message(&name_value_map),
    }
}

fn labels_to_tags(labels: Iter<Label>) -> Vec<Tag> {
    labels
        .map(|label| Tag {
            key: label.key().to_string(),
            val: Some(label.value().to_string()),
        })
        .collect()
}

fn name_value_map_to_message(name_value_map: &MetricNameValueMap) -> String {
    let mut message = String::with_capacity(256);
    match wite!(message, for (key, value) in name_value_map.iter().sorted() { (key) "=" (value.to_string()) } separated {' '})
    {
        Ok(_) => message,
        Err(err) => {
            log!("Error " (err) " on format hist to message");
            String::new()
        },
    }
}

fn labels_into_parts(labels: Iter<Label>) -> HashMap<String, String> {
    labels
        .map(|label| (label.key().to_string(), label.value().to_string()))
        .collect()
}
