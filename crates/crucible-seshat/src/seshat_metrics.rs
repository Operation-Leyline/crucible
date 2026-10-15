use crucible_server_runtime_rust::middlewares::{Recorder, RequestRecord};

use crate::{METRIC_EVENT, METRIC_TARGET};

/// A `Metrics` recorder that counts every request as one increment of a
/// Seshat counter. The line carries `operation` (absent when no operation
/// served the request), `class` (`success`, `error` or `fault`) and `status`;
/// the service's `seshat.json` declares the counter and picks which of them
/// become CloudWatch dimensions.
///
/// ```ignore
/// use crucible_server_runtime_rust::middlewares::Metrics;
/// builder.middleware(Metrics::new(crucible_seshat::SeshatMetrics::new("Requests")))
/// ```
#[derive(Clone, Copy, Debug)]
pub struct SeshatMetrics {
    counter: &'static str,
}

impl SeshatMetrics {
    /// `counter` is the name declared under `metrics.counters` in `seshat.json`.
    pub fn new(counter: &'static str) -> Self {
        SeshatMetrics { counter }
    }

    pub(crate) fn emit(&self, operation: Option<&str>, class: &str, status: u16) {
        tracing::info!(
            target: METRIC_TARGET,
            event = METRIC_EVENT,
            metric = self.counter,
            value = 1u64,
            operation,
            class,
            status,
        );
    }
}

impl Default for SeshatMetrics {
    fn default() -> Self {
        SeshatMetrics::new("Requests")
    }
}

impl Recorder for SeshatMetrics {
    fn record(&self, record: &RequestRecord<'_>) {
        self.emit(
            record.operation.map(|o| o.name),
            record.class,
            record.status,
        );
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write;
    use std::sync::{Arc, Mutex};

    use serde_json::{Value, json};
    use tracing::Level;

    use super::SeshatMetrics;
    use crate::fields::RequestFields;

    #[derive(Clone, Default)]
    struct Buffer(Arc<Mutex<Vec<u8>>>);

    impl Write for Buffer {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    /// The lines a flattening JSON subscriber prints, as Seshat reads them.
    fn lines(f: impl FnOnce()) -> Vec<Value> {
        let buffer = Buffer::default();
        let writer = buffer.clone();
        let subscriber = tracing_subscriber::fmt()
            .json()
            .flatten_event(true)
            .with_max_level(Level::TRACE)
            .with_writer(move || writer.clone())
            .finish();
        tracing::subscriber::with_default(subscriber, f);
        let text = String::from_utf8(buffer.0.lock().unwrap().clone()).unwrap();
        text.lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect()
    }

    #[test]
    fn a_request_line_uses_the_contract_names() {
        let lines = lines(|| {
            RequestFields {
                level: Level::WARN,
                request_id: "r1",
                operation: "GetCity",
                status: 404,
                outcome: "error",
                latency_ms: 1.5,
                error: Some("NoSuchCity"),
                cause: None,
                input: Some(r#"{"cityId":"x"}"#.into()),
                output: None,
                error_fields: None,
                violations: None,
            }
            .emit();
        });
        let line = &lines[0];
        assert_eq!(line["event"], "crucible.request");
        assert_eq!(line["level"], "WARN");
        assert_eq!(line["target"], "crucible::request");
        assert_eq!(line["message"], "request completed");
        assert_eq!(line["request_id"], "r1");
        assert_eq!(line["operation"], "GetCity");
        assert_eq!(line["status"], 404);
        assert_eq!(line["outcome"], "error");
        assert_eq!(line["latency_ms"], 1.5);
        assert_eq!(line["error"], "NoSuchCity");
        assert_eq!(line["input"], r#"{"cityId":"x"}"#);
        assert!(line.get("cause").is_none() && line.get("output").is_none());
    }

    #[test]
    fn a_counter_line_is_one_increment_with_its_dimensions() {
        let lines = lines(|| {
            SeshatMetrics::new("Requests").emit(Some("GetCity"), "success", 200);
            SeshatMetrics::default().emit(None, "error", 404);
        });
        assert_eq!(lines[0]["event"], "seshat.metric");
        assert_eq!(lines[0]["target"], "seshat::metric");
        assert_eq!(
            [
                &lines[0]["metric"],
                &lines[0]["value"],
                &lines[0]["operation"],
                &lines[0]["class"],
                &lines[0]["status"]
            ],
            [
                &json!("Requests"),
                &json!(1),
                &json!("GetCity"),
                &json!("success"),
                &json!(200)
            ]
        );
        assert!(
            lines[1].get("operation").is_none(),
            "no operation, no dimension"
        );
    }
}
