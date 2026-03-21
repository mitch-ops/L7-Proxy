use prometheus::{
    Encoder, HistogramOpts, HistogramVec, IntCounterVec, Opts, Registry, TextEncoder,
};

#[derive(Clone)]
pub struct Metrics {
    pub registry: Registry,
    pub requests_total: IntCounterVec,
    pub request_duration_seconds: HistogramVec,
    pub retries_total: IntCounterVec,
    pub errors_total: IntCounterVec,
}

impl Metrics {
    pub fn new() -> Self {
        let registry = Registry::new();

        let requests_total = IntCounterVec::new(
            Opts::new("proxy_requests_total", "Total number of proxied requests"),
            &["method", "route", "status"],
        )
        .unwrap();

        let request_duration_seconds = HistogramVec::new(
            HistogramOpts::new(
                "proxy_request_duration_seconds",
                "Request latency in seconds",
            )
            .buckets(vec![0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0]),
            &["method", "route"],
        )
        .unwrap();

        let retries_total = IntCounterVec::new(
            Opts::new("proxy_retries_total", "Total number of retry attempts"),
            &["route"],
        )
        .unwrap();

        let errors_total = IntCounterVec::new(
            Opts::new("proxy_errors_total", "Total number of upstream errors"),
            &["route", "error_type"],
        )
        .unwrap();

        registry.register(Box::new(requests_total.clone())).unwrap();
        registry
            .register(Box::new(request_duration_seconds.clone()))
            .unwrap();
        registry.register(Box::new(retries_total.clone())).unwrap();
        registry.register(Box::new(errors_total.clone())).unwrap();

        Metrics {
            registry,
            requests_total,
            request_duration_seconds,
            retries_total,
            errors_total,
        }
    }

    pub fn render(&self) -> String {
        let encoder = TextEncoder::new();
        let metric_families = self.registry.gather();
        let mut buffer = Vec::new();
        encoder.encode(&metric_families, &mut buffer).unwrap();
        String::from_utf8(buffer).unwrap()
    }
}
