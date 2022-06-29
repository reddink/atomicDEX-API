use crate::{native::MetricNameValueMap, MetricType, MetricsJson};

use common::log::{error, Tag};
use fomat_macros::wite;
use metrics::{Counter, Gauge, Key, KeyName, Label, Recorder, Unit};
use metrics_util::registry::{GenerationalAtomicStorage, Registry};
use std::fmt::Write;
use std::{collections::HashMap, slice::Iter, sync::atomic::Ordering};

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
            let value = value.get_inner().data().iter().sum::<f64>();
            let (key, labels) = key.into_parts();
            let mut qauntiles_value = HashMap::new();
            qauntiles_value.insert(key.as_str().to_string(), value as u64);
            output.push(MetricType::Histogram {
                key: key.as_str().to_string(),
                labels: labels_into_parts(labels.clone().iter()),
                quantiles: qauntiles_value,
            });
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

pub fn labels_to_tags(labels: Iter<Label>) -> Vec<Tag> {
    labels
        .map(|label| Tag {
            key: label.clone().into_parts().0.to_string(),
            val: Some(label.value().to_string()),
        })
        .collect()
}

pub fn name_value_map_to_message(name_value_map: &MetricNameValueMap) -> String {
    let mut message = String::with_capacity(256);
    match wite!(message, for (key, value) in name_value_map.iter() {
        (key) "=" (value) } separated {' '})
    {
        Ok(_) => message,
        Err(err) => {
            error!("Error {} on format hist to message", err);
            String::new()
        },
    }
}

fn labels_into_parts(labels: Iter<Label>) -> HashMap<String, String> {
    labels
        .map(|label| (label.key().to_string(), label.value().to_string()))
        .collect()
}
