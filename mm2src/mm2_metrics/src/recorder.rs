use crate::{mm_metrics::MmHistogram, MetricType, MetricsJson};

use metrics::{Counter, CounterFn, Gauge, Histogram, Key, KeyName, Label, Recorder, Unit};
#[cfg(not(target_arch = "wasm32"))]
use metrics_exporter_prometheus::{PrometheusBuilder, PrometheusRecorder};
use metrics_util::registry::{GenerationalAtomicStorage, GenerationalStorage, Registry};
use std::{collections::HashMap,
          slice::Iter,
          sync::{atomic::Ordering, Arc}};

const QUANTILES: [f64; 2] = [0.0, 1.0];

pub struct Snapshot {
    pub counters: HashMap<String, HashMap<Vec<String>, u64>>,
    pub gauges: HashMap<String, HashMap<Vec<String>, f64>>,
    pub histograms: HashMap<String, HashMap<Vec<String>, Vec<f64>>>,
}

/// Please consider moving it, `MultiGauge` and `MultiHistogram` to a separate mod.
#[derive(Default)]
pub(crate) struct MultiCounter {
    counters: Vec<Counter>,
}

impl MultiCounter {
    pub(crate) fn new(counters: Vec<Counter>) -> MultiCounter { MultiCounter { counters } }
}

impl CounterFn for MultiCounter {
    fn increment(&self, value: u64) {
        for counter in self.counters.iter() {
            counter.increment(value)
        }
    }

    fn absolute(&self, value: u64) {
        for counter in self.counters.iter() {
            counter.absolute(value)
        }
    }
}

/// `MmRecorder` the core of mm metrics.
///
///  Registering, Recording, Updating and Collecting metrics is all done from within MmRecorder.
pub struct MmRecorder {
    pub(crate) registry: Registry<Key, GenerationalAtomicStorage>,
    #[cfg(not(target_arch = "wasm32"))]
    prometheus_recorder: PrometheusRecorder,
}

impl Default for MmRecorder {
    fn default() -> Self {
        Self {
            registry: Registry::new(GenerationalStorage::atomic()),
            #[cfg(not(target_arch = "wasm32"))]
            prometheus_recorder: PrometheusBuilder::new().set_quantiles().build_recorder(),
        }
    }
}

impl MmRecorder {
    #[cfg(not(target_arch = "wasm32"))]
    pub fn render(&self) -> String { self.prometheus_recorder.handle().render() }

    pub fn prepare_json(&self) -> MetricsJson {
        let mut output = vec![];

        for (key, counter) in self.registry.get_counter_handles() {
            let (key, labels) = key.into_parts();
            let value = counter.get_inner().load(Ordering::Acquire);
            output.push(MetricType::Counter {
                key: key.as_str().to_string(),
                labels: labels_into_parts(labels.clone().iter()),
                value,
            });
        }

        for (key, gauge) in self.registry.get_gauge_handles() {
            let (key, labels) = key.into_parts();
            let value = f64::from_bits(gauge.get_inner().load(Ordering::Acquire));
            output.push(MetricType::Gauge {
                key: key.as_str().to_string(),
                labels: labels_into_parts(labels.clone().iter()),
                value,
            });
        }

        for (key, histogram) in self.registry.get_histogram_handles() {
            let value = histogram.get_inner().data();
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

    #[cfg(target_arch = "wasm32")]
    fn register_counter(&self, key: &Key) -> Counter { self.registry.get_or_create_gauge(key, |e| e.clone().into()) }

    #[cfg(not(target_arch = "wasm32"))]
    fn register_counter(&self, key: &Key) -> Counter {
        let counter = MultiCounter::new(vec![
            self.registry.get_or_create_gauge(key, |e| e.clone().into()),
            self.prometheus_recorder.register_counter(key),
        ]);
        Counter::from_arc(Arc::new(counter))
    }

    fn register_gauge(&self, key: &Key) -> Gauge { self.registry.get_or_create_gauge(key, |e| e.clone().into()) }

    fn register_histogram(&self, key: &Key) -> Histogram {
        self.registry.get_or_create_histogram(key, |e| e.clone().into())
    }
}

pub trait TryRecorder {
    /// Check for recorder and set one if none is set.
    fn try_recorder(&self) -> Option<Arc<MmRecorder>>;
}

/// Used for parsing `Iter<Label>` into `Key` and `Value`.
fn labels_into_parts(labels: Iter<Label>) -> HashMap<String, String> {
    labels
        .map(|label| (label.key().to_string(), label.value().to_string()))
        .collect()
}
