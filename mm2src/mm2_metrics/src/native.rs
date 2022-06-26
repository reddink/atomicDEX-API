use common::{executor::spawn, log::LogWeak};
use fomat_macros::wite;
use gstuff::ERRL;
use metrics::{IntoLabels, Key, KeyName, Label};
use metrics_core::ScopedString;
use metrics_exporter_prometheus::PrometheusBuilder;
use serde_json::Value;
use std::{collections::HashMap, io::Sink, sync::Arc};

use crate::{common::log::Tag, recorder::MmRecorder, ClockOps, MetricsOps};

const QUANTILES: &[f64] = &[0.0, 1.0];

/// Increment counter if an MmArc is not dropped yet and metrics system is initialized already.
#[macro_export]
macro_rules! mm_counter {
    ($metrics:expr, $name:expr, $value:expr) => {{
        use $crate::metrics::Recorder;
        if let Some(recorder) = $crate::TryRecorder::try_recorder(&$metrics){
            let key = key_from_str($name);
            let counter = recorder.register_counter(&key);
            counter.increment($value);
        };
    }};

    // Register and increment counter with label
    ($metrics:expr, $name:expr, $value:expr, $($label_key:expr => $label_val:expr),+) => {{
        use $crate::metrics::Recorder;
        use $crate::native::from_slice_to_labels;
        if let Some(recorder) = $crate::TryRecorder::try_recorder(&$metrics){
            let key = $crate::metrics::Key::from_parts($name, from_slice_to_labels(vec![$($label_key.to_owned(), $label_val.to_owned()),+]));
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
            let key = $crate::native::key_from_str($name);
            let gauge = recorder.register_gauge(&key);
            gauge.increment($value);
        }
    }};

    // Register and increment gauge with label
    ($metrics:expr, $name:expr, $value:expr, $($label_key:expr => $label_val:expr),+) => {{
        use $crate::metrics::Recorder;
        use $crate::native::from_slice_to_labels;
        if let Some(recorder) = $crate::TryRecorder::try_recorder(&$metrics){
            let key = $crate::metrics::Key::from_parts($name, from_slice_to_labels(vec![$($label_key.to_owned(), $label_val.to_owned()),+]));
            let gauge = recorder.register_gauge(&key);
            gauge.increment($value);
        }
    }};
}

/// Update gauge if an MmArc is not dropped yet and metrics system is initialized already.
#[macro_export]
macro_rules! mm_timing {
    ($metrics:expr, $name:expr, $value:expr) => {{
        use $crate::metrics::Recorder;
        use key_from_str;
        if let Some(recorder) = $crate::TryRecorder::try_recorder(&$metrics){
            let key = key_from_str($name);
            let histo = recorder.register_histogram(&key);
            histo.record($value);
        }
    }};

    // Register and record histogram with label
    ($metrics:expr, $name:expr, $value:expr, $($label_key:expr => $label_val:expr),+) => {{
        use $crate::metrics::Recorder;
        use $crate::native::from_slice_to_labels;
        if let Some(recorder) = $crate::TryRecorder::try_recorder(&$metrics){
            let key = $crate::metrics::Key::from_parts($name, from_slice_to_labels(vec![$($label_key.to_owned(), $label_val.to_owned()),+]));
            let histo = recorder.register_histogram(&key);
            histo.record($value);
        }
    }};
}

/// Convert a string to metrics Key
pub fn key_from_str(name: &'static str) -> Key {
    let key_name = KeyName::from_const_str(name);
    Key::from_name(key_name)
}

/// Convert a slice of string to metric labels
pub fn from_slice_to_labels(labels: Vec<String>) -> Vec<Label> {
    let labels = labels.to_vec();
    let mut new_lab: Vec<_> = vec![];
    let mut i = 0;
    for _ in 0..labels.len() - 1 {
        if i < labels.len() {
            let label = (labels[i].to_owned(), labels[i + 1].to_owned());
            new_lab.push(label);
            i += 2;
        }
    }

    new_lab.as_slice().into_labels()
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

    fn init_with_dashboard(&self, _log_state: LogWeak, _interval: f64) -> Result<(), String> {
        // work to do
        Ok(())
    }

    /// Collect prepared metrics json from the recorder
    fn collect_json(&self) -> Result<Value, String> {
        serde_json::to_value(self.recorder.prepare_json()).map_err(|err| ERRL!("{}", err))
    }
}

pub struct Clock {
    _sink: Sink,
}

impl ClockOps for Clock {
    fn now(&self) -> u64 { 0 }
}

#[derive(Debug)]
#[allow(dead_code)]
pub struct TagMetric {
    pub tags: Vec<Tag>,
    pub message: String,
}

pub type MetricName = ScopedString;

pub type MetricNameValueMap = HashMap<MetricName, Integer>;

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
                log!((err));
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
            log!((error));
            Response::builder().status(status).body(Body::empty()).map_err(|err| {
                log!((err));
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
                log!((err));
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

    use common::log::{LogArc, LogState};

    use crate::{MetricsArc, MetricsOps};

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

        mm_gauge!(metrics, "rpc.connection.count", 3 as f64, "coin" => "KMD");
        mm_gauge!(metrics, "rpc.connection.count", 5 as f64, "coin" => "KMD");

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
                    "value": 4620693217682128896 as u64
                }
            ]
        });

        let mut actual = metrics.collect_json().unwrap();
        // println!("{:#?}", actual);

        let actual = actual["metrics"].as_array_mut().unwrap();
        for expected in expected["metrics"].as_array().unwrap() {
            let index = actual.iter().position(|metric| metric == expected).expect(&format!(
                "Couldn't find expected metric: {:#?} \n in {:#?}",
                expected, actual
            ));
            println!("{}", index);
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
        mm_metrics.init_with_dashboard(log_state.weak(), 5.).unwrap();
        mm_counter!(mm_metrics, "rpc_client.request.count", 7649, "coin" => "tBCH", "client" => "eletrum");
        mm_counter!(mm_metrics, "rpc_client.request.count", 7615, "coin" => "tBCH", "client" => "eletrum");
        mm_counter!(mm_metrics, "rpc_client.request.in", 335206, "coin" => "tBCH", "client" => "eletrum");
        mm_counter!(mm_metrics, "rpc_client.request.out", 700604, "coin" => "tBCH", "client" => "eletrum");
        mm_counter!(mm_metrics, "rpc_client.request.count", 6345, "coin" => "tBCH", "client" => "eletrum");
        mm_counter!(mm_metrics, "rpc_client.request.count", 6345, "coin" => "tBCH", "client" => "eletrum");
        mm_gauge!(mm_metrics, "rpc_client.request.in", 6345.0, "coin" => "tBCH", "client" => "eletrum");
        mm_gauge!(mm_metrics, "rpc_client.request.out", 633840.0, "coin" => "tBCH", "client" => "eletrum");
        // println!("{:#?}", mm_metrics.collect_tag_metrics())
    }
}
