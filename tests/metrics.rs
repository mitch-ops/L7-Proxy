use rust_proxy::config::{Config, RouteConfig, ServerConfig};

use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::{routing::get, Router};
use std::net::SocketAddr;
use std::time::Duration;
use tokio::task;

async fn ok_handler() -> impl IntoResponse {
    (StatusCode::OK, "ok")
}

async fn fail_handler() -> impl IntoResponse {
    (StatusCode::INTERNAL_SERVER_ERROR, "fail")
}

async fn start_upstream(addr: SocketAddr, app: Router) {
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

/// Helper to start the proxy and a metrics server, returning (proxy_addr, metrics_addr)
async fn start_proxy_and_metrics(
    upstreams: Vec<String>,
    max_retries: usize,
) -> (SocketAddr, SocketAddr) {
    // Bind metrics listener first to get an ephemeral port
    let metrics_listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .unwrap();
    let metrics_addr = metrics_listener.local_addr().unwrap();

    let config = Config {
        server: ServerConfig {
            bind: "127.0.0.1:0".to_string(),
            request_timeout_secs: 2,
            max_retries,
            failure_threshold: 100,
            health_cooldown_secs: 60,
            health_check: None,
            retry_backoff_ms: 10,
            retry_budget_percent: 100.0,
            retry_budget_window_secs: 10,
            metrics_bind: Some(metrics_addr.to_string()),
        },
        routes: vec![RouteConfig {
            prefix: "/".to_string(),
            upstream: upstreams,
        }],
    };

    // start_proxy_with_config binds its own listener, but we need the metrics server too.
    // We'll set up state manually to control both listeners.
    use rust_proxy::balancer::RoundRobin;
    use rust_proxy::health::HealthTracker;
    use rust_proxy::metrics::Metrics;
    use rust_proxy::retry_budget::RetryBudget;
    use rust_proxy::router::{Route, Router as ProxyRouter};
    use rust_proxy::server::{start_metrics_server, start_server};
    use rust_proxy::state::AppState;
    use std::sync::Arc;

    let proxy_listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .unwrap();
    let proxy_addr = proxy_listener.local_addr().unwrap();

    let routes = config
        .routes
        .iter()
        .map(|r| Route {
            prefix: r.prefix.clone(),
            upstreams: r.upstream.clone(),
            balancer: RoundRobin::new(),
        })
        .collect();

    let router = Arc::new(ProxyRouter::new(routes));
    let client = hyper::Client::new();
    let health = Arc::new(HealthTracker::new(
        config.server.failure_threshold,
        std::time::Duration::from_secs(config.server.health_cooldown_secs),
    ));
    let retry_budget = Arc::new(RetryBudget::new(
        config.server.retry_budget_percent,
        config.server.retry_budget_window_secs,
    ));
    let metrics = Arc::new(Metrics::new());

    let state = Arc::new(AppState {
        router,
        client,
        config: Arc::new(config),
        health,
        retry_budget,
        metrics,
    });

    let metrics_state = state.clone();
    tokio::spawn(async move {
        start_metrics_server(metrics_state, metrics_listener).await;
    });

    tokio::spawn(async move {
        start_server(state, proxy_listener).await;
    });

    tokio::time::sleep(Duration::from_millis(100)).await;

    (proxy_addr, metrics_addr)
}

#[tokio::test]
async fn metrics_endpoint_records_requests() {
    let upstream_addr = SocketAddr::from(([127, 0, 0, 1], 9401));

    task::spawn(start_upstream(
        upstream_addr,
        Router::new().route("/", get(ok_handler)),
    ));

    tokio::time::sleep(Duration::from_millis(200)).await;

    let (proxy_addr, metrics_addr) = start_proxy_and_metrics(
        vec![format!("http://{}", upstream_addr)],
        2,
    )
    .await;

    // Send 3 successful requests
    for _ in 0..3 {
        let resp = reqwest::get(format!("http://{}", proxy_addr))
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
    }

    // Fetch metrics
    let metrics_body = reqwest::get(format!("http://{}/metrics", metrics_addr))
        .await
        .unwrap()
        .text()
        .await
        .unwrap();

    // Verify request counter
    assert!(
        metrics_body.contains("proxy_requests_total"),
        "metrics should contain proxy_requests_total"
    );
    assert!(
        metrics_body.contains(r#"method="GET"#),
        "metrics should contain method label"
    );
    assert!(
        metrics_body.contains(r#"status="200"#),
        "metrics should contain status=200"
    );

    // Verify latency histogram
    assert!(
        metrics_body.contains("proxy_request_duration_seconds"),
        "metrics should contain proxy_request_duration_seconds"
    );

    // Verify no retries recorded (all succeeded on first attempt)
    assert!(
        !metrics_body.contains("proxy_retries_total{"),
        "no retries should be recorded for successful requests"
    );
}

#[tokio::test]
async fn metrics_records_retries_and_errors() {
    let fail_addr = SocketAddr::from(([127, 0, 0, 1], 9403));
    let ok_addr = SocketAddr::from(([127, 0, 0, 1], 9404));

    task::spawn(start_upstream(
        fail_addr,
        Router::new().route("/", get(fail_handler)),
    ));
    task::spawn(start_upstream(
        ok_addr,
        Router::new().route("/", get(ok_handler)),
    ));

    tokio::time::sleep(Duration::from_millis(200)).await;

    let (proxy_addr, metrics_addr) = start_proxy_and_metrics(
        vec![
            format!("http://{}", fail_addr),
            format!("http://{}", ok_addr),
        ],
        5,
    )
    .await;

    // Send a request — first upstream fails (5xx), retries to second (200)
    let resp = reqwest::get(format!("http://{}", proxy_addr))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    // Fetch metrics
    let metrics_body = reqwest::get(format!("http://{}/metrics", metrics_addr))
        .await
        .unwrap()
        .text()
        .await
        .unwrap();

    // Verify retry was recorded
    assert!(
        metrics_body.contains("proxy_retries_total"),
        "metrics should contain proxy_retries_total"
    );

    // Verify 5xx error was recorded
    assert!(
        metrics_body.contains(r#"error_type="5xx"#),
        "metrics should record 5xx error type"
    );

    // Final response should be 200
    assert!(
        metrics_body.contains(r#"status="200"#),
        "successful response should be recorded"
    );
}
