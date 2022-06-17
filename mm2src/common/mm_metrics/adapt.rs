use std::{collections::HashMap,
          slice::Iter,
          sync::{atomic::Ordering, Arc}};

use crate::{executor::{spawn, Timer},
            log::{LogArc, LogWeak},
            Json};
use indexmap::IndexMap;
use metrics::{KeyName, Recorder, Unit};
use metrics_exporter_prometheus::{formatting::{key_to_parts, sanitize_metric_name, write_help_line,
                                               write_metric_line, write_type_line},
                                  Distribution, DistributionBuilder};
use metrics_util::registry::{GenerationalAtomicStorage, Registry};
use parking_lot::RwLock;
use serde_json as json;

use crate::log::Tag;

use super::{MetricType, MetricsJson};

pub struct Snapshot {
    pub counters: HashMap<String, HashMap<Vec<String>, u64>>,
    pub gauges: HashMap<String, HashMap<Vec<String>, f64>>,
    pub distributions: HashMap<String, IndexMap<Vec<String>, Distribution>>,
}

pub(crate) struct Inner {
    pub registry: Registry<metrics::Key, metrics_util::registry::GenerationalAtomicStorage>,
    pub descriptions: RwLock<HashMap<String, &'static str>>,
    pub distributions: RwLock<HashMap<String, IndexMap<Vec<String>, Distribution>>>,
    pub distribution_builder: DistributionBuilder,
    pub global_labels: IndexMap<String, String>,
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

        let histogram_handles = self.registry.get_histogram_handles();
        for (key, histogram) in histogram_handles {
            let (name, labels) = key_to_parts(&key, Some(&self.global_labels));

            let mut wg = self.distributions.write();
            let entry = wg
                .entry(name.clone())
                .or_insert_with(IndexMap::new)
                .entry(labels)
                .or_insert_with(|| self.distribution_builder.get_distribution(name.as_str()));

            histogram
                .get_inner()
                .clear_with(|samples| entry.record_samples(samples));
        }

        let distributions = self.distributions.read().clone();

        Snapshot {
            counters,
            gauges,
            distributions,
        }
    }

    fn render(&self) -> String {
        let Snapshot {
            mut counters,
            mut gauges,
            mut distributions,
        } = self.get_recent_metrics();

        let mut output = String::new();
        let descriptions = self.descriptions.read();

        for (name, mut by_labels) in counters.drain() {
            if let Some(desc) = descriptions.get(name.as_str()) {
                write_help_line(&mut output, name.as_str(), desc);
            }

            write_type_line(&mut output, name.as_str(), "counter");
            for (labels, value) in by_labels.drain() {
                write_metric_line::<&str, u64>(&mut output, &name, None, &labels, None, value);
            }
            output.push('\n');
        }

        for (name, mut by_labels) in gauges.drain() {
            if let Some(desc) = descriptions.get(name.as_str()) {
                write_help_line(&mut output, name.as_str(), desc);
            }

            write_type_line(&mut output, name.as_str(), "gauge");
            for (labels, value) in by_labels.drain() {
                write_metric_line::<&str, f64>(&mut output, &name, None, &labels, None, value);
            }
            output.push('\n');
        }

        for (name, mut by_labels) in gauges.drain() {
            if let Some(desc) = descriptions.get(name.as_str()) {
                write_help_line(&mut output, name.as_str(), desc);
            }

            write_type_line(&mut output, name.as_str(), "gauge");
            for (labels, value) in by_labels.drain() {
                write_metric_line::<&str, f64>(&mut output, &name, None, &labels, None, value);
            }
            output.push('\n');
        }

        output
    }

    fn prepare_metrics(&self) -> Vec<PreparedMetric> {
        let Snapshot {
            mut counters,
            mut gauges,
            mut distributions,
        } = self.get_recent_metrics();

        let mut output: Vec<PreparedMetric> = vec![];

        for (name, mut by_labels) in counters.drain() {
            for (_labels, value) in by_labels.drain() {
                output.push(PreparedMetric {
                    tags: labels_to_tags(metrics::Key::from_name(name.clone()).labels()),
                    message: format!("{} {}", name, value),
                });
            }
        }

        for (name, mut by_labels) in gauges.drain() {
            for (_labels, value) in by_labels.drain() {
                output.push(PreparedMetric {
                    tags: labels_to_tags(metrics::Key::from_name(name.clone()).labels()),
                    message: format!("{} {}", name, value),
                });
            }
        }

        output
    }

    fn collect_json(&self) -> Result<Json, String> {
        let Snapshot {
            mut counters,
            mut gauges,
            mut distributions,
        } = self.get_recent_metrics();

        let mut output: Vec<MetricType> = vec![];

        for (name, mut by_labels) in counters.drain() {
            for (labels, value) in by_labels.drain() {
                output.push(MetricType::Counter {
                    key: name.clone(),
                    labels,
                    value,
                });
            }
        }

        for (name, mut by_labels) in gauges.drain() {
            for (labels, value) in by_labels.drain() {
                output.push(MetricType::Counter {
                    key: name.clone(),
                    labels,
                    value: value as u64,
                });
            }
        }

        json::to_value(MetricsJson { metrics: output }).map_err(|err| ERRL!("{}", err))
    }
}
#[derive(Clone)]
pub struct MmRecorder {
    inner: Arc<Inner>,
}

impl MmRecorder {
    fn new() -> Self {
        let quantiles = metrics_util::parse_quantiles(&[0.0, 0.5, 0.9, 0.95, 0.99, 0.999, 1.0]);
        Self {
            inner: Arc::new(Inner {
                registry: Registry::new(GenerationalAtomicStorage::atomic()),
                descriptions: RwLock::new(HashMap::new()),
                distributions: Default::default(),
                distribution_builder: DistributionBuilder::new(quantiles, None, None),
                global_labels: Default::default(),
            }),
        }
    }

    pub fn handle(&self) -> MmHandle {
        MmHandle {
            inner: self.inner.clone(),
        }
    }
    fn add_description_if_missing(&self, key_name: &KeyName, description: &'static str) {
        let sanitized = sanitize_metric_name(key_name.as_str());
        let mut descriptions = self.inner.descriptions.write();
        descriptions.entry(sanitized).or_insert(description);
    }
}
impl From<Inner> for MmRecorder {
    fn from(inner: Inner) -> Self { MmRecorder { inner: Arc::new(inner) } }
}

impl Recorder for MmRecorder {
    fn describe_counter(&self, key_name: KeyName, _unit: Option<Unit>, description: &'static str) {
        self.add_description_if_missing(&key_name, description)
    }

    fn describe_gauge(&self, key_name: KeyName, _unit: Option<Unit>, description: &'static str) {
        self.add_description_if_missing(&key_name, description)
    }

    fn describe_histogram(&self, key_name: KeyName, _unit: Option<Unit>, description: &'static str) {
        self.add_description_if_missing(&key_name, description)
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
    inner: Arc<Inner>,
}
impl MmHandle {
    pub fn render(&self) -> String { self.inner.render() }
    pub fn collect_json(&self) -> Result<Json, String> { self.inner.collect_json() }
    pub fn prepare_metrics(&self) -> Vec<PreparedMetric> { self.inner.prepare_metrics() }
    pub fn export_tags(&self, log_state: LogWeak, interval: f64) -> Result<(), String> {
        let exporter = TagExporter {
            log_state,
            inner: Arc::clone(&self.inner),
        };

        spawn(exporter.run(interval));

        Ok(())
    }
}

pub struct PreparedMetric {
    tags: Vec<Tag>,
    message: String,
}

struct TagExporter {
    log_state: LogWeak,
    inner: Arc<Inner>,
}

impl TagExporter {
    async fn run(mut self, interval: f64) {
        loop {
            Timer::sleep(interval).await;
            self.turn();
        }
    }

    fn turn(&mut self) {
        println!("run");

        let log_state = match LogArc::from_weak(&self.log_state) {
            Some(x) => x,
            // MmCtx is dropped already
            _ => {
                return;
            },
        };

        log!(">>>>>>>>>> DEX metrics <<<<<<<<<");

        for PreparedMetric { tags, message } in self.inner.prepare_metrics() {
            log_state.log_deref_tags("", tags, &message);
        }
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

#[cfg(test)]
mod test {
    use metrics::Label;

    use super::*;
    #[test]

    fn collect_json() {
        let metrics = MmRecorder::new();

        let labels = vec![Label::new("hey", "come")];
        let key = metrics::Key::from_parts("basic_ecounter", labels);
        metrics.register_counter(&key);
        let labels = vec![Label::new("wutang", "forever")];
        let key = metrics::Key::from_parts("basic_gauge", labels);
        metrics.register_gauge(&key);
        let json = metrics.handle().collect_json();
        println!("{:?}", json)
    }

    fn _export_tags() {
        let log_state = LogArc::new(crate::log::LogState::in_memory());
        let metrics = MmRecorder::new();
        let labels = vec![Label::new("hey", "come")];
        let key = metrics::Key::from_parts("basic_ecounter", labels);
        metrics.register_counter(&key);
        let labels = vec![Label::new("wutang", "forever")];
        let key = metrics::Key::from_parts("basic_gauge", labels);
        metrics.register_counter(&key);
        metrics.handle().export_tags(log_state.weak(), 5.).unwrap();
    }
}
