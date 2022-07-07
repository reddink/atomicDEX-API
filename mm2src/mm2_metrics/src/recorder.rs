use crate::{native::{MetricNameValueMap, MmHistogram},
            MetricType, MetricsJson};

use common::log::Tag;
use itertools::Itertools;
use metrics::{Counter, Gauge, Key, KeyName, Label, Recorder, Unit};
// use metrics_exporter_prometheus::formatting::{key_to_parts, write_metric_line, write_type_line};
use metrics_util::registry::{GenerationalAtomicStorage, Registry};
use std::{collections::HashMap,
          slice::Iter,
          sync::{atomic::Ordering, Arc}};

// pub struct Snapshot {
//     pub counters: HashMap<String, HashMap<Vec<String>, u64>>,
//     pub gauges: HashMap<String, HashMap<Vec<String>, f64>>,
//     pub histograms: HashMap<String, HashMap<Vec<String>, f64>>,
// }

/// `MmRecorder` the core of mm metrics.
///
///  Registering, Recording, Updating and Collecting metrics is all done from within MmRecorder.
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
    // fn get_metrics(&self) -> Snapshot {
    //     let counters = self
    //         .registry
    //         .get_counter_handles()
    //         .into_iter()
    //         .map(|(key, counter)| {
    //             let value = counter.get_inner().load(Ordering::Acquire);
    //             let inner = key_value_to_snapshot_entry(key.clone(), value);
    //             (key.into_parts().0.as_str().to_string(), inner)
    //         })
    //         .collect::<HashMap<_, _>>();

    //     let gauges = self
    //         .registry
    //         .get_gauge_handles()
    //         .into_iter()
    //         .map(|(key, gauge)| {
    //             gauge.get_generation();
    //             let value = gauge.get_inner().load(Ordering::Acquire);
    //             let inner = key_value_to_snapshot_entry(key.clone(), f64::from_bits(value));
    //             (key.into_parts().0.as_str().to_string(), inner)
    //         })
    //         .collect::<HashMap<_, _>>();

    //     let histograms = self
    //         .registry
    //         .get_histogram_handles()
    //         .into_iter()
    //         .map(|(key, histogram)| {
    //             histogram.get_generation();
    //             let value: f64 = histogram.get_inner().data().iter().sum();
    //             let inner = key_value_to_snapshot_entry(key.clone(), value);
    //             (key.into_parts().0.as_str().to_string(), inner)
    //         })
    //         .collect::<HashMap<_, _>>();

    //     Snapshot {
    //         counters,
    //         gauges,
    //         histograms,
    //     }
    // }

    // pub fn render(&self) -> String {
    //     let Snapshot {
    //         mut counters,
    //         histograms: _,
    //         mut gauges,
    //     } = self.get_metrics();

    //     let mut output = String::new();

    //     for (name, mut by_labels) in counters.drain() {
    //         write_type_line(&mut output, name.as_str(), "counter");
    //         for (labels, value) in by_labels.drain() {
    //             write_metric_line::<&str, u64>(&mut output, &name, None, &labels, None, value);
    //         }
    //         output.push('\n');
    //     }

    //     for (name, mut by_labels) in gauges.drain() {
    //         write_type_line(&mut output, name.as_str(), "gauge");
    //         for (labels, value) in by_labels.drain() {
    //             write_metric_line::<&str, f64>(&mut output, &name, None, &labels, None, value);
    //         }
    //         output.push('\n');
    //     }

    //     for (key, bucket) in self.registry.get_histogram_handles() {
    //         let (name, _labels) = key_to_parts(&key, None);
    //         write_type_line(&mut output, &name, "histogram");
    //         let values = bucket.get_inner().data();
    //         let mut count = 0;
    //         let mut sum = 0.0;

    //         count += values.len();
    //         values.iter().for_each(|value| {
    //             sum += value;
    //         });

    //         write_metric_line::<&str, usize>(
    //             &mut output,
    //             &name,
    //             Some("bucket"),
    //             key_to_parts(&key, None).1.as_slice(),
    //             None,
    //             count,
    //         );
    //         output.push('\n');
    //     }

    //     output
    // }

    pub fn prepare_json(&self) -> MetricsJson {
        let mut output = vec![];

        for counters in self.registry.get_counter_handles() {
            let (key, labels) = counters.0.into_parts();
            let value = counters.1.get_inner().load(Ordering::Acquire);
            output.push(MetricType::Counter {
                key: key.as_str().to_string(),
                labels: labels_into_parts(labels.clone().iter()),
                value,
            });
        }

        for counters in self.registry.get_gauge_handles() {
            let (key, labels) = counters.0.into_parts();
            let value = f64::from_bits(counters.1.get_inner().load(Ordering::Acquire));
            output.push(MetricType::Gauge {
                key: key.as_str().to_string(),
                labels: labels_into_parts(labels.clone().iter()),
                value,
            });
        }

        for (key, value) in self.registry.get_histogram_handles() {
            let value = value.get_inner().data();
            let (key, labels) = key.into_parts();
            let mm_histogram = MmHistogram::new(&value);

            if let Some(qauntiles_value) = mm_histogram {
                output.push(MetricType::Histogram {
                    key: key.as_str().to_string(),
                    labels: labels_into_parts(labels.clone().iter()),
                    quantiles: qauntiles_value.to_json_quantiles(),
                });
            }
        }

        MetricsJson { metrics: output }
    }
}

// fn key_value_to_snapshot_entry<T: Clone>(key: Key, value: T) -> HashMap<Vec<String>, T> {
//     let (_name, labels) = key_to_parts(&key, None);
//     let mut entry = HashMap::new();
//     entry.insert(labels, value);
//     entry
// }

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

pub trait TryRecorder {
    /// Check for recorder and set one if none is set.
    fn try_recorder(&self) -> Option<Arc<MmRecorder>>;
}

pub(crate) fn labels_to_tags(labels: Iter<Label>) -> Vec<Tag> {
    labels
        .map(|label| Tag {
            key: label.clone().into_parts().0.to_string(),
            val: Some(label.value().to_string()),
        })
        .collect()
}

/// Used for parsing `MetricNameValueMap` into Message(loggable string).
pub(crate) fn name_value_map_to_message(name_value_map: &MetricNameValueMap) -> String {
    name_value_map
        .iter()
        .sorted_by(|x, y| x.partial_cmp(y).expect("sorting faulted"))
        .map(|(key, value)| match value {
            crate::native::PreparedMetric::Unsigned(e) => format!("{}={:?}", key, e),
            crate::native::PreparedMetric::Float(e) => format!("{}={:?}", key, e),
            crate::native::PreparedMetric::Histogram(e) => format!("{}={:?}", key, e.to_tag_message()),
        })
        .join(" ")
}

/// Used for parsing `Iter<Label>` into `Key` and `Value`.
fn labels_into_parts(labels: Iter<Label>) -> HashMap<String, String> {
    labels
        .map(|label| (label.key().to_string(), label.value().to_string()))
        .collect()
}
