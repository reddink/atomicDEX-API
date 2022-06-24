use common::{executor::{spawn, Timer},
             log::{LogArc, LogWeak}};
use fomat_macros::wite;
use gstuff::ERRL;
use metrics::{IntoLabels, Key, KeyName, Label};
use metrics_core::ScopedString;
use serde_json::Value;
use std::{collections::HashMap, sync::Arc};

use crate::{common::log::Tag,
            new_lib::{MetricsJson, MetricsOps},
            recorder::MmRecorder};

/// Increment counter if an MmArc is not dropped yet and metrics system is initialized already.
#[macro_export]
macro_rules! mm_counter_new {
    ($metrics:expr, $name:expr, $value:expr) => {{
    use $crate::metrics::Recorder;
            if let Some(recorder) = $crate::new_lib::TryRecorder::try_recorder(&$metrics){
                let key = key_from_str($name);
                let counter = recorder.register_counter(&key);
                counter.increment($value);
            };
    }};

    // Register and increment counter with label
    ($metrics:expr, $name:expr, $value:expr, $($label_key:expr => $label_val:expr),+) => {{
         if let Some(recorder) = $crate::new_lib::TryRecorder::try_recorder(&$metrics){
    use $crate::metrics::Recorder;
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
    use $crate::metrics::Recorder;
            if let Some(recorder) = $crate::new_lib::TryRecorder::try_recorder(&$metrics){
            let key = key_from_str($name);
            let gauge = recorder.register_gauge(&key);
            gauge.increment($value);
        }
    }};

    // Register and increment gauge with label
    ($metrics:expr, $name:expr, $value:expr, $($label_key:expr => $label_val:expr),+) => {{
    use $crate::metrics::Recorder;
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
    use $crate::metrics::Recorder;
            if let Some(recorder) = $crate::new_lib::TryRecorder::try_recorder(&$metrics){
            let key = key_from_str($name);
            let histo = recorder.register_histogram(&key);
            histo.record($value);
        }

    }};

    // Register and record histogram with label
    ($metrics:expr, $name:expr, $value:expr, $($label_key:expr => $label_val:expr),+) => {{
         if let Some(recorder) = $crate::new_lib::TryRecorder::try_recorder(&$metrics){
    use $crate::metrics::Recorder;
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

/// Convert a slice of string to metric labels
pub fn from_slice_to_labels(labels: &[(&'static str, &'static str)]) -> Vec<Label> { labels.into_labels() }

/// Market Maker Metrics, used as inner to get metrics data and exporting
#[derive(Default, Clone)]
pub struct Metrics {
    pub recorder: Arc<MmRecorder>,
}

impl Metrics {
    pub fn init_with_dashboard(&self, log_state: LogWeak) -> Result<(), String> {
        let recorder = Arc::new(self.to_owned());
        let exporter = TagExporter::new(log_state, recorder);

        spawn(exporter.run(5.));

        Ok(())
    }
}

impl MetricsOps for Metrics {
    /// Collect prepared metrics json from the recorder
    fn collect_json(&self) -> Result<Value, String> {
        let value = JsonObserver::observe(&self.recorder);
        serde_json::to_value(value).map_err(|err| ERRL!("{}", err))
    }

    /// Collect prepared metrics tag from the recorder
    fn collect_tag_metrics(&self) -> Vec<PreparedMetric> { self.recorder.prepare_tag_metrics() }
}

/// Exports metrics by converting them to a Tag format and log them using log::Status.
pub struct TagExporter {
    /// Using a weak reference by default in order to avoid circular references and leaks.
    pub log_state: LogWeak,
    /// Handle for acquiring metric snapshots.
    pub metrics: Arc<Metrics>,
}

impl TagExporter {
    pub fn new(log_state: LogWeak, metrics: Arc<Metrics>) -> Self { Self { log_state, metrics } }

    /// Run endless async loop
    pub async fn run(self, interval: f64) {
        let controller = self.metrics;
        loop {
            Timer::sleep(interval).await;
            let log_state = match LogArc::from_weak(&self.log_state) {
                Some(x) => x,
                // MmCtx is dropped already
                _ => return,
            };

            log!(">>>>>>>>>> DEX metrics <<<<<<<<<");

            for PreparedMetric { tags, message } in controller.as_ref().collect_tag_metrics() {
                log_state.log_deref_tags("", tags, &message);
            }
        }
    }
}

struct JsonObserver;

impl JsonObserver {
    fn observe(recorder: &MmRecorder) -> MetricsJson { recorder.prepare_json() }
}

#[derive(Debug)]
#[allow(dead_code)]
pub struct PreparedMetric {
    pub tags: Vec<Tag>,
    pub message: String,
}

#[allow(dead_code)]
#[derive(Eq, PartialEq, PartialOrd, Ord)]
pub enum Integer {
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

pub type MetricName = ScopedString;

pub type MetricNameValueMap = HashMap<MetricName, Integer>;

#[cfg(test)]
mod test {

    use common::log::{LogArc, LogState};

    use crate::new_lib::MetricsArc;

    use super::*;
    #[test]

    fn collect_json() {
        let log_state = LogArc::new(LogState::in_memory());
        let mm_metrics = MetricsArc::new();
        mm_metrics.init_with_dashboard(log_state.weak()).unwrap();

        mm_counter_new!(mm_metrics, "mm_counter_new_test", 123);
        mm_counter_new!(mm_metrics, "counter", 3, "james" => "maker");
        mm_counter_new!(mm_metrics, "counter", 3, "james" => "maker");
        mm_gauge_new!(mm_metrics, "mm_gauge_new_test", 5.0);
        mm_gauge_new!(mm_metrics, "gauge", 3.0, "james" => "taker");
        mm_timing_new!(mm_metrics, "test.uptime", 1.0);
    }
}
