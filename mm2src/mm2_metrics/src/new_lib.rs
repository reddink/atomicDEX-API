use crate::adapt::Inner;
use crate::adapt::MmRecorder;
use crate::Weak;
use metrics_util::registry::{GenerationalAtomicStorage, Registry};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;

#[cfg(target_arch = "wasm32")]
pub use crate::wasm::{Clock, MmRecorder};

pub trait ClockOpsNew {
    fn now(&self) -> u64;
}

#[derive(Clone, Default)]
pub struct MetricsArcNew(pub Arc<Mutex<MmRecorder>>);

impl MetricsArcNew {
    /// Create new `Metrics` instance
    pub fn new() -> MetricsArcNew {
        MetricsArcNew(Arc::new(Mutex::new(MmRecorder {
            inner: Arc::new(Mutex::new(Inner {
                registry: Registry::new(GenerationalAtomicStorage::atomic()),
            })),
        })))
    }

    /// Try to obtain the `Metrics` from the weak pointer.
    pub fn from_weak(weak: &MetricsWeakNew) -> Option<MetricsArcNew> { weak.0.upgrade().map(MetricsArcNew) }

    /// Create a weak pointer from `MetricsWeakNew`.
    pub fn weak(&self) -> MetricsWeakNew { MetricsWeakNew(Arc::downgrade(&self.0)) }
}

#[derive(Clone, Default)]
pub struct MetricsWeakNew(pub Weak<Mutex<MmRecorder>>);

impl MetricsWeakNew {
    /// Create a default MmWeak without allocating any memory.
    pub fn new() -> MetricsWeakNew { MetricsWeakNew::default() }

    pub fn dropped(&self) -> bool { self.0.strong_count() == 0 }
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

pub trait TryRecorder {
    fn try_recorder(&self) -> Option<MmRecorder>;
}

impl TryRecorder for MetricsArcNew {
    fn try_recorder(&self) -> Option<MmRecorder> { Some(self.0.lock().unwrap().to_owned()) }
}

impl TryRecorder for MetricsWeakNew {
    fn try_recorder(&self) -> Option<MmRecorder> {
        let metrics = MetricsArcNew::from_weak(self)?;
        let mut metrics = metrics.0.lock().unwrap();
        let metrics = metrics.try_recorder();
        if let Some(recorder) = metrics {
            return Some(recorder.clone());
        };
        None
    }
}
