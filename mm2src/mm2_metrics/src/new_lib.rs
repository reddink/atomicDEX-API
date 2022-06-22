use metrics::try_recorder;

use crate::adapt::Metrics;
use crate::adapt::MmRecorder;
use crate::adapt::PreparedMetric;
use crate::Weak;
use std::collections::HashMap;
use std::sync::Arc;

#[cfg(target_arch = "wasm32")]
pub use crate::wasm::{Clock, MmRecorder};

pub trait ClockOpsNew {
    fn now(&self) -> u64;
}

pub trait TryRecorder {
    fn try_recorder(&self) -> Option<Arc<MmRecorder>>;
}

#[derive(Clone)]
pub struct MetricsArc(pub(crate) Arc<Metrics>);

impl Default for MetricsArc {
    fn default() -> Self { Self::new() }
}

impl MetricsArc {
    pub fn new() -> Self { Self(Arc::new(Metrics::default())) }
    /// Try to obtain the `Metrics` from the weak pointer.
    pub fn from_weak(weak: &MetricsWeak) -> Option<MetricsArc> { weak.0.upgrade().map(MetricsArc) }

    /// Create a weak pointer from `MetricsWeak`.
    pub fn weak(&self) -> MetricsWeak { MetricsWeak(Arc::downgrade(&self.0)) }
}

impl TryRecorder for MetricsArc {
    fn try_recorder(&self) -> Option<Arc<MmRecorder>> {
        let recorder = Some(self.0.recorder.to_owned());
        if try_recorder().is_some() {
            return recorder;
        };
        None
    }
}

#[derive(Clone, Default)]
pub struct MetricsWeak(pub Weak<Metrics>);

impl MetricsWeak {
    /// Create a default MmWeak without allocating any memory.
    pub fn new() -> MetricsWeak { MetricsWeak::default() }

    pub fn dropped(&self) -> bool { self.0.strong_count() == 0 }
}

impl TryRecorder for MetricsWeak {
    fn try_recorder(&self) -> Option<Arc<MmRecorder>> {
        let metrics = MetricsArc::from_weak(self)?;
        if let Some(recorder) = metrics.try_recorder() {
            return Some(recorder);
        };
        None
    }
}

pub trait MetricsOps {
    /// Collect the metrics as Json.
    fn collect_json(&self) -> Result<crate::Json, String>;

    // Prepare metrics json for export
    fn prepare_tag_metrics(&self) -> Vec<PreparedMetric>;
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
