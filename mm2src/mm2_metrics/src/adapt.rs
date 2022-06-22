use std::{collections::HashMap,
          slice::Iter,
          sync::{atomic::Ordering, Arc}};

use fomat_macros::wite;
use gstuff::ERRL;
use itertools::Itertools;
use metrics::{try_recorder, IntoLabels, Key, KeyName, Label, Recorder, Unit};
use metrics_core::ScopedString;
use metrics_exporter_prometheus::formatting::key_to_parts;
use metrics_util::registry::{GenerationalAtomicStorage, Registry};
use serde_json::Value;
use std::fmt::Write;

use crate::{common::log::Tag,
            new_lib::{MetricType, MetricsArc, MetricsJson, MetricsOps, MetricsWeak, TryRecorder}};

/// Increment counter if an MmArc is not dropped yet and metrics system is initialized already.
#[macro_export]
macro_rules! mm_counter_new {
    ($metrics:expr, $name:expr, $value:expr) => {{
            if let Some(recorder) = $crate::new_lib::TryRecorder::try_recorder(&$metrics){
                let key = key_from_str($name);
                let counter = recorder.register_counter(&key);
                counter.increment($value);
            };
    }};

    // Register and increment counter with label
    ($metrics:expr, $name:expr, $value:expr, $($label_key:expr => $label_val:expr),+) => {{
         if let Some(recorder) = $crate::new_lib::TryRecorder::try_recorder(&$metrics){
             let key = $crate::metrics::Key::from_parts($name, from_slice_to_labels(&[($($label_key, $label_val),+)]));
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
            let key = key_from_str($name);
            let gauge = recorder.register_gauge(&key);
            gauge.increment($value);
        }
    }};

    // Register and increment gauge with label
    ($metrics:expr, $name:expr, $value:expr, $($label_key:expr => $label_val:expr),+) => {{
         if let Some(recorder) = $crate::new_lib::TryRecorder::try_recorder(&$metrics){
            let key = $crate::metrics::Key::from_parts($name, from_slice_to_labels(&[($($label_key, $label_val),+)]));
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
            let key = key_from_str($name);
            let histo = recorder.register_histogram(&key);
            histo.record($value);
        }

    }};

    // Register and record histogram with label
    ($metrics:expr, $name:expr, $value:expr, $($label_key:expr => $label_val:expr),+) => {{
         if let Some(recorder) = $crate::new_lib::TryRecorder::try_recorder(&$metrics){
            let key = $crate::metrics::Key::from_parts($name, from_slice_to_labels(&[($($label_key, $label_val),+)]));
            let histo = recorder.register_histogram(&key);
            histo.record($value);
         }
    }};
}

pub fn key_from_str(name: &'static str) -> Key {
    let key_name = KeyName::from_const_str(name);
    Key::from_name(key_name)
}

pub fn from_slice_to_labels(labels: &[(&'static str, &'static str)]) -> Vec<Label> { labels.into_labels() }

pub struct Snapshot {
    pub counters: HashMap<String, HashMap<Vec<String>, u64>>,
    pub gauges: HashMap<String, HashMap<Vec<String>, u64>>,
    pub histograms: HashMap<String, HashMap<Vec<String>, f64>>,
}

#[allow(dead_code)]
pub struct MmRecorder {
    pub(crate) registry: Registry<metrics::Key, GenerationalAtomicStorage>,
}

impl Default for MmRecorder {
    fn default() -> Self {
        Self {
            registry: Registry::new(metrics_util::registry::GenerationalStorage::atomic()),
        }
    }
}

impl MmRecorder {
    fn get_metrics(&self) -> Snapshot {
        let counters = self
            .registry
            .get_counter_handles()
            .into_iter()
            .map(|(key, counter)| {
                let value = counter.get_inner().load(Ordering::Acquire); // This is a specific part for counter/gauge and histogram.
                let inner = key_value_to_snapshot_entry(key.clone(), value);
                (key.to_string(), inner)
            })
            .collect::<HashMap<String, HashMap<_, _>>>();

        let gauges = self
            .registry
            .get_gauge_handles()
            .into_iter()
            .map(|(key, counter)| {
                let value = counter.get_inner().load(Ordering::Acquire); // This is a specific part for counter/gauge and histogram.
                let inner = key_value_to_snapshot_entry(key.clone(), value);
                (key.to_string(), inner)
            })
            .collect::<HashMap<String, HashMap<_, _>>>();

        let histograms = self
            .registry
            .get_histogram_handles()
            .into_iter()
            .map(|(key, histogram)| {
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

    fn prepare_tag_metrics(&self) -> Vec<PreparedMetric> {
        let mut output: Vec<PreparedMetric> = vec![];

        for (key, counter) in self.registry.get_counter_handles().drain() {
            let value = counter.get_inner().load(Ordering::Acquire);
            output.push(map_metric_to_prepare_metric(key, value));
        }

        for (key, gauge) in self.registry.get_gauge_handles().drain() {
            let value = gauge.get_inner().load(Ordering::Acquire);
            output.push(map_metric_to_prepare_metric(key, value));
        }

        for (key, histogram) in self.registry.get_histogram_handles().drain() {
            let value: f64 = histogram.get_inner().data().iter().sum();
            output.push(map_metric_to_prepare_metric(key, value as u64));
        }

        output
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
        self.registry.get_or_create_counter(key, |e| e.clone().into())
    }

    fn register_gauge(&self, key: &metrics::Key) -> metrics::Gauge {
        self.registry.get_or_create_gauge(key, |e| e.clone().into())
    }

    fn register_histogram(&self, key: &metrics::Key) -> metrics::Histogram {
        self.registry.get_or_create_histogram(key, |e| e.clone().into())
    }
}

fn key_value_to_snapshot_entry<T: Clone>(key: Key, value: T) -> HashMap<Vec<String>, T> {
    let (_name, labels) = key_to_parts(&key, None);
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

#[derive(Default)]
pub struct Metrics {
    ///  Metrics recorder can be initialized only once time.
    pub recorder: Arc<MmRecorder>,
}

impl MetricsOps for Metrics {
    fn try_recorder(&self) -> Option<Self> {
        if try_recorder().is_some() {
            return Some(Self {
                recorder: Arc::clone(&self.recorder),
            });
        };
        None
    }

    fn collect_json(&self) -> Result<Value, String> {
        let mut output = vec![];

        for counters in self.recorder.get_metrics().counters {
            for (labels, value) in counters.1.iter() {
                output.push(MetricType::Counter {
                    key: counters.0.clone(),
                    labels: labels.clone(),
                    value: *value,
                });
            }
        }

        for gauges in self.recorder.get_metrics().gauges {
            for (labels, value) in gauges.1.iter() {
                output.push(MetricType::Gauge {
                    key: gauges.0.clone(),
                    labels: labels.clone(),
                    value: *value as i64,
                });
            }
        }

        for histograms in self.recorder.get_metrics().histograms {
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

    fn prepare_tag_metrics(&self) -> Vec<PreparedMetric> { self.recorder.prepare_tag_metrics() }
}

impl TryRecorder for MetricsWeak {
    fn try_recorder(&self) -> Option<Arc<MmRecorder>> {
        let metrics = MetricsArc::from_weak(self)?;
        if let Some(recorder) = metrics.0.try_recorder() {
            return Some(recorder.recorder);
        };
        None
    }
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

#[cfg(test)]
mod test {

    use crate::new_lib::{MetricsArc, MetricsOps};

    use super::*;
    #[test]

    fn collect_json() {
        let mm_metrics = MetricsArc::new();

        mm_counter_new!(mm_metrics, "mm_counter_new_test", 123);
        mm_counter_new!(mm_metrics, "counter", 3, "james" => "maker");
        mm_counter_new!(mm_metrics, "counter", 3, "james" => "maker");
        mm_gauge_new!(mm_metrics, "mm_gauge_new_test", 5.0);
        mm_gauge_new!(mm_metrics, "gauge", 3.0, "james" => "taker");
        mm_timing_new!(mm_metrics, "test.uptime", 1.0);

        println!("{:#?}", &mm_metrics.0.collect_json());
    }
}
