use common::log::error;
use common::{executor::{spawn, Timer},
             log::{LogArc, LogWeak}};
use gstuff::ERRL;
use hdrhistogram::Histogram;
use itertools::Itertools;
use metrics::{Key, KeyName, Label};
use metrics_exporter_prometheus::PrometheusBuilder;
use metrics_util::{parse_quantiles, Quantile};
use serde_json::Value;
use std::{collections::HashMap,
          fmt::Display,
          sync::{atomic::Ordering, Arc}};

use crate::{common::log::Tag,
            recorder::{labels_to_tags, name_value_map_to_message},
            MetricsOps, MmRecorder};

const QUANTILES: &[f64] = &[0.0, 1.0];

type MetricLabels = Vec<Label>;

pub type MetricNameValueMap = HashMap<String, PreparedMetric>;

/// Increment counter if an MmArc is not dropped yet and metrics system is initialized already.
#[macro_export]
macro_rules! mm_counter {
    ($metrics:expr, $name:expr, $value:expr) => {{
        use $crate::metrics::Recorder;
        if let Some(recorder) = $crate::TryRecorder::try_recorder(&$metrics){
            let key =$crate::metrics::Key::from_static_name($name);
            let counter = recorder.register_counter(&key);
            counter.increment($value);
        };
    }};

    // Register and increment counter with label
    ($metrics:expr, $name:expr, $value:expr, $($label_key:expr => $label_val:expr),+) => {{
        use $crate::metrics::Recorder;
        // use $crate::native::from_slice_to_labels;
        if let Some(recorder) = $crate::TryRecorder::try_recorder(&$metrics){
            let key = $crate::metrics::Key::from_parts($name, vec![$(($label_key.to_owned(), $label_val.to_owned())),+].as_slice());
            let counter = recorder.register_counter(&key);
            counter.increment($value);
        };
    }};
}

/// Update gauge if an MmArc is not dropped yet and metrics system is initialized already.
#[macro_export]
macro_rules! mm_gauge {
    ($metrics:expr, $name:expr, $value:expr) => {{
        use $crate::metrics::Recorder;
        if let Some(recorder) = $crate::TryRecorder::try_recorder(&$metrics){
            let key =$crate::metrics::Key::from_static_name($name);
            let gauge = recorder.register_gauge(&key);
            gauge.set($value);
        }
    }};

    // Register and set gauge with label
    ($metrics:expr, $name:expr, $value:expr, $($label_key:expr => $label_val:expr),+) => {{
        use $crate::metrics::Recorder;
        // use $crate::native::from_slice_to_labels;
        if let Some(recorder) = $crate::TryRecorder::try_recorder(&$metrics){
            let key = $crate::metrics::Key::from_parts($name, vec![$(($label_key.to_owned(), $label_val.to_owned())),+].as_slice());
            let gauge = recorder.register_gauge(&key);
            gauge.set($value);
        }
    }};
}

/// Update gauge if an MmArc is not dropped yet and metrics system is initialized already.
#[macro_export]
macro_rules! mm_timing {
    ($metrics:expr, $name:expr, $value:expr) => {{
        use $crate::metrics::Recorder;
        use $crate::native::key_from_str;
        if let Some(recorder) = $crate::TryRecorder::try_recorder(&$metrics){
            let key =$crate::metrics::Key::from_static_name($name);
            let histo = recorder.register_histogram(&key);
            histo.record($value);
        }
    }};

    // Register and record histogram with label
    ($metrics:expr, $name:expr, $value:expr, $($label_key:expr => $label_val:expr),+) => {{
        use $crate::metrics::Recorder;
        // use $crate::native::from_slice_to_labels;
        if let Some(recorder) = $crate::TryRecorder::try_recorder(&$metrics){
            let key = $crate::metrics::Key::from_parts($name, vec![$(($label_key.to_owned(), $label_val.to_owned())),+].as_slice());
            let histo = recorder.register_histogram(&key);
            histo.record($value);
        }
    }};
}

/// Market Maker Metrics, used as inner to get metrics data and exporting
#[derive(Default, Clone)]
pub struct Metrics {
    pub recorder: Arc<MmRecorder>,
}

impl Metrics {
    /// Collect the metrics in Prometheus format.
    pub fn collect_prometheus_format(&self) -> Result<String, String> {
        let prometheus = PrometheusBuilder::new()
            .set_quantiles(QUANTILES)
            .unwrap()
            .build()
            .unwrap();

        Ok(prometheus.0.handle().render())
    }
}

impl MetricsOps for Metrics {
    fn init(&self) -> Result<(), String> {
        Metrics::default();
        Ok(())
    }

    fn init_with_dashboard(&self, log_state: LogWeak, interval: f64) -> Result<(), String> {
        let recorder = self.recorder.clone();
        let runner = TagObserver::log_tag_metrics(log_state, recorder, interval);
        spawn(runner);

        Ok(())
    }

    /// Collect prepared metrics json from the recorder
    fn collect_json(&self) -> Result<Value, String> {
        serde_json::to_value(self.recorder.prepare_json()).map_err(|err| ERRL!("{}", err))
    }
}

#[derive(Debug)]
#[allow(dead_code)]
pub struct TagMetric {
    pub tags: Vec<Tag>,
    pub message: String,
}

#[derive(PartialEq, PartialOrd)]
pub enum PreparedMetric {
    Unsigned(u64),
    Float(f64),
}

impl Display for PreparedMetric {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PreparedMetric::Unsigned(e) => write!(f, "{}", e),
            PreparedMetric::Float(e) => write!(f, "{}", e),
        }
    }
}

pub struct TagObserver;

impl TagObserver {
    pub async fn log_tag_metrics(log_state: LogWeak, recorder: Arc<MmRecorder>, interval: f64) {
        loop {
            Timer::sleep(interval).await;
            let log_state = match LogArc::from_weak(&log_state) {
                Some(la) => la,
                _ => {
                    return;
                },
            };

            log!(">>>>>>>>>> DEX metrics <<<<<<<<<");

            Self::prepare_tag_metrics(&recorder)
                .into_iter()
                .for_each(|(labels, name_value_map)| {
                    let tags = labels_to_tags(labels.iter());
                    let message = name_value_map_to_message(&name_value_map);
                    log_state.log_deref_tags("", tags, &message);
                });

            let quantiles = parse_quantiles(QUANTILES);
            Self::prepare_tag_histograms(&recorder).into_iter().for_each(|histo| {
                histo.into_iter().for_each(|(key, hist)| {
                    let tags = labels_to_tags(Key::from_name(KeyName::from(key.clone())).labels());
                    let message = format!("{}: {}", key, hist_to_message(quantiles.as_slice(), hist));
                    log_state.log_deref_tags("", tags, &message);
                });
            });
        }
    }

    fn prepare_tag_metrics(recorder: &MmRecorder) -> HashMap<MetricLabels, MetricNameValueMap> {
        let mut output = HashMap::new();

        for (key, counter) in recorder.registry.get_counter_handles() {
            let value = counter.get_inner().load(Ordering::Acquire);
            map_metrics_to_prepare_tag_metric_output(key, PreparedMetric::Unsigned(value), &mut output);
        }

        for (key, gauge) in recorder.registry.get_gauge_handles() {
            let value = f64::from_bits(gauge.get_inner().load(Ordering::Acquire));
            map_metrics_to_prepare_tag_metric_output(key, PreparedMetric::Float(value), &mut output);
        }
        output
    }

    fn prepare_tag_histograms(recorder: &MmRecorder) -> Option<HashMap<String, Histogram<u64>>> {
        let mut output = None;
        for (key, histogram) in recorder.registry.get_histogram_handles() {
            let value = histogram.get_inner().data();
            output = prepared_historam(key, value);
        }
        output
    }
}

fn prepared_historam(key: Key, values: Vec<f64>) -> Option<HashMap<String, Histogram<u64>>> {
    let mut histograms = HashMap::new();
    let entry = histograms.entry(key.name().to_string()).or_insert({
        // Use default significant figures value.
        // For more info on `sigfig` see the Historgam::new_with_bounds().
        let sigfig = 3;
        match Histogram::<u64>::new(sigfig) {
            Ok(x) => x,
            Err(err) => {
                error!("failed to create histogram: {}", err);
                // do nothing on error
                return None;
            },
        }
    });

    for value in values {
        if let Err(err) = entry.record(value as u64) {
            error!("failed to observe histogram value: {}", err);
        }
    }
    Some(histograms)
}

fn map_metrics_to_prepare_tag_metric_output(
    key: Key,
    value: PreparedMetric,
    output: &mut HashMap<MetricLabels, MetricNameValueMap>,
) {
    let (metric_name, labels) = key.into_parts();
    output
        .entry(labels)
        .or_insert_with(MetricNameValueMap::new)
        .insert(metric_name.as_str().to_string(), value);
}

fn hist_to_message(quantiles: &[Quantile], histogram: Histogram<u64>) -> String {
    if quantiles.is_empty() {
        return format!("count={}", histogram.len());
    }

    let fmt_quantiles = quantiles
        .iter()
        .map(|quantile| {
            let key = quantile.label().to_string();
            let val = histogram.value_at_quantile(quantile.value());
            format!("{}={}", key, val)
        })
        .join(" ");
    format!("count={} {}", histogram.len(), fmt_quantiles)
}

pub mod prometheus {
    use crate::{MetricsArc, MetricsWeak};

    use super::*;
    use futures::future::{Future, FutureExt};
    use hyper::http::{self, header, Request, Response, StatusCode};
    use hyper::service::{make_service_fn, service_fn};
    use hyper::{Body, Server};
    use std::convert::Infallible;
    use std::net::SocketAddr;

    #[derive(Clone)]
    pub struct PrometheusCredentials {
        pub userpass: String,
    }

    pub fn spawn_prometheus_exporter(
        metrics: MetricsWeak,
        address: SocketAddr,
        shutdown_detector: impl Future<Output = ()> + 'static + Send,
        credentials: Option<PrometheusCredentials>,
    ) -> Result<(), String> {
        let make_svc = make_service_fn(move |_conn| {
            let metrics = metrics.clone();
            let credentials = credentials.clone();
            futures::future::ready(Ok::<_, Infallible>(service_fn(move |req| {
                futures::future::ready(scrape_handle(req, metrics.clone(), credentials.clone()))
            })))
        });

        let server = try_s!(Server::try_bind(&address))
            .http1_half_close(false) // https://github.com/hyperium/hyper/issues/1764
            .serve(make_svc)
            .with_graceful_shutdown(shutdown_detector);

        let server = server.then(|r| {
            if let Err(err) = r {
                error!("{}", err);
            };
            futures::future::ready(())
        });

        spawn(server);
        Ok(())
    }

    fn scrape_handle(
        req: Request<Body>,
        metrics: MetricsWeak,
        credentials: Option<PrometheusCredentials>,
    ) -> Result<Response<Body>, http::Error> {
        fn on_error(status: StatusCode, error: String) -> Result<Response<Body>, http::Error> {
            error!("{}", error);
            Response::builder().status(status).body(Body::empty()).map_err(|err| {
                error!("{}", err);
                err
            })
        }

        if req.uri() != "/metrics" {
            return on_error(
                StatusCode::BAD_REQUEST,
                ERRL!("Warning Prometheus: unexpected URI {}", req.uri()),
            );
        }

        if let Some(credentials) = credentials {
            if let Err(err) = check_auth_credentials(&req, credentials) {
                return on_error(StatusCode::UNAUTHORIZED, err);
            }
        }

        let metrics = match MetricsArc::from_weak(&metrics) {
            Some(m) => m,
            _ => {
                return on_error(
                    StatusCode::BAD_REQUEST,
                    ERRL!("Warning Prometheus: metrics system unavailable"),
                )
            },
        };

        let body = match metrics.0.collect_prometheus_format() {
            Ok(body) => Body::from(body),
            _ => {
                return on_error(
                    StatusCode::BAD_REQUEST,
                    ERRL!("Warning Prometheus: metrics system is not initialized yet"),
                )
            },
        };

        Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "text/plain")
            .body(body)
            .map_err(|err| {
                error!("{}", err);
                err
            })
    }

    fn check_auth_credentials(req: &Request<Body>, expected: PrometheusCredentials) -> Result<(), String> {
        let header_value = req
            .headers()
            .get(header::AUTHORIZATION)
            .ok_or(ERRL!("Warning Prometheus: authorization required"))
            .and_then(|header| Ok(try_s!(header.to_str())))?;

        let expected = format!("Basic {}", base64::encode_config(&expected.userpass, base64::URL_SAFE));

        if header_value != expected {
            return Err(format!("Warning Prometheus: invalid credentials: {}", header_value));
        }

        Ok(())
    }
}

#[cfg(test)]
mod test {

    use crate::{MetricsArc, MetricsOps};

    use common::{block_on,
                 executor::Timer,
                 log::{LogArc, LogState}};

    #[test]
    fn test_collect_json() {
        let metrics = MetricsArc::new();

        metrics.init().unwrap();

        mm_counter!(metrics, "rpc.traffic.tx", 62, "coin" => "BTC");
        mm_counter!(metrics, "rpc.traffic.rx", 105, "coin" => "BTC");

        mm_counter!(metrics, "rpc.traffic.tx", 30, "coin" => "BTC");
        mm_counter!(metrics, "rpc.traffic.rx", 44, "coin" => "BTC");

        mm_counter!(metrics, "rpc.traffic.tx", 54, "coin" => "KMD");
        mm_counter!(metrics, "rpc.traffic.rx", 158, "coin" => "KMD");

        mm_gauge!(metrics, "rpc.connection.count", 3.0, "coin" => "KMD");
        mm_gauge!(metrics, "rpc.connection.count", 5.0, "coin" => "KMD");

        // mm_timing!(metrics,
        //            "rpc.query.spent_time",
        //            // ~ 1 second
        //            34381019796149, // start
        //            34382022725155, // end
        //            "coin" => "KMD",
        //            "method" => "blockchain.transaction.get");
        //
        // mm_timing!(metrics,
        //            "rpc.query.spent_time",
        //            // ~ 2 second
        //            34382022774105, // start
        //            34384023173373, // end
        //            "coin" => "KMD",
        //            "method" => "blockchain.transaction.get");

        let expected = serde_json::json!({
            "metrics": [
                {
                    "key": "rpc.traffic.rx",
                    "labels": { "coin": "BTC" },
                    "type": "counter",
                    "value": 149
                },
                {
                    "key": "rpc.traffic.tx",
                    "labels": { "coin": "KMD" },
                    "type": "counter",
                    "value": 54
                },
                {
                    "key": "rpc.traffic.tx",
                    "labels": { "coin": "BTC" },
                    "type": "counter",
                    "value": 92
                },
                {
                    "key": "rpc.traffic.rx",
                    "labels": { "coin": "KMD" },
                    "type": "counter",
                    "value": 158
                },
                // {
                //     "count": 2,
                //     "key": "rpc.query.spent_time",
                //     "labels": { "coin": "KMD", "method": "blockchain.transaction.get" },
                //     "max": 2000683007,
                //     "min": 1002438656,
                //     "type": "histogram"
                // },
                {
                    "key": "rpc.connection.count",
                    "labels": { "coin": "KMD" },
                    "type": "gauge",
                    "value": 5.0
                }
            ]
        });

        let mut actual = metrics.collect_json().unwrap();
        let actual = actual["metrics"].as_array_mut().unwrap();
        for expected in expected["metrics"].as_array().unwrap() {
            let index = actual.iter().position(|metric| metric == expected).expect(&format!(
                "Couldn't find expected metric: {:#?} \n in {:#?}",
                expected, actual
            ));
            actual.remove(index);
        }

        assert!(
            actual.is_empty(),
            "More metrics collected than expected. Excess metrics: {:?}",
            actual
        );
    }

    #[test]
    fn collect_tag_metrics() {
        let log_state = LogArc::new(LogState::in_memory());
        let mm_metrics = MetricsArc::new();

        mm_metrics.init_with_dashboard(log_state.weak(), 6.).unwrap();

        mm_counter!(mm_metrics, "rpc_client.request.count", 1, "coin" => "tBCH", "client" => "eletrum");
        mm_counter!(mm_metrics, "rpc_client.request.count", 1, "coin" => "tBCH", "client" => "eletrum");
        mm_timing!(mm_metrics, "rpc_client.request.out", 3.0, "coin" => "tBCH", "client" => "eletrum");
        block_on(async { Timer::sleep(6.).await });
        mm_gauge!(mm_metrics, "rpc_client.request.in", 2.0, "coin" => "tBCH", "client" => "eletrum");
        mm_counter!(mm_metrics, "rpc_client.request.out", 3, "coin" => "tBCH", "client" => "eletrum");
        mm_counter!(mm_metrics, "rpc_client.request.count", 1, "coin" => "tBCH", "client" => "eletrum");
        mm_counter!(mm_metrics, "rpc_client.request.count", 1, "coin" => "tBCH", "client" => "eletrum");
        mm_gauge!(mm_metrics, "rpc_client.request.in", 6345.0, "coin" => "tBCH", "client" => "eletrum");
        mm_gauge!(mm_metrics, "rpc_client.request.out", 2.0, "coin" => "tBCH", "client" => "eletrum");
        mm_timing!(mm_metrics, "peer.outgoing_request.timing", 0.4, "peer" => "peer");
        block_on(async { Timer::sleep(6.).await });
    }
}
