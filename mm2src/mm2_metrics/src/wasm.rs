use super::*;
use common::log::LogWeak;

/// The dummy macro that imitates [`crate::mm_metrics::native::mm_counter`].
/// These macros borrow the `$metrics`, `$name`, `$value` and takes ownership of the `$label_key`, `$label_val` to prevent the `unused_variable` warning.
/// The labels have to be moved because [`metrics_runtime::Sink::increment_counter_with_labels`] also takes ownership of the labels.
#[macro_export]
macro_rules! mm_counter {
    ($metrics:expr, $name:expr, $value:expr) => {{
        let _ = (&$metrics, &$name, &$value); // borrow
    }};
    ($metrics:expr, $name:expr, $value:expr, $($label_key:expr => $label_val:expr),+) => {{
        let _ = (&$metrics, &$name, &$value); // borrow
        let _ = ($($label_key, $label_val),+); // move
    }};
}

/// The dummy macro that imitates [`crate::mm_metrics::native::mm_gauge`].
/// These macros borrow the `$metrics`, `$name`, `$value` and takes ownership of the `$label_key`, `$label_val` to prevent the `unused_variable` warning.
/// The labels have to be moved because [`metrics_runtime::Sink::update_gauge_with_labels`] also takes ownership of the labels.
#[macro_export]
macro_rules! mm_gauge {
    ($metrics:expr, $name:expr, $value:expr) => {{
        let _ = (&$metrics, &$name, &$value); // borrow
    }};
    ($metrics:expr, $name:expr, $value:expr, $($label_key:expr => $label_val:expr),+) => {{
        let _ = (&$metrics, &$name, &$value); // borrow
        let _ = ($($label_key, $label_val),+); // move
    }};
}

/// The dummy macro that imitates [`crate::mm_metrics::native::mm_timing`].
/// These macros borrow the `$metrics`, `$name`, `$end` and takes ownership of the `$label_key`, `$label_val` to prevent the `unused_variable` warning.
/// The labels have to be moved because [`metrics_runtime::Sink::record_timing_with_labels`] also takes ownership of the labels.
#[macro_export]
macro_rules! mm_timing {
    ($metrics:expr, $name:expr, $end:expr) => {{
        let _ = (&$metrics, &$name, &$end); // borrow
    }};
    ($metrics:expr, $name:expr, $end:expr, $($label_key:expr => $label_val:expr),+) => {{
        let _ = (&$metrics, &$name, &$end); // borrow
        let _ = ($($label_key, $label_val),+); // move
    }};
}

#[derive(Default)]
pub struct MmRecorder {}

#[derive(Default)]
pub struct Metrics {
    pub recorder: Arc<MmRecorder>,
}

impl MetricsOps for Metrics {
    fn init(&self) {}

    fn init_with_dashboard(&self, _log_state: LogWeak, _record_interval: f64) -> MmMetricsResult<()> { Ok(()) }

    fn collect_json(&self) -> MmMetricsResult<Json> { Ok(Json::Array(Vec::new())) }
}

pub trait TryRecorder {
    fn try_recorder(&self) -> Option<Arc<MmRecorder>>;
}

impl TryRecorder for Metrics {
    fn try_recorder(&self) -> Option<Arc<MmRecorder>> { None }
}
