use std::{collections::HashMap,
          convert::TryInto,
          slice::Iter,
          sync::{atomic::Ordering, Arc}};

use hdrhistogram::Histogram;
use metrics::{KeyName, Recorder, Unit};
use metrics_exporter_prometheus::formatting::key_to_parts;
use metrics_util::{registry::{GenerationalAtomicStorage, Registry},
                   Quantile};
use serde_json::Value;

use crate::log::Tag;

pub struct MmMetricsBuilder {
    global_labels: HashMap<String, Value>,
}

#[allow(dead_code)]
impl MmMetricsBuilder {
    fn new() -> Self {
        Self {
            global_labels: HashMap::new(),
        }
    }
    /// Add a label which will be attached to all metrics by default.
    pub fn add_global_label<K, V>(mut self, key: K, value: V) -> Self
    where
        K: Into<String>,
        V: Into<Value>,
    {
        self.global_labels.insert(key.into(), value.into());
        self
    }

    fn build(&self) -> Result<MmRecorder, String> {
        Ok(MmRecorder {
            inner: Arc::new(Inner {
                registry: Registry::new(GenerationalAtomicStorage::atomic()),
                global_labels: self.global_labels.clone(),
            }),
        })
    }
}
pub struct Snapshot {
    pub counters: HashMap<String, HashMap<Vec<String>, u64>>,
    pub gauges: HashMap<String, HashMap<Vec<String>, f64>>,
}

#[allow(dead_code)]
pub(crate) struct Inner {
    pub registry: Registry<metrics::Key, GenerationalAtomicStorage>,
    pub global_labels: HashMap<String, Value>,
}

impl Inner {
    fn get_recent_metrics(&self) -> Snapshot {
        let mut counters = HashMap::new();
        let counter_handles = self.registry.get_counter_handles();
        for (key, counter) in counter_handles {
            let (name, labels) = key_to_parts(&key, None);
            let value = counter.get_inner().load(Ordering::Acquire);
            let entry = counters
                .entry(name)
                .or_insert_with(HashMap::new)
                .entry(labels)
                .or_insert(0);
            *entry = value;
        }

        let mut gauges = HashMap::new();
        let gauge_handles = self.registry.get_gauge_handles();
        for (key, gauge) in gauge_handles {
            let (name, labels) = key_to_parts(&key, None);
            let value = f64::from_bits(gauge.get_inner().load(Ordering::Acquire));
            let entry = gauges
                .entry(name)
                .or_insert_with(HashMap::new)
                .entry(labels)
                .or_insert(0.0);
            *entry = value;
        }

        Snapshot { counters, gauges }
    }

    fn prepare_metrics(&self) -> Vec<PreparedMetric> {
        let Snapshot {
            mut counters,
            mut gauges,
        } = self.get_recent_metrics();

        let mut output: Vec<PreparedMetric> = vec![];

        for (name, mut by_labels) in counters.drain() {
            for (_labels, value) in by_labels.drain() {
                output.push(PreparedMetric {
                    tags: labels_to_tags(metrics_core::Key::from_name(name.clone()).labels()),
                    message: format!("{} {}", name, value),
                });
            }
        }

        for (name, mut by_labels) in gauges.drain() {
            for (_labels, value) in by_labels.drain() {
                output.push(PreparedMetric {
                    tags: labels_to_tags(metrics_core::Key::from_name(name.clone()).labels()),
                    message: format!("{} {}", name, value),
                });
            }
        }

        output
    }

    fn collect_json(&self) -> Result<MetricsJson, String> {
        let mut output: Vec<MetricType> = vec![];

        for (key, gauge) in self.registry.get_counter_handles() {
            let value = gauge.get_inner().load(Ordering::Acquire);
            let (_name, labels) = key_to_parts(&key, None);
            output.push(MetricType::Counter {
                key: key.to_string(),
                labels,
                value,
            });
        }

        for (key, gauge) in self.registry.get_gauge_handles() {
            let value = gauge.get_inner().load(Ordering::Acquire);
            let (_name, labels) = key_to_parts(&key, None);
            output.push(MetricType::Gauge {
                key: key.to_string(),
                labels,
                value: value.try_into().unwrap(),
            });
        }

        for (key, histogram) in self.registry.get_histogram_handles() {
            let (_name, labels) = key_to_parts(&key, None);
            let mut q_values = vec![];
            let mut count = 0;
            histogram.get_inner().clear_with(|values| {
                count += values.len();
                for value in values {
                    q_values.push(Quantile::new(*value));
                }
            });

            // Use default significant figures value.
            // For more info on `sigfig` see the Historgam::new_with_bounds().
            let sigfig = 3;
            let histogram = Histogram::new(sigfig).unwrap();
            let mut quantiles = hist_at_quantiles(histogram, &q_values);
            // add total quantiles number
            quantiles.insert("count".into(), count as u64);
            output.push(MetricType::Histogram {
                key: key.to_string(),
                labels,
                quantiles,
            });
        }

        Ok(MetricsJson { metrics: output })
    }
}

#[derive(Clone)]
pub struct MmRecorder {
    inner: Arc<Inner>,
}

impl MmRecorder {
    pub fn handle(&self) -> MmHandle {
        MmHandle {
            metrics: self.inner.clone(),
        }
    }
    /// Install this recorder as the global default recorder.
    pub fn install(self) -> Result<MmHandle, String> {
        let handle = self.handle();
        metrics::set_boxed_recorder(Box::new(self)).map_err(|e| e.to_string())?;
        Ok(handle)
    }
}

impl From<Inner> for MmRecorder {
    fn from(inner: Inner) -> Self { MmRecorder { inner: Arc::new(inner) } }
}

impl Recorder for MmRecorder {
    fn describe_counter(&self, _key_name: KeyName, _unit: Option<Unit>, _description: &'static str) {
        // self.add_description_if_missing(&key_name, description)
    }

    fn describe_gauge(&self, _key_name: KeyName, _unit: Option<Unit>, _description: &'static str) {
        // self.add_description_if_missing(&key_name, description)
    }

    fn describe_histogram(&self, _key_name: KeyName, _unit: Option<Unit>, _description: &'static str) {
        // self.add_description_if_missing(&key_name, description)
    }

    fn register_counter(&self, key: &metrics::Key) -> metrics::Counter {
        self.inner.registry.get_or_create_counter(key, |e| e.clone().into())
    }

    fn register_gauge(&self, key: &metrics::Key) -> metrics::Gauge {
        self.inner.registry.get_or_create_gauge(key, |e| e.clone().into())
    }

    fn register_histogram(&self, key: &metrics::Key) -> metrics::Histogram {
        self.inner.registry.get_or_create_histogram(key, |e| e.clone().into())
    }
}

pub struct MmHandle {
    metrics: Arc<Inner>,
}

impl MmHandle {
    // pub fn render(&self) -> String { self.metrics.render() }
    pub fn collect_json(&self) -> Result<Value, String> {
        serde_json::to_value(self.metrics.collect_json().unwrap()).map_err(|err| ERRL!("{}", err))
    }
    pub fn prepare_metrics(&self) -> Vec<PreparedMetric> { self.metrics.prepare_metrics() }
}

#[derive(Serialize, Debug, Default, Deserialize)]
pub struct MetricsJson {
    pub metrics: Vec<MetricType>,
}

#[derive(Eq, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
#[serde(tag = "type")]
pub enum MetricType {
    Counter {
        key: String,
        labels: Vec<String>,
        value: u64,
    },
    Gauge {
        key: String,
        labels: Vec<String>,
        value: i64,
    },
    Histogram {
        key: String,
        labels: Vec<String>,
        #[serde(flatten)]
        quantiles: HashMap<String, u64>,
    },
}

#[allow(dead_code)]
pub struct PreparedMetric {
    tags: Vec<Tag>,
    message: String,
}

// struct TagExporter {
//     /// Using a weak reference by default in order to avoid circular references and leaks.
//     log_state: LogWeak,
//     /// Handle for converting snapshots into log.
//     metrics: Vec<PreparedMetric>,
// }

// impl TagExporter {
//     /// Run endless async loop
//     async fn run(mut self, interval: f64) {
//         loop {
//             Timer::sleep(interval).await;
//             self.turn();
//         }
//     }
//     fn turn(&mut self) {
//         println!("runnig");
//         let log_state = match LogArc::from_weak(&self.log_state) {
//             Some(x) => x,
//             // MmCtx is dropped already
//             _ => {
//                 return;
//             },
//         };

//         log!(">>>>>>>>>> DEX metrics <<<<<<<<<");

//         for PreparedMetric { tags, message } in &self.metrics {
//             log_state.log_deref_tags("", tags.to_vec(), &message);
//         }
//     }
// }

fn labels_to_tags(labels: Iter<metrics_core::Label>) -> Vec<Tag> {
    labels
        .map(|label| Tag {
            key: label.key().to_string(),
            val: Some(label.value().to_string()),
        })
        .collect()
}

fn hist_at_quantiles(hist: Histogram<u64>, quantiles: &[Quantile]) -> HashMap<String, u64> {
    quantiles
        .iter()
        .map(|quantile| {
            let key = quantile.label().to_string();
            let val = hist.value_at_quantile(quantile.value());
            (key, val)
        })
        .collect()
}

#[cfg(test)]
mod test {
    use super::*;
    use metrics::{register_counter, register_gauge, register_histogram};
    #[test]

    fn collect_json() {
        let mm_metrics = MmMetricsBuilder::new()
            .add_global_label("MarketMaker", "Metrics")
            .build()
            .unwrap();

        let handle = mm_metrics.install().unwrap();
        let counter1 = register_counter!("test_counter");
        counter1.increment(1);
        let counter2 = register_counter!("test_counter", "type" => "absolute");
        counter2.absolute(42);
        let gauge1 = register_gauge!("test_gauge", "g" => "test");
        gauge1.increment(1.4);
        let histogram1 = register_histogram!("test_histogram");
        histogram1.record(5.0);
        histogram1.record(1.);

        let expected: MetricsJson = serde_json::from_value(handle.collect_json().unwrap()).unwrap();
        println!("{:#?}", &expected);
    }
}
