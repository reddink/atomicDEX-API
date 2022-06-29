#[cfg(not(target_arch = "wasm32"))]
#[macro_use]
extern crate gstuff;
#[macro_use] extern crate serde_derive;
#[cfg(not(target_arch = "wasm32"))]
#[macro_use]
extern crate common;

use serde_json::Value as Json;
use std::collections::HashMap;
use std::sync::{Arc, Weak};

#[cfg(not(target_arch = "wasm32"))] pub mod recorder;
#[cfg(not(target_arch = "wasm32"))] pub use metrics;
#[cfg(not(target_arch = "wasm32"))] use metrics::try_recorder;
#[cfg(not(target_arch = "wasm32"))] pub use recorder::MmRecorder;
#[cfg(not(target_arch = "wasm32"))] pub mod native;
#[cfg(not(target_arch = "wasm32"))]
pub use native::{prometheus, Clock, Metrics, PreparedMetric};

#[cfg(target_arch = "wasm32")] mod wasm;
#[cfg(target_arch = "wasm32")]
pub use wasm::{try_recorder, Clock, Metrics, MmRecorder};

pub trait MetricsOps {
    fn init(&self) -> Result<(), String>
    where
        Self: Sized;

    fn init_with_dashboard(&self, log_state: common::log::LogWeak, interval: f64) -> Result<(), String>;

    /// Collect the metrics as Json.
    fn collect_json(&self) -> Result<crate::Json, String>;
}

pub trait ClockOps {
    fn now(&self) -> u64;
}

pub trait TryRecorder {
    fn try_recorder(&self) -> Option<Arc<MmRecorder>>;
}

#[derive(Clone)]
pub struct MetricsArc(pub(crate) Arc<Metrics>);

impl Default for MetricsArc {
    fn default() -> Self { Self(Arc::new(Metrics::default())) }
}

impl MetricsArc {
    // Create new instance of our metrics recorder set to default
    pub fn new() -> Self { Self(Default::default()) }

    /// Try to obtain the `Metrics` from the weak pointer.
    pub fn from_weak(weak: &MetricsWeak) -> Option<MetricsArc> { weak.0.upgrade().map(MetricsArc) }

    /// Create a weak pointer from `MetricsWeak`.
    pub fn weak(&self) -> MetricsWeak { MetricsWeak(Arc::downgrade(&self.0)) }
}

impl TryRecorder for MetricsArc {
    fn try_recorder(&self) -> Option<Arc<MmRecorder>> {
        if try_recorder().is_some() {
            return None;
        }
        Some(self.0.recorder.to_owned())
    }
}

impl MetricsOps for MetricsArc {
    fn init(&self) -> Result<(), String> {
        self.0.init().unwrap();
        Ok(())
    }

    fn init_with_dashboard(&self, log_state: common::log::LogWeak, interval: f64) -> Result<(), String> {
        self.0.init_with_dashboard(log_state, interval).unwrap();
        Ok(())
    }

    fn collect_json(&self) -> Result<crate::Json, String> { self.0.collect_json() }
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

#[derive(Serialize, Debug, Default, Deserialize)]
pub struct MetricsJson {
    pub metrics: Vec<MetricType>,
}

#[derive(Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
#[serde(tag = "type")]
pub enum MetricType {
    Counter {
        key: String,
        labels: HashMap<String, String>,
        value: u64,
    },
    Gauge {
        key: String,
        labels: HashMap<String, String>,
        value: f64,
    },
    Histogram {
        key: String,
        labels: HashMap<String, String>,
        #[serde(flatten)]
        quantiles: HashMap<String, u64>,
    },
}
