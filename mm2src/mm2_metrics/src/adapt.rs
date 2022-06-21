use std::{collections::HashMap,
          slice::Iter,
          sync::{atomic::Ordering, Arc, Mutex}};

use fomat_macros::wite;
use gstuff::ERRL;
use itertools::Itertools;
use metrics::{try_recorder, IntoLabels, KeyName, Label, Recorder, Unit};
use metrics_core::ScopedString;
use metrics_exporter_prometheus::formatting::key_to_parts;
use metrics_util::registry::{GenerationalAtomicStorage, Registry};
use serde_json::Value;
use std::fmt::Write;

use crate::{common::log::Tag,
            new_lib::{MetricType, MetricsJson}};

/// Increment counter if an MmArc is not dropped yet and metrics system is initialized already.
#[macro_export]
macro_rules! mm_counter_new {
    ($metrics:expr, $name:expr, $value:expr) => {{
            if let Some(recorder) = $crate::new_lib::TryRecorder::try_recorder(&$metrics){
                 let key_name = $crate::metrics::KeyName::from_const_str(&$name);
                 let key = $crate::metrics::Key::from_name(key_name);
                 let counter = recorder.register_counter(&key);
                 counter.increment($value);
            };
    }};

    // Register and increment counter with label
    ($metrics:expr, $name:expr, $value:expr, $($label_key:expr => $label_val:expr),+) => {{
         if let Some(recorder) = $crate::new_lib::TryRecorder::try_recorder(&$metrics){
             let key_name = $crate::metrics::KeyName::from_const_str(&$name);
             let key = $crate::metrics::Key::from_parts(key_name, from_slice_to_labels(&[($($label_key, $label_val),+)]));
             let counter = recorder.register_counter(&key);
             counter.increment($value);
         };
    }};
}

/// Update gauge if an MmArc is not dropped yet and metrics system is initialized already.
#[macro_export]
macro_rules! mm_gauge_new {
    ($metrics:expr, $name:expr, $value:expr) => {{
            if let Some(recorder) = $crate::new_lib::TryRecorder::try_recorder(&$metrics){
            let key_name = $crate::metrics::KeyName::from_const_str(&$name);
            let key = $crate::metrics::Key::from_name(key_name);
            let gauge = recorder.register_gauge(&key);
            gauge.increment($value);
        }
    }};

    // Register and increment gauge with label
    ($metrics:expr, $name:expr, $value:expr, $($label_key:expr => $label_val:expr),+) => {{
         if let Some(recorder) = $crate::new_lib::TryRecorder::try_recorder(&$metrics){
            let key_name = $crate::metrics::KeyName::from_const_str(&$name);
            let key = $crate::metrics::Key::from_parts(key_name, from_slice_to_labels(&[($($label_key, $label_val),+)]));
            let gauge = recorder.register_gauge(&key);
            gauge.increment($value);
        }
    }};
}

/// Update gauge if an MmArc is not dropped yet and metrics system is initialized already.
#[macro_export]
macro_rules! mm_timing_new {
    ($metrics:expr, $name:expr, $value:expr) => {{
            if let Some(recorder) = $crate::new_lib::TryRecorder::try_recorder(&$metrics){
            let key_name = $crate::metrics::KeyName::from_const_str(&$name);
            let key = $crate::metrics::Key::from_name(key_name);
            let histo = recorder.register_histogram(&key);
            histo.record($value);
        }

    }};

    // Register and record histogram with label
    ($metrics:expr, $name:expr, $value:expr, $($label_key:expr => $label_val:expr),+) => {{
         if let Some(recorder) = $crate::new_lib::TryRecorder::try_recorder(&$metrics){
            let key_name = $crate::metrics::KeyName::from_const_str(&$name);
            let key = $crate::metrics::Key::from_parts(key_name, from_slice_to_labels(&[($($label_key, $label_val),+)]));
            let histo = recorder.register_histogram(&key);
            histo.record($value);
         }
    }};
}

#[allow(dead_code)]
fn from_slice_to_labels(labels: &[(&'static str, &'static str)]) -> Vec<Label> { labels.into_labels() }

pub struct Snapshot {
    pub counters: HashMap<String, HashMap<Vec<String>, u64>>,
    pub gauges: HashMap<String, HashMap<Vec<String>, u64>>,
    pub histograms: HashMap<String, HashMap<Vec<String>, f64>>,
}

#[allow(dead_code)]
pub(crate) struct Inner {
    pub registry: Registry<metrics::Key, GenerationalAtomicStorage>,
}

impl Inner {
    fn get_metrics(&self) -> Snapshot {
        let mut counters = HashMap::new();
        for (key, counter) in self.registry.get_counter_handles() {
            let value = counter.get_inner().load(Ordering::Acquire);
            let (_name, labels) = key_to_parts(&key, None);
            let entry = counters
                .entry(key.to_string())
                .or_insert_with(HashMap::new)
                .entry(labels)
                .or_insert(value);
            *entry = value;
        }

        let mut gauges = HashMap::new();
        for (key, gauge) in self.registry.get_gauge_handles() {
            let value = gauge.get_inner().load(Ordering::Acquire);
            let (_name, labels) = key_to_parts(&key, None);
            let entry = gauges
                .entry(key.to_string())
                .or_insert_with(HashMap::new)
                .entry(labels)
                .or_insert(value);
            *entry = value;
        }

        let mut histograms = HashMap::new();
        let histogram_handles = self.registry.get_histogram_handles();
        for (key, histogram) in histogram_handles {
            let (name, labels) = key_to_parts(&key, None);
            let value = histogram.get_inner().data().iter().sum();
            let entry = histograms
                .entry(name)
                .or_insert_with(HashMap::new)
                .entry(labels)
                .or_insert(value);
            *entry = value;
        }

        Snapshot {
            counters,
            gauges,
            histograms,
        }
    }

    fn prepare_tag_metrics(&self) -> Vec<PreparedMetric> {
        let mut output: Vec<PreparedMetric> = vec![];

        for (key, counter) in self.registry.get_counter_handles().drain() {
            let value = counter.get_inner().load(Ordering::Acquire);
            let (name, _labels) = key_to_parts(&key, None);
            let mut name_value_map = HashMap::new();
            name_value_map.insert(ScopedString::Owned(name), Integer::Unsigned(value));
            output.push(PreparedMetric {
                tags: labels_to_tags(key.labels()),
                message: name_value_map_to_message(&name_value_map),
            });
        }

        for (key, gauge) in self.registry.get_gauge_handles().drain() {
            let value = gauge.get_inner().load(Ordering::Acquire);
            let (name, _labels) = key_to_parts(&key, None);
            let mut name_value_map = HashMap::new();
            name_value_map.insert(ScopedString::Owned(name), Integer::Unsigned(value));
            output.push(PreparedMetric {
                tags: labels_to_tags(key.labels()),
                message: name_value_map_to_message(&name_value_map),
            });
        }

        for (key, histogram) in self.registry.get_histogram_handles().drain() {
            let value: f64 = histogram.get_inner().data().iter().sum();
            let (name, _labels) = key_to_parts(&key, None);
            let mut name_value_map = HashMap::new();

            name_value_map.insert(ScopedString::Owned(name), Integer::Unsigned(value as u64));
            output.push(PreparedMetric {
                tags: labels_to_tags(key.labels()),
                message: name_value_map_to_message(&name_value_map),
            });
        }

        output
    }
}

#[derive(Clone)]
pub struct MmRecorder {
    pub(crate) inner: Arc<Mutex<Inner>>,
}

impl Default for MmRecorder {
    fn default() -> Self {
        Self {
            inner: Arc::new(Mutex::new(Inner {
                registry: Registry::new(metrics_util::registry::GenerationalStorage::atomic()),
            })),
        }
    }
}

impl From<Inner> for MmRecorder {
    fn from(inner: Inner) -> Self {
        MmRecorder {
            inner: Arc::new(Mutex::new(inner)),
        }
    }
}

impl MmRecorder {
    pub fn try_recorder(&mut self) -> Option<&mut Self> {
        if try_recorder().is_some() {
            return Some(self);
        };
        None
    }

    pub fn handle(&self) -> MmHandle {
        MmHandle {
            metrics: self.inner.clone(),
        }
    }
}

impl Recorder for MmRecorder {
    fn describe_counter(&self, _key_name: KeyName, _unit: Option<Unit>, _description: &'static str) {
        // mm2_metrics don't use this method
    }

    fn describe_gauge(&self, _key_name: KeyName, _unit: Option<Unit>, _description: &'static str) {
        // mm2_metrics don't use this method
    }

    fn describe_histogram(&self, _key_name: KeyName, _unit: Option<Unit>, _description: &'static str) {
        // mm2_metrics don't use this method
    }

    fn register_counter(&self, key: &metrics::Key) -> metrics::Counter {
        self.inner
            .lock()
            .unwrap()
            .registry
            .get_or_create_counter(key, |e| e.clone().into())
    }

    fn register_gauge(&self, key: &metrics::Key) -> metrics::Gauge {
        self.inner
            .lock()
            .unwrap()
            .registry
            .get_or_create_gauge(key, |e| e.clone().into())
    }

    fn register_histogram(&self, key: &metrics::Key) -> metrics::Histogram {
        self.inner
            .lock()
            .unwrap()
            .registry
            .get_or_create_histogram(key, |e| e.clone().into())
    }
}

pub struct MmHandle {
    metrics: Arc<Mutex<Inner>>,
}

impl MmHandle {
    pub fn collect_json(&self) -> Result<Value, String> {
        let mut output = vec![];

        for counters in self.metrics.lock().unwrap().get_metrics().counters {
            for (labels, value) in counters.1.iter() {
                output.push(MetricType::Counter {
                    key: counters.0.clone(),
                    labels: labels.clone(),
                    value: *value,
                });
            }
        }

        for gauges in self.metrics.lock().unwrap().get_metrics().gauges {
            for (labels, value) in gauges.1.iter() {
                output.push(MetricType::Gauge {
                    key: gauges.0.clone(),
                    labels: labels.clone(),
                    value: *value as i64,
                });
            }
        }

        for histograms in self.metrics.lock().unwrap().get_metrics().histograms {
            for (labels, value) in histograms.1.iter() {
                let mut qauntiles_value = HashMap::new();
                qauntiles_value.insert(histograms.0.clone(), *value as u64);
                output.push(MetricType::Histogram {
                    key: histograms.0.clone(),
                    labels: labels.clone(),
                    quantiles: qauntiles_value,
                });
            }
        }

        let output = MetricsJson { metrics: output };
        serde_json::to_value(output).map_err(|err| ERRL!("{}", err))
    }

    pub fn prepare_tag_metrics(&self) -> Vec<PreparedMetric> { self.metrics.lock().unwrap().prepare_tag_metrics() }
}

#[derive(Debug)]
#[allow(dead_code)]
pub struct PreparedMetric {
    tags: Vec<Tag>,
    message: String,
}

#[allow(dead_code)]
#[derive(Eq, PartialEq, PartialOrd, Ord)]
enum Integer {
    Signed(i64),
    Unsigned(u64),
}

impl ToString for Integer {
    fn to_string(&self) -> String {
        match self {
            Integer::Signed(x) => format!("{}", x),
            Integer::Unsigned(x) => format!("{}", x),
        }
    }
}

type MetricName = ScopedString;

type MetricNameValueMap = HashMap<MetricName, Integer>;

fn labels_to_tags(labels: Iter<metrics::Label>) -> Vec<Tag> {
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

#[cfg(test)]
mod test {

    use crate::new_lib::MetricsArcNew;

    use super::*;
    #[test]

    fn collect_json() {
        let mm_metrics = MetricsArcNew::new();
        // let mm_metrics = *mm_metrics;
        mm_counter_new!(mm_metrics, "mm_counter_new_test", 123);
        mm_counter_new!(mm_metrics, "counter", 3, "james" => "maker");
        mm_counter_new!(mm_metrics, "counter", 3, "james" => "maker");
        mm_gauge_new!(mm_metrics, "mm_gauge_new_test", 5.0);
        mm_gauge_new!(mm_metrics, "gauge", 3.0, "james" => "taker");
        mm_timing_new!(mm_metrics, "test.uptime", 1.0);

        // let expected: MetricsJson = serde_json::from_value(handle.collect_json().unwrap()).unwrap();
        println!("{:#?}", &mm_metrics.0.lock().unwrap().handle().collect_json());
    }
}
